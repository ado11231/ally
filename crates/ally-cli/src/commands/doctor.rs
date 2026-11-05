//! `ally doctor`: checks this machine's setup as an Agent, a Client, or both, and prints
//! the fix for each problem, so a stranger does not have to search the troubleshooting page.

use crate::{client, route};
use ally_core::config::{Agent, Config};
use ally_core::control::{Request, Response};
use ally_core::preflight::{self, Check};
use ally_core::presentation;
use std::time::Duration;

/// The paths a Client can take, in the order Ally tries them.
const PATHS: [&str; 3] = ["local", "tailnet", "iroh"];

/// A path that has not answered by now counts as down, so one dead path cannot hold up
/// the whole check.
const PATH_LIMIT: Duration = Duration::from_secs(30);

pub async fn doctor(requested: Option<String>) -> anyhow::Result<i32> {
    let config = Config::load_or_empty()?;
    let root = ally_agent::service::root()?;
    let is_agent = root.exists();
    if !is_agent && config.names().is_empty() {
        anyhow::bail!(
            "This machine is not set up yet. Run ally start on the Agent, then ally link <code> on the Client"
        );
    }
    let mut failed = false;
    if is_agent {
        presentation::section("This machine as an Agent");
        failed |= preflight::report(&agent_checks(&root)?);
    }
    if !config.names().is_empty() {
        let agent = config.resolve(requested.as_deref())?;
        presentation::section(&format!("This machine as a Client of {}", agent.name));
        failed |= preflight::report(&client_checks(agent).await);
    }
    Ok(i32::from(failed))
}

fn agent_checks(root: &std::path::Path) -> anyhow::Result<Vec<Check>> {
    let mut checks = preflight::start_checks();
    checks.push(match ally_agent::service::running(root)? {
        true => Check::pass("Ally is running"),
        false => Check::fail("Ally is not running", "Run ally start"),
    });
    checks.push(match ally_agent::boot::installed()? {
        true => Check::pass("Ally starts by itself"),
        false => Check::warn(
            "Ally does not start by itself after a restart",
            "Run ally start --boot",
        ),
    });
    Ok(checks)
}

async fn client_checks(agent: &Agent) -> Vec<Check> {
    let mut checks = vec![
        preflight::tool_check("ssh", None),
        preflight::tool_check("rsync", None),
    ];
    let saved: Vec<&str> = PATHS
        .into_iter()
        .filter(|via| route::saved(agent, via))
        .collect();
    for via in &saved {
        checks.push(path_check(agent, via).await);
    }
    if saved.is_empty() {
        checks.push(Check::fail(
            format!("No way to reach {} is saved", agent.name),
            "Run ally link again",
        ));
    }
    checks
}

/// Reach the Agent over `via` alone and ask it to answer, which also proves the key,
/// the Agent's ally program, and that both machines speak the same version.
async fn path_check(agent: &Agent, via: &str) -> Check {
    let label = route::label(via);
    let failed =
        |fix: String| Check::fail(format!("{} did not answer via {label}", agent.name), fix);
    if route::force(Some(&agent.name), via).is_err() {
        return failed(route::hint(via).to_string());
    }
    match tokio::time::timeout(PATH_LIMIT, client::request(agent, Request::Ping)).await {
        Ok(Ok(Response::Pong)) => Check::pass(format!("{} answers via {label}", agent.name)),
        Ok(Ok(_)) => failed("Update Ally on both machines to the same version".to_string()),
        Ok(Err(error)) => failed(format!("{error:#}")),
        Err(_) => failed(route::hint(via).to_string()),
    }
}
