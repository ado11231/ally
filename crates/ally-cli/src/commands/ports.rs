//! `ally ports`: every port listening on the Agent, who opened it, and where it opens on
//! this machine.

use crate::client::{self, Control, unexpected};
use crate::forward;
use crate::ssh::RemoteCommand;
use ally_core::config::Config;
use ally_core::control::{Job, Listening, Request, Response, Source};
use ally_core::presentation::{Style, Tone};
use ally_core::storage;
use std::collections::HashMap;

pub async fn ports(agent: Option<String>) -> anyhow::Result<i32> {
    let config = Config::load()?;
    let target = config.resolve(agent.as_deref())?;
    let connecting = client::connecting(target);
    let mut control = Control::connect(target).await?;
    let ports = control.call(Request::Ports).await?;
    let jobs = control.call(Request::Jobs { all: false }).await?;
    control.close().await;
    let (Response::Ports(ports), Response::Jobs(jobs)) = (ports, jobs) else {
        return Err(unexpected());
    };
    client::connected(connecting, target);
    let here: HashMap<u16, u16> = match RemoteCommand::to(target, String::new(), Vec::new()).shared
    {
        Some(socket) => ports
            .iter()
            .filter_map(|listening| {
                forward::forwarded_at(&socket, listening.port).map(|local| (listening.port, local))
            })
            .collect(),
        None => HashMap::new(),
    };
    print!(
        "\n{}",
        render(&target.name, &ports, &jobs, &here, Style::stdout())
    );
    Ok(0)
}

/// The table. An open forward is green, like a running job in `ally ps`, and anything not
/// worth a look is dimmed: a port not forwarded, and a row from another account. The hint
/// to forward one shows only for a port worth opening.
fn render(
    agent: &str,
    ports: &[Listening],
    jobs: &[Job],
    here: &HashMap<u16, u16>,
    style: Style,
) -> String {
    let mut output = format!("{}\n\n", style.heading(format!("Ports on {agent}")));
    if ports.is_empty() {
        output.push_str(&format!(
            "  Nothing listening. Servers started with {} or {} show here\n",
            style.paint("ally run", Tone::Info),
            style.paint("ally attach", Tone::Info)
        ));
        return output;
    }
    let header = format!(
        "{:<6} {:<12} {:<14} {:<16} {}",
        "PORT", "PROGRAM", "FROM", "PROJECT", "HERE"
    );
    output.push_str(&format!("  {}\n", style.dim(header)));
    let projects: HashMap<&str, &str> = jobs
        .iter()
        .map(|job| {
            (
                job.id.as_str(),
                job.project_name.as_deref().unwrap_or("home"),
            )
        })
        .collect();
    for listening in ports {
        let program = match listening.program.is_empty() {
            true => "-".to_string(),
            false => listening.program.chars().take(12).collect(),
        };
        let (from, project) = match &listening.from {
            Source::Job(id) => (
                storage::short_id(id).to_string(),
                projects.get(id.as_str()).copied().unwrap_or(""),
            ),
            Source::Docker => ("Docker".to_string(), ""),
            Source::Outside => ("outside Ally".to_string(), ""),
            Source::OtherAccount => ("other account".to_string(), ""),
        };
        let project: String = project.chars().take(16).collect();
        let row = format!("{:<6} {program:<12} ", listening.port);
        let from = format!("{from:<14}");
        let project = format!(" {project:<16} ");
        let forwarded = here.get(&listening.port);
        let place = match forwarded {
            Some(local) => format!("localhost:{local}"),
            None => "not forwarded".to_string(),
        };
        let line = match &listening.from {
            Source::OtherAccount => style.dim(format!("{row}{from}{project}{place}")),
            source => {
                let from = match source {
                    Source::Docker => style.paint(from, Tone::Info),
                    _ => from,
                };
                let place = match forwarded {
                    Some(_) => style.paint(place, Tone::Good),
                    None => style.dim(place),
                };
                format!("{row}{from}{project}{place}")
            }
        };
        output.push_str(&format!("  {}\n", line.trim_end()));
    }
    if ports.iter().any(|listening| {
        listening.from != Source::OtherAccount && !here.contains_key(&listening.port)
    }) {
        output.push_str(&format!(
            "\n  Forward one with {}\n",
            style.paint("ally forward <port>", Tone::Info)
        ));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use ally_core::control::{JobKind, JobState};

    fn listening(port: u16, program: &str, from: Source) -> Listening {
        Listening {
            port,
            address: "127.0.0.1".to_string(),
            program: program.to_string(),
            from,
        }
    }

    fn job(id: &str, project: Option<&str>) -> Job {
        Job {
            id: id.to_string(),
            kind: JobKind::Session,
            project: project.map(str::to_string),
            project_name: project.map(str::to_string),
            command: "Shell session".to_string(),
            state: JobState::Running,
            started: 0,
            ended: None,
            exit_code: None,
            pid: None,
            process_start: None,
            boot: 0,
            stop_requested: false,
        }
    }

    #[test]
    fn each_port_says_who_opened_it_and_where_it_opens_here() {
        let ports = [
            listening(22, "", Source::OtherAccount),
            listening(5173, "node", Source::Job("3f2a1b9c0000".to_string())),
            listening(5432, "db", Source::Docker),
            listening(6379, "redis", Source::Outside),
            listening(8765, "python", Source::Job("a219089f0000".to_string())),
        ];
        let jobs = [job("3f2a1b9c0000", Some("app")), job("a219089f0000", None)];
        let here = HashMap::from([(5173, 5173), (5432, 5433)]);
        let output = render("archbox", &ports, &jobs, &here, Style::new(false));
        assert_eq!(
            output,
            "\
Ports on archbox

  PORT   PROGRAM      FROM           PROJECT          HERE
  22     -            other account                   not forwarded
  5173   node         3f2a1b9c       app              localhost:5173
  5432   db           Docker                          localhost:5433
  6379   redis        outside Ally                    not forwarded
  8765   python       a219089f       home             not forwarded

  Forward one with ally forward <port>
"
        );
    }

    #[test]
    fn color_marks_open_forwards_docker_and_dims_the_rest() {
        let ports = [
            listening(22, "", Source::OtherAccount),
            listening(5432, "db", Source::Docker),
            listening(6379, "redis", Source::Outside),
        ];
        let here = HashMap::from([(5432, 5432)]);
        let output = render("archbox", &ports, &[], &here, Style::new(true));
        assert!(output.contains("\x1b[32mlocalhost:5432\x1b[0m"), "{output}");
        assert!(output.contains("\x1b[36mDocker        \x1b[0m"), "{output}");
        assert!(output.contains("\x1b[2mnot forwarded\x1b[0m"), "{output}");
        assert!(output.contains("  \x1b[2m22     -"), "{output}");
    }

    #[test]
    fn nothing_listening_says_where_servers_come_from_and_nothing_left_to_open_needs_no_hint() {
        let empty = render("archbox", &[], &[], &HashMap::new(), Style::new(false));
        assert_eq!(
            empty,
            "Ports on archbox\n\n  Nothing listening. Servers started with ally run or ally attach show here\n"
        );
        let ports = [
            listening(22, "", Source::OtherAccount),
            listening(5432, "db", Source::Docker),
        ];
        let all = render(
            "archbox",
            &ports,
            &[],
            &HashMap::from([(5432, 5432)]),
            Style::new(false),
        );
        assert!(!all.contains("Forward one"), "{all}");
    }
}
