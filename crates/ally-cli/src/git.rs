//! Sending the Client's git history to the Agent's copy after a push, so `git status`,
//! `git diff`, and `git log` work there. git itself copies the history, through the same
//! ssh connection rsync uses.

use crate::client::unexpected;
use crate::project::Local;
use crate::route;
use crate::transfer::{self, ProjectSession};
use ally_core::config::Agent;
use ally_core::control::{GitHead, HeadOutcome, PUSHED, Request, Response};
use anyhow::{Context, bail};
use std::path::Path;
use std::process::Stdio;

/// Push this machine's `HEAD` into the Agent's copy and move the copy's git to it. `None`
/// when the project is not a git repository or has no commit yet.
pub async fn send(
    opened: &mut ProjectSession,
    agent: &Agent,
    local: &Local,
) -> anyhow::Result<Option<HeadOutcome>> {
    let root = &local.root;
    if !root.join(".git").exists() {
        return Ok(None);
    }
    let Some(commit) = read(root, &["rev-parse", "--verify", "--quiet", "HEAD^{commit}"]).await?
    else {
        return Ok(None);
    };
    let head = GitHead {
        commit,
        branch: read(root, &["symbolic-ref", "--quiet", "--short", "HEAD"]).await?,
    };
    let name = read(root, &["config", "--get", "user.name"]).await?;
    let email = read(root, &["config", "--get", "user.email"]).await?;
    let Response::Repository { path } = opened
        .control
        .call(Request::Repository {
            project: opened.id.clone(),
        })
        .await?
    else {
        return Err(unexpected());
    };
    let pushing = push(agent, root, &path, &head.commit);
    transfer::with_heartbeat(&mut opened.control, pushing).await?;
    let request = Request::Head {
        project: opened.id.clone(),
        head,
        name,
        email,
    };
    match opened.control.call(request).await? {
        Response::Head(outcome) => Ok(Some(outcome)),
        _ => Err(unexpected()),
    }
}

/// Send `commit` and everything it needs that the copy lacks. Local hooks never run, so a
/// pre-push hook such as a test suite cannot make the Client do heavy work.
async fn push(agent: &Agent, root: &Path, path: &str, commit: &str) -> anyhow::Result<()> {
    let out = tokio::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "push",
            "--quiet",
            "--force",
            "--no-verify",
            "--recurse-submodules=no",
        ])
        .arg(format!("ally:{path}"))
        .arg(format!("{commit}:{PUSHED}"))
        .env("GIT_SSH_COMMAND", transfer::remote_shell()?)
        .env("GIT_SSH_VARIANT", "simple")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("ALLY_RSH_AGENT", &agent.name)
        .env(route::ROUTE_ENV, route::resolve(agent).token())
        .stdin(Stdio::null())
        .kill_on_drop(true)
        .output()
        .await
        .context("Could not start git push")?;
    if !out.status.success() {
        bail!(
            "git push to the Agent failed\n{}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}

/// Ask git a question in `root`. Exit code 1 is git's ordinary "none", such as a detached
/// `HEAD` or an unset setting, so it is `None`.
async fn read(root: &Path, args: &[&str]) -> anyhow::Result<Option<String>> {
    let out = tokio::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .await
        .context("Could not start git on this machine")?;
    match out.status.code() {
        Some(0) => Ok(Some(
            String::from_utf8_lossy(&out.stdout).trim().to_string(),
        )),
        Some(1) => Ok(None),
        _ => bail!(
            "git {} failed on this machine: {}",
            args[0],
            String::from_utf8_lossy(&out.stderr).trim()
        ),
    }
}
