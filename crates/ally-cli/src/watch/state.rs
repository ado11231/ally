//! Deciding when something is worth a notification, and saying it the way a person would.
//! Each observation of the Agent goes in, and the notices it earns come out, so the rules
//! can be tested without an Agent.

use super::event::{self, Machine, Notice, NoticeKind};
use crate::commands::health::{MEMORY, TEMPERATURE};
use ally_core::control::{Job, JobKind, JobState, Listening, Source};
use ally_core::presentation::{capacity, plural};
use ally_core::protocol::Health;
use ally_core::telemetry::DISK_WARNING_MIB;
use std::collections::{HashMap, HashSet};

/// A run shorter than this finished while you were still looking at it.
const WORTH_NOTICING: u64 = 10;

/// Failed checks in a row before the Agent counts as unreachable, so one slow answer is not
/// reported as an outage.
const FAILURES_BEFORE_UNREACHABLE: u32 = 2;

/// Seconds since the first failed check before the Agent counts as unreachable, so a
/// switch between networks, which reconnects within this, is not reported as an outage.
const SILENT_BEFORE_UNREACHABLE: u64 = 60;

/// Free workspace space must recover this far past the warning before it can warn again.
const DISK_RECOVERED_MIB: u64 = DISK_WARNING_MIB * 2;

const COMMAND_WIDTH: usize = 40;

/// At most this many job notices within `BURST_WINDOW` seconds of each other. The rest
/// become one summary, so a test loop or a restart does not flood Notification Center.
const BURST_LIMIT: usize = 3;
const BURST_WINDOW: u64 = 60;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Alarm {
    Memory,
    Workspace,
    Temperature(usize),
}

pub struct Watch {
    name: String,
    /// Finished jobs already accounted for. Empty until the first answer sets the baseline.
    known: Option<HashSet<String>>,
    failures: u32,
    /// When the first check failed since the Agent last answered.
    silent_since: Option<u64>,
    unreachable: bool,
    alarms: HashSet<Alarm>,
    /// Servers in Ally jobs. Empty until the first answer sets the baseline.
    servers: Option<Servers>,
    /// When each job told within the last `BURST_WINDOW` seconds ended, by the Agent's clock.
    recent: Vec<u64>,
}

/// Job notices held back by a burst, counted by how the jobs ended.
#[derive(Default)]
struct Held {
    finished: usize,
    failed: usize,
    interrupted: usize,
}

/// A port a job listens on, by job ID.
type Server = (String, u16);

#[derive(Default)]
struct Servers {
    /// Told already, kept while the job runs, so a restarting dev server is told once.
    announced: HashSet<Server>,
    /// Seen once without a forward, given one more look so the notice can say where it opens.
    waiting: HashSet<Server>,
}

impl Watch {
    pub fn new(name: &str) -> Watch {
        Watch {
            name: name.to_string(),
            known: None,
            failures: 0,
            silent_since: None,
            unreachable: false,
            alarms: HashSet::new(),
            servers: None,
            recent: Vec::new(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Failed checks since the Agent last answered.
    pub fn failures(&self) -> u32 {
        self.failures
    }

    /// The Agent answered, over the path named `path`.
    pub fn reached(&mut self, path: &str) -> Vec<Notice> {
        self.failures = 0;
        self.silent_since = None;
        if !std::mem::take(&mut self.unreachable) {
            return Vec::new();
        }
        vec![Notice {
            kind: NoticeKind::Back,
            title: format!("{} is back", self.name),
            subtitle: None,
            body: format!("Via {path}"),
            open: None,
        }]
    }

    /// The Agent did not answer at `now`, for the reason in `error`. The notice uses the same
    /// plain words as the offline screen rather than the raw error, plus the fix when there
    /// is one.
    pub fn failed(&mut self, error: &str, now: u64) -> Vec<Notice> {
        self.failures += 1;
        let since = *self.silent_since.get_or_insert(now);
        if self.unreachable
            || self.failures < FAILURES_BEFORE_UNREACHABLE
            || now.saturating_sub(since) < SILENT_BEFORE_UNREACHABLE
        {
            return Vec::new();
        }
        self.unreachable = true;
        let problem = event::explain(&self.name, error);
        let body = match problem.fix {
            Some(fix) => format!(
                "{} Run {} on {}.",
                problem.detail,
                fix.command,
                match fix.machine {
                    Machine::Agent => self.name.as_str(),
                    Machine::Client => "this machine",
                }
            ),
            None => problem.detail,
        };
        vec![Notice {
            kind: NoticeKind::Unreachable,
            title: problem.title,
            subtitle: None,
            body,
            open: None,
        }]
    }

    /// Jobs that finished since the last list. The first list only sets the baseline, so
    /// starting the menu bar never replays history.
    pub fn jobs(&mut self, jobs: &[Job]) -> Vec<Notice> {
        let finished: Vec<&Job> = jobs.iter().filter(|job| !job.state.active()).collect();
        let Some(known) = &mut self.known else {
            self.known = Some(finished.iter().map(|job| job.id.clone()).collect());
            return Vec::new();
        };
        let ended: Vec<&Job> = finished
            .iter()
            .filter(|job| known.insert(job.id.clone()))
            .copied()
            .collect();
        let mut notices = Vec::new();
        let mut held = Held::default();
        for job in ended {
            let Some(notice) = notice(&self.name, job) else {
                continue;
            };
            let at = job.ended.unwrap_or_default();
            self.recent
                .retain(|told| at.saturating_sub(*told) < BURST_WINDOW);
            if self.recent.len() < BURST_LIMIT {
                self.recent.push(at);
                notices.push(notice);
                continue;
            }
            match notice.kind {
                NoticeKind::JobFailed => held.failed += 1,
                NoticeKind::JobInterrupted => held.interrupted += 1,
                _ => held.finished += 1,
            }
        }
        notices.extend(summary(&self.name, &held));
        let listed: HashSet<&str> = jobs.iter().map(|job| job.id.as_str()).collect();
        known.retain(|id| listed.contains(id.as_str()));
        notices
    }

    /// Servers that just started listening in an Ally job, with where each opens here when
    /// it is forwarded. A port is told once it is forwarded, or on its second look, about 4
    /// seconds later, so a forward that is on its way can still be named. The first list
    /// only sets the baseline, so starting the menu bar never announces old servers.
    pub fn servers(
        &mut self,
        ports: &[Listening],
        jobs: &[Job],
        here: &HashMap<u16, u16>,
    ) -> Vec<Notice> {
        let listening: Vec<(&Listening, &str)> = ports
            .iter()
            .filter_map(|port| match &port.from {
                Source::Job(id) => Some((port, id.as_str())),
                _ => None,
            })
            .collect();
        let Some(servers) = &mut self.servers else {
            self.servers = Some(Servers {
                announced: listening
                    .iter()
                    .map(|(port, id)| (id.to_string(), port.port))
                    .collect(),
                waiting: HashSet::new(),
            });
            return Vec::new();
        };
        let mut notices = Vec::new();
        for (port, id) in &listening {
            let server = (id.to_string(), port.port);
            if servers.announced.contains(&server) {
                continue;
            }
            let local = here.get(&port.port).copied();
            if local.is_none() && servers.waiting.insert(server.clone()) {
                continue;
            }
            servers.waiting.remove(&server);
            servers.announced.insert(server);
            let job = jobs.iter().find(|job| job.id == *id);
            notices.push(ready(&self.name, port, job, local));
        }
        let active: HashSet<&str> = jobs
            .iter()
            .filter(|job| job.state.active())
            .map(|job| job.id.as_str())
            .collect();
        servers
            .announced
            .retain(|(id, _)| active.contains(id.as_str()));
        servers.waiting.retain(|(id, number)| {
            listening
                .iter()
                .any(|(port, job)| job == id && port.port == *number)
        });
        notices
    }

    /// Resources that just crossed into trouble. Each warns once, then again only after
    /// it has clearly recovered.
    pub fn health(&mut self, health: &Health) -> Vec<Notice> {
        let mut notices = Vec::new();
        let (used, total) = (health.memory_used_mib, health.memory_total_mib);
        if total > 0 && used <= total {
            let percent = used as f64 / total as f64 * 100.0;
            if self.alarm(
                Alarm::Memory,
                percent >= MEMORY.high,
                percent < MEMORY.warning,
            ) {
                notices.push(resource(
                    format!("{} is low on memory", self.name),
                    format!("{percent:.0}% in use"),
                ));
            }
        }
        let free = health.workspace_free_mib.unwrap_or(health.disk_free_mib);
        if self.alarm(
            Alarm::Workspace,
            free < DISK_WARNING_MIB,
            free >= DISK_RECOVERED_MIB,
        ) {
            notices.push(resource(
                format!("{} is low on disk", self.name),
                format!("{} left", capacity(free)),
            ));
        }
        for (index, gpu) in health.gpus.iter().enumerate() {
            let Some(celsius) = gpu.temperature_c else {
                continue;
            };
            let value = celsius as f64;
            if self.alarm(
                Alarm::Temperature(index),
                value >= TEMPERATURE.high,
                value < TEMPERATURE.warning,
            ) {
                notices.push(resource(
                    format!("{}'s GPU is hot", self.name),
                    format!("{celsius}°C"),
                ));
            }
        }
        notices
    }

    /// True when `alarm` should fire now. It re-arms only once `recovered` holds.
    fn alarm(&mut self, alarm: Alarm, triggered: bool, recovered: bool) -> bool {
        if recovered {
            self.alarms.remove(&alarm);
            return false;
        }
        triggered && self.alarms.insert(alarm)
    }
}

fn resource(title: String, body: String) -> Notice {
    Notice {
        kind: NoticeKind::Resource,
        title,
        subtitle: None,
        body,
        open: None,
    }
}

/// What a finished job is worth saying. The command is the subtitle, so the title can stay
/// short and plain. Stopping is something the person just did, whether with `ally stop`
/// or with Ctrl C, which ends a run with exit code 130. A session usually ends because
/// someone left it. So only surprises about those are told.
fn notice(name: &str, job: &Job) -> Option<Notice> {
    let took = job.ended?.saturating_sub(job.started);
    let place = job
        .project_name
        .as_ref()
        .map(|project| format!(" · {project}"))
        .unwrap_or_default();
    let what = match job.kind {
        JobKind::Run => short(&job.command),
        JobKind::Session => "Session".to_string(),
    };
    let (kind, title, body) = match (job.kind, job.state) {
        (JobKind::Run, JobState::Interrupted) if job.exit_code == Some(130) => return None,
        (_, JobState::Interrupted) => (
            NoticeKind::JobInterrupted,
            format!("Interrupted on {name}"),
            format!("Connection dropped or {name} restarted"),
        ),
        (JobKind::Run, _) if took < WORTH_NOTICING => return None,
        (JobKind::Run, JobState::Completed) => (
            NoticeKind::JobFinished,
            format!("Finished on {name}"),
            format!("{}{place}", brief(took)),
        ),
        (JobKind::Run, JobState::Failed) => (
            NoticeKind::JobFailed,
            format!("Failed on {name}"),
            match job.exit_code {
                Some(code) => format!("Exit {code} · {}{place}", brief(took)),
                None => format!("{}{place}", brief(took)),
            },
        ),
        _ => return None,
    };
    Some(Notice {
        kind,
        title,
        subtitle: Some(what),
        body,
        open: None,
    })
}

/// One notice for the jobs a burst held back. Failures come first and are never left out.
fn summary(name: &str, held: &Held) -> Option<Notice> {
    let total = held.finished + held.failed + held.interrupted;
    if total == 0 {
        return None;
    }
    let body = [
        (held.failed, "failed"),
        (held.interrupted, "interrupted"),
        (held.finished, "finished"),
    ]
    .iter()
    .filter(|(count, _)| *count > 0)
    .map(|(count, word)| format!("{count} {word}"))
    .collect::<Vec<_>>()
    .join(" · ");
    let kind = match held.failed + held.interrupted {
        0 => NoticeKind::JobFinished,
        _ => NoticeKind::JobFailed,
    };
    Some(Notice {
        kind,
        title: format!("{} ended on {name}", plural(total, "more job")),
        subtitle: None,
        body,
        open: None,
    })
}

/// A server that is ready: what started it, and where to open it. A click opens a forwarded
/// one, and one that is not forwarded says how to forward it.
fn ready(name: &str, port: &Listening, job: Option<&Job>, local: Option<u16>) -> Notice {
    let what = match job {
        Some(job) if job.kind == JobKind::Run => short(&job.command),
        _ => port.program.clone(),
    };
    let place = job
        .and_then(|job| job.project_name.as_ref())
        .map(|project| format!(" · {project}"))
        .unwrap_or_default();
    let number = port.port;
    let (body, open) = match local {
        Some(local) => (
            format!("localhost:{local}"),
            Some(format!("http://localhost:{local}")),
        ),
        None => (
            format!("{name}:{number} · Run ally forward {number} to open it here"),
            None,
        ),
    };
    Notice {
        kind: NoticeKind::ServerReady,
        title: format!("Server ready on {name}"),
        subtitle: Some(format!("{what}{place}")),
        body,
        open,
    }
}

/// `12 sec`, `3 min`, or `1 hr 5 min`, short enough for a notification.
fn brief(seconds: u64) -> String {
    let (hours, minutes) = (seconds / 3600, seconds % 3600 / 60);
    match (hours, minutes) {
        (0, 0) => format!("{seconds} sec"),
        (0, minutes) => format!("{minutes} min"),
        (hours, 0) => format!("{hours} hr"),
        (hours, minutes) => format!("{hours} hr {minutes} min"),
    }
}

fn short(command: &str) -> String {
    match command.chars().count() > COMMAND_WIDTH {
        true => format!(
            "{}…",
            command.chars().take(COMMAND_WIDTH - 1).collect::<String>()
        ),
        false => command.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ally_core::protocol::GpuHealth;

    fn job(id: &str, kind: JobKind, state: JobState, took: u64) -> Job {
        Job {
            id: id.to_string(),
            kind,
            project: Some("p".to_string()),
            project_name: Some("app".to_string()),
            command: "cargo build --release".to_string(),
            state,
            started: 1_000,
            ended: (!state.active()).then_some(1_000 + took),
            exit_code: match state {
                JobState::Completed => Some(0),
                JobState::Failed => Some(101),
                _ => None,
            },
            pid: None,
            process_start: None,
            boot: 0,
            stop_requested: false,
        }
    }

    fn run(id: &str, state: JobState, took: u64) -> Job {
        job(id, JobKind::Run, state, took)
    }

    fn health(memory_used_mib: u64, workspace_free_mib: u64, celsius: u32) -> Health {
        Health {
            cpu_percent: 10.0,
            memory_used_mib,
            memory_total_mib: 100,
            swap_total_mib: 0,
            disk_free_mib: 0,
            workspace_free_mib: Some(workspace_free_mib),
            gpus: vec![GpuHealth {
                name: "RTX 3090".to_string(),
                vram_free_mib: None,
                vram_total_mib: None,
                utilization_percent: None,
                temperature_c: Some(celsius),
            }],
            gpu_problem: None,
        }
    }

    const PLENTY: u64 = 100_000;

    fn port(number: u16, program: &str, from: Source) -> Listening {
        Listening {
            port: number,
            address: "127.0.0.1".to_string(),
            program: program.to_string(),
            from,
        }
    }

    fn running(id: &str) -> Job {
        job(id, JobKind::Run, JobState::Running, 0)
    }

    #[test]
    fn a_new_server_is_told_once_with_the_address_to_open() {
        let mut watch = Watch::new("archbox");
        let jobs = [running("old"), running("new")];
        let old = [port(3000, "node", Source::Job("old".to_string()))];
        assert!(watch.servers(&old, &jobs, &HashMap::new()).is_empty());
        let both = [
            port(3000, "node", Source::Job("old".to_string())),
            port(8765, "python", Source::Job("new".to_string())),
            port(5432, "db", Source::Docker),
        ];
        let here = HashMap::from([(8765, 8766)]);
        let notices = watch.servers(&both, &jobs, &here);
        assert_eq!(
            notices,
            [Notice {
                kind: NoticeKind::ServerReady,
                title: "Server ready on archbox".to_string(),
                subtitle: Some("cargo build --release · app".to_string()),
                body: "localhost:8766".to_string(),
                open: Some("http://localhost:8766".to_string()),
            }]
        );
        assert!(watch.servers(&both, &jobs, &here).is_empty());
    }

    /// A forward usually follows within seconds, so one look is not enough to give up on it.
    #[test]
    fn a_server_not_forwarded_waits_one_look_then_says_how_to_forward_it() {
        let mut watch = Watch::new("archbox");
        let jobs = [running("a")];
        watch.servers(&[], &jobs, &HashMap::new());
        let ports = [port(5173, "node", Source::Job("a".to_string()))];
        assert!(watch.servers(&ports, &jobs, &HashMap::new()).is_empty());
        let notices = watch.servers(&ports, &jobs, &HashMap::new());
        assert_eq!(
            notices[0].body,
            "archbox:5173 · Run ally forward 5173 to open it here"
        );
        assert_eq!(notices[0].open, None);
    }

    /// A dev server that restarts closes and reopens its port, which is not news.
    #[test]
    fn a_restarting_server_is_not_told_again_while_its_job_runs() {
        let mut watch = Watch::new("archbox");
        let jobs = [running("a")];
        let ports = [port(5173, "node", Source::Job("a".to_string()))];
        let here = HashMap::from([(5173, 5173)]);
        watch.servers(&[], &jobs, &here);
        assert_eq!(watch.servers(&ports, &jobs, &here).len(), 1);
        assert!(watch.servers(&[], &jobs, &here).is_empty());
        assert!(watch.servers(&ports, &jobs, &here).is_empty());
    }

    #[test]
    fn the_first_list_sets_a_baseline() {
        let mut watch = Watch::new("archbox");
        let old = [
            run("a", JobState::Completed, 60),
            run("b", JobState::Failed, 60),
        ];
        assert!(watch.jobs(&old).is_empty());
        assert!(watch.jobs(&old).is_empty());
    }

    #[test]
    fn a_long_run_that_ends_is_told_once() {
        let mut watch = Watch::new("archbox");
        watch.jobs(&[run("a", JobState::Running, 0)]);
        let notices = watch.jobs(&[run("a", JobState::Completed, 192)]);
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0].kind, NoticeKind::JobFinished);
        assert_eq!(notices[0].title, "Finished on archbox");
        assert_eq!(
            notices[0].subtitle.as_deref(),
            Some("cargo build --release")
        );
        assert_eq!(notices[0].body, "3 min · app");
        assert!(watch.jobs(&[run("a", JobState::Completed, 192)]).is_empty());
    }

    fn ended_at(id: &str, state: JobState, at: u64) -> Job {
        let mut job = run(id, state, 30);
        job.ended = Some(at);
        job
    }

    #[test]
    fn a_burst_of_runs_becomes_one_summary_that_counts_failures() {
        let mut watch = Watch::new("archbox");
        watch.jobs(&[]);
        let states = [
            JobState::Completed,
            JobState::Completed,
            JobState::Completed,
            JobState::Failed,
            JobState::Completed,
            JobState::Failed,
            JobState::Interrupted,
            JobState::Completed,
            JobState::Completed,
            JobState::Completed,
        ];
        let jobs: Vec<Job> = states
            .iter()
            .enumerate()
            .map(|(index, state)| ended_at(&index.to_string(), *state, 2_000 + index as u64))
            .collect();
        let notices = watch.jobs(&jobs);
        assert_eq!(notices.len(), BURST_LIMIT + 1);
        let summary = notices.last().unwrap();
        assert_eq!(summary.title, "7 more jobs ended on archbox");
        assert_eq!(summary.body, "2 failed · 1 interrupted · 4 finished");
        assert_eq!(summary.kind, NoticeKind::JobFailed);
    }

    #[test]
    fn runs_ending_far_apart_are_each_told() {
        let mut watch = Watch::new("archbox");
        watch.jobs(&[]);
        let mut seen = Vec::new();
        for index in 0..5u64 {
            let job = ended_at(
                &index.to_string(),
                JobState::Completed,
                2_000 + index * BURST_WINDOW,
            );
            seen.push(job);
            let notices = watch.jobs(&seen);
            assert_eq!(notices.len(), 1);
            assert_eq!(notices[0].title, "Finished on archbox");
        }
    }

    #[test]
    fn a_burst_across_checks_is_still_held_and_a_quiet_one_has_no_sound() {
        let mut watch = Watch::new("archbox");
        watch.jobs(&[]);
        let first: Vec<Job> = (0..3)
            .map(|index| ended_at(&format!("a{index}"), JobState::Completed, 2_000 + index))
            .collect();
        assert_eq!(watch.jobs(&first).len(), 3);
        let mut both = first.clone();
        both.push(ended_at("b", JobState::Completed, 2_010));
        let notices = watch.jobs(&both);
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0].title, "1 more job ended on archbox");
        assert_eq!(notices[0].body, "1 finished");
        assert_eq!(notices[0].kind, NoticeKind::JobFinished);
    }

    #[test]
    fn a_failure_names_the_exit_code() {
        let mut watch = Watch::new("archbox");
        watch.jobs(&[]);
        let notices = watch.jobs(&[run("a", JobState::Failed, 12)]);
        assert_eq!(notices[0].kind, NoticeKind::JobFailed);
        assert_eq!(notices[0].title, "Failed on archbox");
        assert_eq!(notices[0].body, "Exit 101 · 12 sec · app");
    }

    #[test]
    fn short_and_stopped_runs_and_ended_sessions_stay_quiet() {
        let mut watch = Watch::new("archbox");
        watch.jobs(&[]);
        assert!(
            watch
                .jobs(&[
                    run("a", JobState::Completed, 9),
                    run("b", JobState::Failed, 2),
                    run("c", JobState::Stopped, 600),
                    job("d", JobKind::Session, JobState::Ended, 600),
                ])
                .is_empty()
        );
    }

    #[test]
    fn interruptions_are_always_told() {
        let mut watch = Watch::new("archbox");
        watch.jobs(&[]);
        let notices = watch.jobs(&[
            job("a", JobKind::Session, JobState::Interrupted, 3),
            run("b", JobState::Interrupted, 1),
        ]);
        assert_eq!(notices.len(), 2);
        assert!(
            notices
                .iter()
                .all(|notice| notice.kind == NoticeKind::JobInterrupted)
        );
        assert_eq!(notices[0].title, "Interrupted on archbox");
        assert_eq!(notices[0].subtitle.as_deref(), Some("Session"));
        assert_eq!(notices[1].body, "Connection dropped or archbox restarted");
    }

    #[test]
    fn ctrl_c_is_not_told_but_a_lost_connection_is() {
        let mut watch = Watch::new("archbox");
        watch.jobs(&[]);
        let mut ctrl_c = run("a", JobState::Interrupted, 30);
        ctrl_c.exit_code = Some(130);
        let lost = run("b", JobState::Interrupted, 30);
        let notices = watch.jobs(&[ctrl_c, lost]);
        assert_eq!(notices.len(), 1);
        assert!(notices[0].body.contains("Connection dropped"));
    }

    #[test]
    fn long_commands_are_shortened() {
        let long = "x".repeat(60);
        let shortened = short(&long);
        assert_eq!(shortened.chars().count(), COMMAND_WIDTH);
        assert!(shortened.ends_with('…'));
    }

    #[test]
    fn durations_are_brief() {
        assert_eq!(brief(1), "1 sec");
        assert_eq!(brief(48), "48 sec");
        assert_eq!(brief(192), "3 min");
        assert_eq!(brief(3600), "1 hr");
        assert_eq!(brief(3900), "1 hr 5 min");
    }

    #[test]
    fn unreachable_needs_two_failures_and_is_told_once_then_back() {
        let refused = "Could not reach archbox: Ally is not running on the Agent. Run ally start there: Connection refused (os error 111)";
        let mut watch = Watch::new("archbox");
        assert!(watch.reached("local network").is_empty());
        assert!(watch.failed(refused, 1_000).is_empty());
        let notices = watch.failed(refused, 1_060);
        assert_eq!(notices[0].kind, NoticeKind::Unreachable);
        assert_eq!(notices[0].title, "archbox is unavailable");
        assert_eq!(notices[0].subtitle, None);
        assert_eq!(
            notices[0].body,
            "Ally stopped there. Run ally start on archbox."
        );
        assert!(watch.failed(refused, 1_090).is_empty());
        let notices = watch.reached("tailnet");
        assert_eq!(notices[0].kind, NoticeKind::Back);
        assert_eq!(notices[0].body, "Via tailnet");
        assert!(watch.reached("tailnet").is_empty());
    }

    #[test]
    fn an_offline_cause_without_a_fix_explains_itself() {
        let mut watch = Watch::new("archbox");
        watch.failed("something new", 1_000);
        let notices = watch.failed("something new", 1_060);
        assert_eq!(notices[0].title, "archbox is unavailable");
        assert_eq!(notices[0].body, "something new");
    }

    #[test]
    fn one_failure_between_answers_is_not_an_outage() {
        let mut watch = Watch::new("archbox");
        watch.failed("slow", 1_000);
        watch.reached("local network");
        assert!(watch.failed("slow", 1_060).is_empty());
    }

    #[test]
    fn a_network_switch_shorter_than_a_minute_is_not_an_outage() {
        let mut watch = Watch::new("archbox");
        assert!(watch.failed("did not answer in time", 1_000).is_empty());
        assert!(watch.failed("did not answer in time", 1_020).is_empty());
        assert!(watch.failed("did not answer in time", 1_059).is_empty());
        assert!(watch.reached("local network").is_empty());
    }

    #[test]
    fn resources_warn_once_and_rearm_after_recovering() {
        let mut watch = Watch::new("archbox");
        assert!(watch.health(&health(50, PLENTY, 60)).is_empty());
        let notices = watch.health(&health(95, PLENTY, 60));
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0].title, "archbox is low on memory");
        assert_eq!(notices[0].body, "95% in use");
        assert!(watch.health(&health(95, PLENTY, 60)).is_empty());
        assert!(watch.health(&health(80, PLENTY, 60)).is_empty());
        assert!(watch.health(&health(92, PLENTY, 60)).is_empty());
        assert!(watch.health(&health(70, PLENTY, 60)).is_empty());
        assert_eq!(watch.health(&health(91, PLENTY, 60)).len(), 1);
    }

    #[test]
    fn disk_and_temperature_warn_with_their_own_limits() {
        let mut watch = Watch::new("archbox");
        let notices = watch.health(&health(10, 1024, 90));
        let titles: Vec<_> = notices.iter().map(|notice| notice.title.as_str()).collect();
        assert_eq!(titles, ["archbox is low on disk", "archbox's GPU is hot"]);
        assert_eq!(notices[0].body, "1.0 GiB left");
        assert_eq!(notices[1].body, "90°C");
        assert!(watch.health(&health(10, DISK_WARNING_MIB, 80)).is_empty());
        assert!(watch.health(&health(10, 1024, 90)).is_empty());
        assert!(watch.health(&health(10, DISK_RECOVERED_MIB, 70)).is_empty());
        assert_eq!(watch.health(&health(10, 1024, 90)).len(), 2);
    }
}
