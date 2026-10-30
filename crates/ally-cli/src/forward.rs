//! Automatic port forwarding while `ally run` or `ally attach` is connected. The Agent
//! reports the ports its work listens on, each one is added to the shared ssh connection
//! with `ssh -O forward`, and removed with `ssh -O cancel` once it closes or the command ends.

use crate::client::Control;
use crate::ssh::RemoteCommand;
use ally_core::config::Agent;
use ally_core::control::{Listening, Request, Response, Source};
use ally_core::preflight;
use ally_core::presentation::{Style, Tone};
use ally_core::storage;
use std::collections::BTreeMap;
use std::fs::File;
use std::net::{Ipv6Addr, SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

/// How often the Agent is asked for its ports. The first time waits too, so `attach` has
/// labeled the session before its bar is redrawn.
const POLL_EVERY: Duration = Duration::from_secs(2);

/// How many ports after a taken one are tried before the system picks one.
const NEARBY: u16 = 100;

/// Where news of a forward goes.
pub enum Shown {
    /// Lines in this terminal. `raw` when ssh holds the terminal in raw mode, where a line
    /// needs its own carriage return.
    Terminal { raw: bool },
    /// The bar and a short message inside a tmux session on the Agent, since tmux owns the
    /// screen there. `label` is the bar without ports.
    Session {
        tmux: String,
        socket: String,
        id: String,
        label: String,
    },
}

impl Shown {
    /// tmux shows text as it is, so only the terminal gets color.
    fn style(&self) -> Style {
        match self {
            Shown::Terminal { .. } => Style::stderr(),
            Shown::Session { .. } => Style::new(false),
        }
    }

    /// Show this round's news. A session's bar is redrawn whenever the forwards changed.
    async fn tell(
        &self,
        agent: &Agent,
        news: &[String],
        forwards: &BTreeMap<u16, Forward>,
        changed: bool,
    ) {
        match self {
            Shown::Terminal { raw } => {
                for text in news {
                    print(text, *raw);
                }
            }
            Shown::Session {
                tmux,
                socket,
                id,
                label,
            } if changed || !news.is_empty() => {
                let mut remote = RemoteCommand::to(
                    agent,
                    tmux.clone(),
                    session_args(socket, id, &bar(label, forwards), news),
                );
                remote.tty = false;
                let _ = tokio::process::Command::new("ssh")
                    .args(remote.to_ssh_args())
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .kill_on_drop(true)
                    .status()
                    .await;
            }
            Shown::Session { .. } => {}
        }
    }
}

/// A port forwarded from the Agent: where it listens here, the `-L` value that removes it
/// again, and the claim that keeps other Ally commands from forwarding it too.
struct Forward {
    local: u16,
    spec: String,
    _claim: File,
}

/// Forwarding running in the background until `stop`, or until it is dropped.
pub struct Forwarding {
    stop: oneshot::Sender<()>,
    task: JoinHandle<()>,
}

impl Forwarding {
    pub fn start(agent: &Agent, shown: Shown) -> Forwarding {
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(forward(agent.clone(), shown, stopped));
        Forwarding { stop, task }
    }

    /// Remove every forward before returning, so none outlives the command.
    pub async fn stop(self) {
        let _ = self.stop.send(());
        let _ = tokio::time::timeout(Duration::from_secs(5), self.task).await;
    }
}

async fn forward(agent: Agent, shown: Shown, mut stopped: oneshot::Receiver<()>) {
    let style = shown.style();
    let mut forwards: BTreeMap<u16, Forward> = BTreeMap::new();
    let mut remote = RemoteCommand::to(&agent, String::new(), Vec::new());
    remote.tty = false;
    let Some(socket) = remote.shared.clone() else {
        let text = off(
            "no private folder could hold the shared ssh connection\n  Check that the temporary folder can be written",
            style,
        );
        shown.tell(&agent, &[text], &forwards, false).await;
        return;
    };
    let mut control = match Control::connect(&agent).await {
        Ok(control) => control,
        Err(error) => {
            let text = off(&format!("{error:#}"), style);
            shown.tell(&agent, &[text], &forwards, false).await;
            return;
        }
    };
    let mut failed: Vec<u16> = Vec::new();
    let mut first = true;
    loop {
        tokio::select! {
            _ = &mut stopped => break,
            _ = tokio::time::sleep(POLL_EVERY) => {}
        }
        let ports = match control.call(Request::Ports).await {
            Ok(Response::Ports(ports)) => ports
                .into_iter()
                .filter(|listening| listening.from.forwarded_automatically())
                .collect::<Vec<_>>(),
            Ok(_) => break,
            Err(error) => {
                if first {
                    let text = off(&format!("{error:#}"), style);
                    shown.tell(&agent, &[text], &forwards, false).await;
                }
                break;
            }
        };
        first = false;
        let mut news = Vec::new();
        let mut changed = false;
        let open: Vec<u16> = ports.iter().map(|listening| listening.port).collect();
        let closed: Vec<u16> = forwards
            .keys()
            .filter(|port| !open.contains(port))
            .copied()
            .collect();
        for port in closed {
            if let Some(forward) = forwards.remove(&port) {
                remote.change_forward("cancel", &forward.spec).await;
                changed = true;
            }
        }
        failed.retain(|port| open.contains(port));
        for listening in ports {
            if forwards.contains_key(&listening.port) || failed.contains(&listening.port) {
                continue;
            }
            let Some(mut claim) = claim(&socket, listening.port) else {
                continue;
            };
            remote
                .change_forward("cancel", &spec(listening.port, &listening))
                .await;
            let wanted = listening.port;
            let local = tokio::task::spawn_blocking(move || free_port(wanted))
                .await
                .ok()
                .flatten();
            let spec = local.map(|local| spec(local, &listening));
            match (local, spec) {
                (Some(local), Some(spec)) if remote.change_forward("forward", &spec).await => {
                    record(&mut claim, local);
                    news.push(added(&agent.name, &listening, local, style));
                    forwards.insert(
                        listening.port,
                        Forward {
                            local,
                            spec,
                            _claim: claim,
                        },
                    );
                    changed = true;
                }
                _ => {
                    news.push(not_forwarded(&agent.name, &listening, style));
                    failed.push(listening.port);
                }
            }
        }
        shown.tell(&agent, &news, &forwards, changed).await;
    }
    for forward in forwards.values() {
        remote.change_forward("cancel", &forward.spec).await;
    }
    control.close().await;
}

/// Claim an Agent port for this process. Every Ally command to the same Agent shares one
/// ssh connection, so a port another command already forwards is left to it. The claim is
/// a lock that the system drops when its process ends, however it ends, so the next round
/// of a command still running takes the port over. A command killed by a signal leaves its
/// forward in the shared connection, so holding the claim also means any forward of the
/// port to the same port here is left over, and is removed first.
pub fn claim(socket: &Path, port: u16) -> Option<File> {
    storage::try_lock(&claim_path(socket, port)).ok().flatten()
}

fn claim_path(socket: &Path, port: u16) -> PathBuf {
    socket.with_extension(format!("{port}.lock"))
}

/// Write where a claimed port opens here into its claim, so other commands can say so.
pub fn record(claim: &mut File, local: u16) {
    use std::io::{Seek, Write};

    let _ = claim
        .set_len(0)
        .and_then(|()| claim.rewind())
        .and_then(|()| write!(claim, "{local}"));
}

/// Where another Ally command forwards an Agent port here, or `None` when no live command
/// holds its claim.
pub fn forwarded_at(socket: &Path, port: u16) -> Option<u16> {
    let path = claim_path(socket, port);
    if !path.exists() || storage::try_lock(&path).ok()?.is_some() {
        return None;
    }
    std::fs::read_to_string(&path).ok()?.trim().parse().ok()
}

fn off(reason: &str, style: Style) -> String {
    style.status(format!("Port forwarding is off: {reason}"), Tone::Warning)
}

fn not_forwarded(agent: &str, listening: &Listening, style: Style) -> String {
    style.status(
        format!(
            "Could not forward {agent}:{} to this machine\n  Restart the server on the Agent to try again",
            listening.port
        ),
        Tone::Warning,
    )
}

fn print(text: &str, raw: bool) {
    match raw {
        true => eprint!("\r{}\r\n", text.replace('\n', "\r\n")),
        false => eprintln!("{text}"),
    }
}

/// The session bar with the forwarded ports, as in `▶ archbox via tailnet · app ·
/// localhost:5173 :3001`.
fn bar(label: &str, forwards: &BTreeMap<u16, Forward>) -> String {
    let ports: Vec<String> = forwards
        .values()
        .enumerate()
        .map(|(index, forward)| match index {
            0 => format!("localhost:{}", forward.local),
            _ => format!(":{}", forward.local),
        })
        .collect();
    match ports.is_empty() {
        true => label.to_string(),
        false => format!("{label} · {}", ports.join(" ")),
    }
}

/// One tmux call that redraws the bar and, when there is news, shows it for 4 seconds.
/// `#` starts tmux formatting, so it is dropped from the news, which names programs.
fn session_args(socket: &str, id: &str, bar: &str, news: &[String]) -> Vec<String> {
    let pane = format!("={id}:");
    let mut args: Vec<String> = ["-u", "-S", socket, "set-option", "-t", &pane, "@ally", bar]
        .map(String::from)
        .to_vec();
    if !news.is_empty() {
        let message = news.join(" · ").replace("\n  ", " · ").replace('#', "");
        args.extend(
            [";", "display-message", "-t", &pane, "-d", "4000", &message].map(String::from),
        );
    }
    args
}

/// `▶ Forwarding archbox:5173 to localhost:5173 · node`, with a second line when the
/// port was taken here. A Docker port names its container.
fn added(agent: &str, listening: &Listening, local: u16, style: Style) -> String {
    let owner = match listening.from {
        Source::Docker => format!("Docker {}", listening.program),
        _ => listening.program.clone(),
    };
    let mut text = format!(
        "Forwarding {} to {} · {owner}",
        style.paint(format!("{agent}:{}", listening.port), Tone::Info),
        style.paint(format!("localhost:{local}"), Tone::Info)
    );
    if local != listening.port {
        text.push_str(&format!("\n  Port {} is in use here", listening.port));
    }
    style.status(text, Tone::Info)
}

/// ssh's `-L` value for a port the Agent reported. It reaches the server at the address
/// it listens on, or at loopback when it listens on all.
fn spec(local: u16, listening: &Listening) -> String {
    let target = match listening.address.as_str() {
        "0.0.0.0" => "127.0.0.1".to_string(),
        "::" => "[::1]".to_string(),
        address if address.contains(':') => format!("[{address}]"),
        address => address.to_string(),
    };
    spec_to(local, &target, listening.port)
}

/// ssh's `-L` value. The forward listens on this machine's loopback address only.
pub fn spec_to(local: u16, target: &str, port: u16) -> String {
    format!("127.0.0.1:{local}:{target}:{port}")
}

/// The port to listen on here: `wanted` when nothing on this machine uses it, else the
/// next free one nearby, else one the system picks, which also covers ports below 1024
/// that only an administrator may open.
pub fn free_port(wanted: u16) -> Option<u16> {
    (wanted..=wanted.saturating_add(NEARBY))
        .filter(|port| *port != 0)
        .find(|port| free(*port))
        .or_else(|| {
            let listener = TcpListener::bind(("127.0.0.1", 0)).ok()?;
            listener.local_addr().ok().map(|address| address.port())
        })
}

/// Whether nothing here uses `port`. Something answering on either loopback address
/// counts, since a browser tries `localhost` on IPv6 first, and a server listening on
/// every address may not stop this process from opening the port too.
fn free(port: u16) -> bool {
    TcpListener::bind(("127.0.0.1", port)).is_ok()
        && !preflight::is_listening(SocketAddr::from(([127, 0, 0, 1], port)))
        && !preflight::is_listening(SocketAddr::from((Ipv6Addr::LOCALHOST, port)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listening(address: &str, port: u16, job: Option<&str>, program: &str) -> Listening {
        Listening {
            port,
            address: address.to_string(),
            program: program.to_string(),
            from: match job {
                Some(id) => Source::Job(id.to_string()),
                None => Source::Docker,
            },
        }
    }

    #[test]
    fn forwards_listen_on_loopback_and_reach_the_servers_own_address() {
        for (address, expected) in [
            ("127.0.0.1", "127.0.0.1:3001:127.0.0.1:3000"),
            ("0.0.0.0", "127.0.0.1:3001:127.0.0.1:3000"),
            ("::1", "127.0.0.1:3001:[::1]:3000"),
            ("::", "127.0.0.1:3001:[::1]:3000"),
            ("100.67.90.119", "127.0.0.1:3001:100.67.90.119:3000"),
        ] {
            assert_eq!(
                spec(3001, &listening(address, 3000, Some("ab12"), "node")),
                expected
            );
        }
    }

    #[test]
    fn a_taken_port_moves_to_the_next_free_one() {
        let taken = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = taken.local_addr().unwrap().port();
        let chosen = free_port(port).unwrap();
        assert_ne!(chosen, port);
        assert!(TcpListener::bind(("127.0.0.1", chosen)).is_ok());
    }

    fn forward(local: u16) -> Forward {
        Forward {
            local,
            spec: String::new(),
            _claim: tempfile(),
        }
    }

    fn tempfile() -> File {
        let path = std::env::temp_dir().join(format!("ally-test-{}", storage::new_id()));
        let file = File::create(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        file
    }

    /// Whether a dropped claim frees within a second. A process another test starts at that
    /// moment holds a copy of the lock until it runs its program, so the claim may outlive
    /// its holder by a few milliseconds.
    fn released(socket: &Path, port: u16) -> bool {
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        while forwarded_at(socket, port).is_some() {
            if std::time::Instant::now() > deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        true
    }

    /// Two commands to one Agent must not both forward a port, and a claim ends with its holder.
    #[test]
    fn only_one_holder_may_claim_a_port_until_it_lets_go() {
        let socket = std::env::temp_dir().join(format!("ally-test-{}.sock", storage::new_id()));
        let mut first = claim(&socket, 5173).unwrap();
        assert!(claim(&socket, 5173).is_none());
        assert!(claim(&socket, 3000).is_some());
        assert_eq!(forwarded_at(&socket, 5173), None);
        record(&mut first, 5174);
        assert_eq!(forwarded_at(&socket, 5173), Some(5174));
        assert_eq!(forwarded_at(&socket, 3000), None);
        drop(first);
        assert!(released(&socket, 5173));
        assert!(claim(&socket, 5173).is_some());
        let _ = std::fs::remove_file(socket.with_extension("5173.lock"));
        let _ = std::fs::remove_file(socket.with_extension("3000.lock"));
    }

    #[test]
    fn the_bar_lists_where_each_port_opens_here() {
        let mut forwards = BTreeMap::new();
        assert_eq!(bar("▶ archbox · app", &forwards), "▶ archbox · app");
        forwards.insert(5173, forward(5173));
        forwards.insert(3000, forward(3001));
        assert_eq!(
            bar("▶ archbox · app", &forwards),
            "▶ archbox · app · localhost:3001 :5173"
        );
    }

    #[test]
    fn session_news_is_one_message_without_tmux_formatting() {
        let quiet = session_args("/s", "ab12", "bar", &[]);
        assert_eq!(
            quiet,
            [
                "-u",
                "-S",
                "/s",
                "set-option",
                "-t",
                "=ab12:",
                "@ally",
                "bar"
            ]
        );
        let news = [
            "▶ Forwarding archbox:3000 to localhost:3001 · node\n  Port 3000 is in use here"
                .to_string(),
            "▶ Forwarding archbox:5432 to localhost:5432 · Docker #db".to_string(),
        ];
        let args = session_args("/s", "ab12", "bar", &news);
        assert_eq!(
            args[8..],
            [
                ";",
                "display-message",
                "-t",
                "=ab12:",
                "-d",
                "4000",
                "▶ Forwarding archbox:3000 to localhost:3001 · node · Port 3000 is in use here · ▶ Forwarding archbox:5432 to localhost:5432 · Docker db"
            ]
        );
    }

    #[test]
    fn the_line_names_both_ends_and_who_listens() {
        let style = Style::new(false);
        assert_eq!(
            added(
                "archbox",
                &listening("127.0.0.1", 5173, Some("ab12"), "node"),
                5173,
                style
            ),
            "▶ Forwarding archbox:5173 to localhost:5173 · node"
        );
        assert_eq!(
            added(
                "archbox",
                &listening("0.0.0.0", 3000, Some("ab12"), "node"),
                3001,
                style
            ),
            "▶ Forwarding archbox:3000 to localhost:3001 · node\n  Port 3000 is in use here"
        );
        assert_eq!(
            added(
                "archbox",
                &listening("0.0.0.0", 5432, None, "db"),
                5432,
                style
            ),
            "▶ Forwarding archbox:5432 to localhost:5432 · Docker db"
        );
        assert_eq!(
            not_forwarded("archbox", &listening("0.0.0.0", 3000, None, "db"), style),
            "! Could not forward archbox:3000 to this machine\n  Restart the server on the Agent to try again"
        );
    }
}
