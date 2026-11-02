//! Git in a project's copy, so a coding agent there can see history and commit. The
//! Client's history arrives by `git push` into a hidden ref, and the copy follows it only
//! while no commit made on the Agent would be left behind.

use crate::projects::{self, Blocking, Paths};
use ally_core::control::{GitHead, HeadOutcome, PUSHED};
use ally_core::{storage, telemetry};
use anyhow::{Context, bail, ensure};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Make the copy a repository if it is not one yet, and return its path for `git push`.
pub fn prepare(root: &Path, id: &str) -> anyhow::Result<String> {
    let (paths, _) = projects::load(root, id)?;
    ensure!(
        telemetry::is_installed("git"),
        "Git is not installed on the Agent. Fix: ally tools"
    );
    if !paths.source.join(".git").exists() {
        git(&paths.source, &["init", "--quiet"])?;
        git(&paths.source, &["config", "receive.shallowUpdate", "true"])?;
    }
    paths
        .source
        .to_str()
        .map(str::to_string)
        .context("The project copy's path is not UTF 8")
}

/// Move the copy's branch and index to the pushed commit, leaving its files alone, since
/// the sync already made them match the Client. Nothing moves when a branch or `HEAD`
/// holds commits made on the Agent, so no commit is ever lost.
pub fn point(
    root: &Path,
    id: &str,
    head: &GitHead,
    name: Option<&str>,
    email: Option<&str>,
) -> anyhow::Result<HeadOutcome> {
    let (paths, metadata) = projects::load(root, id)?;
    let _lock = projects::acquire(root, &paths, &metadata, Blocking::Runs)?;
    let dir = &paths.source;
    ensure!(
        is_commit(&head.commit),
        "The Client sent an unusable commit"
    );
    if let Some(branch) = &head.branch {
        optional(dir, &["check-ref-format", &format!("refs/heads/{branch}")])?
            .context("The Client sent an unusable branch name")?;
    }
    let pushed = optional(dir, &["rev-parse", "--verify", "--quiet", PUSHED])?;
    ensure!(
        pushed.as_deref() == Some(head.commit.as_str()),
        "The pushed history did not arrive on the Agent. Run the sync again"
    );
    for (key, value) in [("user.name", name), ("user.email", email)] {
        if let Some(value) = value {
            git(dir, &["config", key, value])?;
        }
    }

    let last: Option<GitHead> = storage::read_json(&last_file(&paths))?;
    let current = position(dir)?;
    let mut checks = Vec::new();
    if let Some(current) = &current {
        let label = current.branch.clone().unwrap_or_else(|| "HEAD".to_string());
        checks.push((label, current.commit.clone()));
    }
    if let Some(branch) = &head.branch
        && let Some(commit) = optional(
            dir,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("refs/heads/{branch}"),
            ],
        )?
    {
        checks.push((branch.clone(), commit));
    }
    for (reference, commit) in checks {
        let contained =
            optional(dir, &["merge-base", "--is-ancestor", &commit, &head.commit])?.is_some();
        if !may_move(&commit, last.as_ref().map(|l| l.commit.as_str()), contained) {
            let commits = count(dir, &head.commit, &commit)?;
            return Ok(HeadOutcome::Kept { reference, commits });
        }
    }

    let commits = match &current {
        Some(current) => Some(count(dir, &current.commit, &head.commit)?),
        None => None,
    };
    match &head.branch {
        Some(branch) => git(
            dir,
            &["symbolic-ref", "HEAD", &format!("refs/heads/{branch}")],
        )?,
        None => git(dir, &["update-ref", "--no-deref", "HEAD", &head.commit])?,
    };
    if current.as_ref().map(|c| c.commit.as_str()) != Some(head.commit.as_str()) {
        git(dir, &["reset", "--quiet", &head.commit])?;
    }
    storage::write_json(&last_file(&paths), head)?;
    Ok(HeadOutcome::Moved { commits })
}

/// A reference may follow the Client when it is where Ally last put it, or when the new
/// commit already contains it. Anything else means someone committed on the Agent.
fn may_move(reference: &str, last: Option<&str>, contained: bool) -> bool {
    contained || last == Some(reference)
}

/// A full SHA1 or SHA256 commit name, so it can never be read as an option or a range.
fn is_commit(name: &str) -> bool {
    matches!(name.len(), 40 | 64) && name.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The commit and branch of the copy's `HEAD`, or `None` before its first commit.
fn position(dir: &Path) -> anyhow::Result<Option<GitHead>> {
    let Some(commit) = optional(dir, &["rev-parse", "--verify", "--quiet", "HEAD^{commit}"])?
    else {
        return Ok(None);
    };
    let branch = optional(dir, &["symbolic-ref", "--quiet", "--short", "HEAD"])?;
    Ok(Some(GitHead { commit, branch }))
}

/// How many commits `to` has that `from` does not.
fn count(dir: &Path, from: &str, to: &str) -> anyhow::Result<usize> {
    git(dir, &["rev-list", "--count", &format!("{from}..{to}")])?
        .parse()
        .context("git rev-list gave an unreadable count")
}

/// The commit and branch Ally last put the copy at.
fn last_file(paths: &Paths) -> PathBuf {
    paths.state.join("git.json")
}

fn git(dir: &Path, args: &[&str]) -> anyhow::Result<String> {
    optional(dir, args)?.with_context(|| {
        format!(
            "git {} failed in the Agent's copy. Try the sync again",
            args[0]
        )
    })
}

/// Run git in `dir`. Exit code 1 is git's ordinary "no" for queries such as
/// `merge-base --is-ancestor`, so it is `None`, and any other failure is an error.
fn optional(dir: &Path, args: &[&str]) -> anyhow::Result<Option<String>> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .context("Could not start git on the Agent. Fix: ally tools")?;
    match out.status.code() {
        Some(0) => Ok(Some(
            String::from_utf8_lossy(&out.stdout).trim().to_string(),
        )),
        Some(1) => Ok(None),
        _ => bail!(
            "git {} failed in the Agent's copy: {}. Try the sync again",
            args[0],
            String::from_utf8_lossy(&out.stderr).trim()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reference_moves_only_when_no_agent_commit_would_be_left_behind() {
        assert!(may_move("aaa", Some("aaa"), false));
        assert!(may_move("aaa", Some("bbb"), true));
        assert!(may_move("aaa", None, true));
        assert!(!may_move("aaa", Some("bbb"), false));
        assert!(!may_move("aaa", None, false));
    }

    #[test]
    fn only_full_commit_names_are_accepted() {
        assert!(is_commit(&"a".repeat(40)));
        assert!(is_commit(&"0".repeat(64)));
        assert!(!is_commit("HEAD"));
        assert!(!is_commit(&format!("-{}", "a".repeat(39))));
        assert!(!is_commit(&"a".repeat(41)));
    }
}
