//! The TCP ports that Ally's work listens on, for port forwarding. Sockets come from `ss`
//! on Linux and `lsof` on macOS, and count only when a job's process holds them. Ports
//! that Docker containers publish come from `docker ps`, since Docker's own processes
//! hold them outside any job.

use crate::jobs;
use ally_core::control::Listening;
use ally_core::preflight;
use ally_core::telemetry::is_installed;
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

/// A listening TCP socket and the process that holds it.
#[derive(Debug, PartialEq)]
struct Socket {
    address: String,
    port: u16,
    pid: u32,
    program: String,
}

/// Every port that an active job's processes or a Docker container listen on, one entry
/// per port. `ss` or `lsof` runs only while a job is active.
pub fn list(root: &Path) -> anyhow::Result<Vec<Listening>> {
    let mut owners = HashMap::new();
    for job in jobs::list(root, false)? {
        for pid in jobs::processes(root, &job).into_keys() {
            owners.insert(pid, job.id.clone());
        }
    }
    let mut found = Vec::new();
    if !owners.is_empty() {
        for socket in sockets()? {
            if let Some(job) = owners.get(&socket.pid) {
                found.push(Listening {
                    port: socket.port,
                    address: socket.address,
                    program: socket.program,
                    job: Some(job.clone()),
                });
            }
        }
    }
    found.extend(docker());
    Ok(merged(found))
}

fn sockets() -> anyhow::Result<Vec<Socket>> {
    match std::env::consts::OS {
        "macos" => Ok(from_lsof(&output(
            "lsof",
            &["-nP", "-iTCP", "-sTCP:LISTEN", "-F", "pcn"],
            "lsof",
        )?)),
        _ => Ok(from_ss(&output("ss", &["-Hltnp"], "iproute2")?)),
    }
}

/// What `program` prints. lsof exits 1 when nothing listens, so the exit code is ignored.
fn output(program: &str, args: &[&str], package: &str) -> anyhow::Result<String> {
    match Command::new(program).args(args).output() {
        Ok(out) => Ok(String::from_utf8_lossy(&out.stdout).into_owned()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => anyhow::bail!(
            "{program} is not installed on the Agent, so its ports cannot be found. Fix: {}",
            preflight::install_hint(package)
        ),
        Err(error) => Err(anyhow::Error::from(error).context(format!("Could not run {program}"))),
    }
}

/// Lines of `ss -Hltnp`, such as
/// `LISTEN 0 511 127.0.0.1:5173 0.0.0.0:* users:(("node",pid=7417,fd=23))`.
/// ss names processes only for this account, which are the only ones a job can own.
fn from_ss(text: &str) -> Vec<Socket> {
    let mut sockets = Vec::new();
    for line in text.lines() {
        let Some((address, port)) = line.split_whitespace().nth(3).and_then(split_address) else {
            continue;
        };
        let users = line.split_once("users:(").map_or("", |(_, rest)| rest);
        for entry in users.split("(\"").skip(1) {
            let Some((program, rest)) = entry.split_once("\",") else {
                continue;
            };
            let digits: String = rest
                .strip_prefix("pid=")
                .unwrap_or("")
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            if let Ok(pid) = digits.parse() {
                sockets.push(Socket {
                    address: address.clone(),
                    port,
                    pid,
                    program: program.to_string(),
                });
            }
        }
    }
    sockets
}

/// The field output of `lsof -F pcn`: a `p` line with the process ID, a `c` line with its
/// name, then an `n` line for each listening address.
fn from_lsof(text: &str) -> Vec<Socket> {
    let mut sockets = Vec::new();
    let mut pid = None;
    let mut program = String::new();
    for line in text.lines() {
        match line.split_at_checked(1) {
            Some(("p", value)) => pid = value.parse().ok(),
            Some(("c", value)) => program = value.to_string(),
            Some(("n", value)) => {
                if let (Some(pid), Some((address, port))) = (pid, split_address(value)) {
                    sockets.push(Socket {
                        address,
                        port,
                        pid,
                        program: program.clone(),
                    });
                }
            }
            _ => {}
        }
    }
    sockets
}

/// Ports that running containers publish. Docker that is missing or refuses this account
/// means no containers to report, never an error.
fn docker() -> Vec<Listening> {
    if !is_installed("docker") {
        return Vec::new();
    }
    let Ok(out) = Command::new("docker")
        .args(["ps", "--format", "{{.Names}}\t{{.Ports}}"])
        .output()
    else {
        return Vec::new();
    };
    from_docker(&String::from_utf8_lossy(&out.stdout))
}

/// Lines of `docker ps`, a name and its ports, such as
/// `db\t0.0.0.0:5432->5432/tcp, [::]:5432->5432/tcp`. A port without `->` is not
/// published, and UDP cannot be forwarded over ssh.
fn from_docker(text: &str) -> Vec<Listening> {
    let mut found = Vec::new();
    for line in text.lines() {
        let Some((name, ports)) = line.split_once('\t') else {
            continue;
        };
        for published in ports.split(", ") {
            let Some((host, inside)) = published.split_once("->") else {
                continue;
            };
            if !inside.ends_with("/tcp") {
                continue;
            }
            if let Some((address, port)) = split_address(host) {
                found.push(Listening {
                    port,
                    address,
                    program: name.to_string(),
                    job: None,
                });
            }
        }
    }
    found
}

/// An address and port such as `127.0.0.1:3000`, `[::1]:3000`, `*:3000`, or
/// `127.0.0.53%lo:53`, as the address alone and the port. `*` means every address.
fn split_address(text: &str) -> Option<(String, u16)> {
    let (host, port) = text.rsplit_once(':')?;
    let port = port.parse().ok()?;
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let host = host.split('%').next().unwrap_or(host);
    match host {
        "*" | "" => Some(("0.0.0.0".to_string(), port)),
        host => Some((host.to_string(), port)),
    }
}

/// One entry per port. A server often listens on both IPv4 and IPv6, and IPv4 is kept,
/// since it reaches a server that listens on every address either way.
fn merged(mut found: Vec<Listening>) -> Vec<Listening> {
    found.sort_by_key(|listening| (listening.port, listening.address.contains(':')));
    found.dedup_by_key(|listening| listening.port);
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn socket(address: &str, port: u16, pid: u32, program: &str) -> Socket {
        Socket {
            address: address.to_string(),
            port,
            pid,
            program: program.to_string(),
        }
    }

    fn listening(address: &str, port: u16, job: Option<&str>) -> Listening {
        Listening {
            port,
            address: address.to_string(),
            program: "node".to_string(),
            job: job.map(str::to_string),
        }
    }

    /// Copied from an Arch Linux Agent. Sockets of other accounts carry no process.
    /// Runs the real `ss` or `lsof`, which must name this test's own listening socket.
    #[test]
    fn this_machines_tool_finds_a_port_this_process_listens_on() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let found = sockets().unwrap();
        assert!(
            found
                .iter()
                .any(|s| s.port == port && s.pid == std::process::id() && s.address == "127.0.0.1"),
            "{found:?}"
        );
    }

    #[test]
    fn ss_lines_give_this_accounts_sockets_with_their_processes() {
        let text = "\
LISTEN 0      4096                 100.67.90.119:48074 0.0.0.0:*
LISTEN 0      5                        127.0.0.1:8765  0.0.0.0:* users:((\"python\",pid=7417,fd=3))
LISTEN 0      511                              *:5173        *:* users:((\"node\",pid=88,fd=20),(\"node\",pid=89,fd=20))
LISTEN 0      511                          [::1]:3000     [::]:* users:((\"next-server\",pid=12,fd=21))
LISTEN 0      128                           [::]:22       [::]:*
";
        assert_eq!(
            from_ss(text),
            [
                socket("127.0.0.1", 8765, 7417, "python"),
                socket("0.0.0.0", 5173, 88, "node"),
                socket("0.0.0.0", 5173, 89, "node"),
                socket("::1", 3000, 12, "next-server"),
            ]
        );
    }

    #[test]
    fn lsof_fields_give_each_process_with_its_addresses() {
        let text =
            "p7417\ncpython\nf3\nn127.0.0.1:8765\np90\ncnode\nf20\nn*:5173\nf21\nn[::1]:5173\n";
        assert_eq!(
            from_lsof(text),
            [
                socket("127.0.0.1", 8765, 7417, "python"),
                socket("0.0.0.0", 5173, 90, "node"),
                socket("::1", 5173, 90, "node"),
            ]
        );
    }

    #[test]
    fn docker_gives_only_published_tcp_ports() {
        let text = "db\t0.0.0.0:5432->5432/tcp, [::]:5432->5432/tcp\nweb\t127.0.0.1:8080->80/tcp, 443/tcp\ndns\t0.0.0.0:53->53/udp\nidle\t\n";
        let found = from_docker(text);
        let ports: Vec<_> = found
            .iter()
            .map(|l| {
                (
                    l.program.as_str(),
                    l.address.as_str(),
                    l.port,
                    l.job.is_none(),
                )
            })
            .collect();
        assert_eq!(
            ports,
            [
                ("db", "0.0.0.0", 5432, true),
                ("db", "::", 5432, true),
                ("web", "127.0.0.1", 8080, true),
            ]
        );
    }

    #[test]
    fn addresses_lose_brackets_and_interfaces_and_a_star_means_every_address() {
        for (text, address, port) in [
            ("127.0.0.1:3000", "127.0.0.1", 3000),
            ("[::1]:3000", "::1", 3000),
            ("*:3000", "0.0.0.0", 3000),
            ("127.0.0.53%lo:53", "127.0.0.53", 53),
            ("[fe80::1%eth0]:8080", "fe80::1", 8080),
            (":::5432", "::", 5432),
        ] {
            assert_eq!(
                split_address(text),
                Some((address.to_string(), port)),
                "{text}"
            );
        }
        assert_eq!(split_address("127.0.0.1:8000-8001"), None);
        assert_eq!(split_address("nothing"), None);
    }

    #[test]
    fn each_port_appears_once_and_ipv4_wins() {
        let found = merged(vec![
            listening("::", 5432, None),
            listening("0.0.0.0", 5432, None),
            listening("::1", 3000, Some("ab12")),
            listening("127.0.0.1", 8765, Some("ab12")),
            listening("127.0.0.1", 8765, Some("ab12")),
        ]);
        assert_eq!(
            found,
            [
                listening("::1", 3000, Some("ab12")),
                listening("0.0.0.0", 5432, None),
                listening("127.0.0.1", 8765, Some("ab12")),
            ]
        );
    }
}
