//! `ally forward <port>`: reach one Agent port at `localhost` on this machine until Ctrl C,
//! for a server left running after leaving its session, or one Ally did not start.

use crate::forward;
use crate::route;
use crate::ssh::{Disconnected, RemoteCommand};
use ally_core::config::Config;
use ally_core::presentation::{self, Style, Tone};
use anyhow::Context;

pub async fn forward(agent: Option<String>, port: u16) -> anyhow::Result<i32> {
    let config = Config::load()?;
    let target = config.resolve(agent.as_deref())?;
    let mut remote = RemoteCommand::to(target, String::new(), Vec::new());
    remote.tty = false;
    let socket = remote.shared.take();
    let mut claim = match &socket {
        Some(socket) => match forward::claim(socket, port) {
            Some(claim) => Some(claim),
            None => anyhow::bail!(already(
                &target.name,
                port,
                forward::forwarded_at(socket, port)
            )),
        },
        None => None,
    };
    let local = tokio::task::spawn_blocking(move || forward::free_port(port))
        .await
        .ok()
        .flatten()
        .context("Could not find a free port on this machine. Close a program using ports here, then run this again")?;
    if let Some(claim) = claim.as_mut() {
        forward::record(claim, local);
    }

    let style = Style::stderr();
    eprintln!(
        "{}",
        announcement(
            &target.name,
            port,
            local,
            route::resolve(target).name(),
            style
        )
    );
    let held = remote
        .hold(&forward::spec_to(local, "localhost", port))
        .await;
    let again = format!("Run ally forward {port} again");
    match held {
        Ok(None) => {
            presentation::success(format!("Stopped forwarding localhost:{local}"));
            Ok(0)
        }
        Ok(Some(_)) => anyhow::bail!("The forward to {}:{port} ended. {again}", target.name),
        Err(error) if error.downcast_ref::<Disconnected>().is_some() => {
            anyhow::bail!("Lost connection to {}. {again}", target.name)
        }
        Err(error) => Err(error),
    }
}

/// The opening lines, colored like every `▶` line, with a note when the port was taken here.
fn announcement(agent: &str, port: u16, local: u16, path: &str, style: Style) -> String {
    let mut text = format!(
        "Forwarding {} to {} via {}",
        style.paint(format!("{agent}:{port}"), Tone::Info),
        style.paint(format!("localhost:{local}"), Tone::Info),
        style.paint(path, Tone::Info)
    );
    if local != port {
        text.push_str(&format!("\n  Port {port} is in use here"));
    }
    text.push_str("\n  Ctrl C to stop");
    style.status(text, Tone::Info)
}

/// Said when another Ally command already forwards the port, naming where when it can.
fn already(agent: &str, port: u16, local: Option<u16>) -> String {
    let place = local
        .map(|local| format!(" at localhost:{local}"))
        .unwrap_or_default();
    format!(
        "{agent}:{port} is already forwarded{place} by another Ally command. Open it there, or end that command and run this again"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_opening_lines_name_both_ends_the_route_and_how_to_stop() {
        let style = Style::new(false);
        assert_eq!(
            announcement("archbox", 8080, 8080, "local network", style),
            "▶ Forwarding archbox:8080 to localhost:8080 via local network\n  Ctrl C to stop"
        );
        assert_eq!(
            announcement("archbox", 8080, 8081, "tailnet", style),
            "▶ Forwarding archbox:8080 to localhost:8081 via tailnet\n  Port 8080 is in use here\n  Ctrl C to stop"
        );
    }

    #[test]
    fn a_port_forwarded_elsewhere_says_where_when_known() {
        assert_eq!(
            already("archbox", 8080, Some(8081)),
            "archbox:8080 is already forwarded at localhost:8081 by another Ally command. Open it there, or end that command and run this again"
        );
        assert!(already("archbox", 8080, None).starts_with("archbox:8080 is already forwarded by"));
    }
}
