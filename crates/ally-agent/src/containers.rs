//! Containers a run or session started with `docker run`, so stopping the job stops them
//! too. Docker runs a container apart from the `docker` command, so ending the command
//! alone can leave its container running. Compose stops its own containers on Ctrl C.

use ally_core::telemetry::is_installed;
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::Path;
use std::process::Command;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

#[derive(Debug, Clone, PartialEq)]
pub struct Container {
    id: String,
    pub name: String,
}

/// The containers a job started. `unsure` holds those another `docker run` could also have
/// started, which are never stopped, because they may not be the job's.
#[derive(Debug, Default, PartialEq)]
pub struct Found {
    pub ours: Vec<Container>,
    pub unsure: Vec<Container>,
}

/// Look while the job's processes still run: `tree` is the job's processes with their start
/// times, and `since` is when the job started. Docker that is missing or refuses this
/// account finds nothing, which leaves stopping as it was.
pub fn started_by(tree: &HashMap<u32, u64>, since: u64) -> Found {
    if !is_installed("docker") {
        return Found::default();
    }
    let (ours, others): (Vec<_>, Vec<_>) = docker_runs()
        .into_iter()
        .partition(|(pid, start)| tree.get(pid) == Some(start));
    if ours.is_empty() {
        return Found::default();
    }
    let starts =
        |runs: Vec<(u32, u64)>| -> Vec<u64> { runs.into_iter().map(|(_, start)| start).collect() };
    let events = docker(&[
        "events",
        "--since",
        &since.to_string(),
        "--until",
        &ally_core::storage::now().to_string(),
        "--filter",
        "type=container",
        "--filter",
        "event=start",
        "--format",
        "{{.Actor.ID}}\t{{index .Actor.Attributes \"name\"}}\t{{.Time}}",
    ]);
    classify(
        &starts(ours),
        &starts(others),
        &from_events(&events),
        &running(),
    )
}

/// Stop the job's containers that still run once the job has ended. Returns the names of
/// those that are stopped, and those that may still run.
pub fn stop(ours: Vec<Container>) -> (Vec<String>, Vec<String>) {
    let still = running();
    let (live, ended): (Vec<_>, Vec<_>) = ours.into_iter().partition(|c| still.contains(&c.id));
    let mut stopped: Vec<String> = ended.into_iter().map(|c| c.name).collect();
    let mut failed = Vec::new();
    if !live.is_empty() {
        let ids: Vec<&str> = live.iter().map(|c| c.id.as_str()).collect();
        let answered = docker(&[&["stop"], ids.as_slice()].concat());
        let confirmed: HashSet<&str> = answered.lines().map(str::trim).collect();
        for container in live {
            match confirmed.contains(container.id.as_str()) {
                true => stopped.push(container.name),
                false => failed.push(container.name),
            }
        }
    }
    (stopped, failed)
}

/// Seconds of slack when comparing a process's start with a container's, since Linux
/// works out a process's start time from the boot time, which can be a second off.
const SLACK: u64 = 1;

/// A container counts as the job's when one of the job's `docker run` commands started
/// before it, and as unsure when another `docker run` had also started by then.
fn classify(
    ours: &[u64],
    others: &[u64],
    events: &[(Container, u64)],
    running: &HashSet<String>,
) -> Found {
    let mut found = Found::default();
    let mut seen = HashSet::new();
    for (container, at) in events {
        if !running.contains(&container.id) || !seen.insert(&container.id) {
            continue;
        }
        if !ours.iter().any(|start| *start <= at + SLACK) {
            continue;
        }
        match others.iter().any(|start| *start <= at + SLACK) {
            true => found.unsure.push(container.clone()),
            false => found.ours.push(container.clone()),
        }
    }
    found
}

/// Every live `docker run` or `docker container run` on the machine, with its start time.
fn docker_runs() -> Vec<(u32, u64)> {
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always),
    );
    system
        .processes()
        .iter()
        .filter(|(_, process)| is_docker_run(process.cmd()))
        .map(|(pid, process)| (pid.as_u32(), process.start_time()))
        .collect()
}

/// True for `docker run ...` and `docker container run ...`, after any options.
fn is_docker_run(command: &[OsString]) -> bool {
    let Some(program) = command.first() else {
        return false;
    };
    if Path::new(program).file_name() != Some("docker".as_ref()) {
        return false;
    }
    let mut words = command[1..]
        .iter()
        .filter_map(|word| word.to_str())
        .filter(|word| !word.starts_with('-'));
    match words.next() {
        Some("run") => true,
        Some("container") => words.next() == Some("run"),
        _ => false,
    }
}

/// Lines of `docker events`: a container's full ID, its name, and when it started.
fn from_events(text: &str) -> Vec<(Container, u64)> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split('\t');
            let id = parts.next()?.to_string();
            let name = parts.next()?.to_string();
            let at = parts.next()?.trim().parse().ok()?;
            Some((Container { id, name }, at))
        })
        .collect()
}

fn running() -> HashSet<String> {
    docker(&["ps", "--no-trunc", "--format", "{{.ID}}"])
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|id| !id.is_empty())
        .collect()
}

/// What a docker command printed, or nothing when it could not run.
fn docker(args: &[&str]) -> String {
    Command::new("docker")
        .args(args)
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn container(id: &str) -> Container {
        Container {
            id: id.to_string(),
            name: format!("{id}-name"),
        }
    }

    fn words(line: &str) -> Vec<OsString> {
        line.split(' ').map(OsString::from).collect()
    }

    #[test]
    fn only_docker_run_commands_count() {
        assert!(is_docker_run(&words("docker run --rm alpine sleep 9")));
        assert!(is_docker_run(&words(
            "/usr/bin/docker -D container run nginx"
        )));
        assert!(!is_docker_run(&words("docker compose up")));
        assert!(!is_docker_run(&words("docker ps")));
        assert!(!is_docker_run(&words("dockerd run")));
        assert!(!is_docker_run(&[]));
    }

    #[test]
    fn events_give_the_id_name_and_start() {
        let text = "f49a\tweb-1\t1793739250\nbroken line\n";
        let web = Container {
            id: "f49a".to_string(),
            name: "web-1".to_string(),
        };
        assert_eq!(from_events(text), vec![(web, 1793739250)]);
    }

    /// Only a running container that started after the job's own `docker run` is the job's.
    /// When another `docker run` had started by then too, it is unsure and never stopped.
    #[test]
    fn a_container_is_the_jobs_only_when_no_other_docker_run_could_own_it() {
        let events = vec![
            (container("early"), 90),
            (container("mine"), 105),
            (container("mine"), 106),
            (container("gone"), 107),
            (container("shared"), 130),
        ];
        let running: HashSet<String> = ["early", "mine", "shared"].map(String::from).into();

        let found = classify(&[100], &[120], &events, &running);
        let close = classify(&[106], &[], &[(container("mine"), 105)], &running);

        assert_eq!(found.ours, vec![container("mine")]);
        assert_eq!(found.unsure, vec![container("shared")]);
        assert_eq!(close.ours, vec![container("mine")]);
    }

    #[test]
    fn nothing_is_the_jobs_without_its_own_docker_run() {
        let events = vec![(container("other"), 105)];
        let running: HashSet<String> = ["other".to_string()].into();

        assert_eq!(classify(&[], &[100], &events, &running), Found::default());
    }
}
