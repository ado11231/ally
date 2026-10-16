//! Keeps the Agent from sleeping on its own while `slingshot start` runs, because a sleeping
//! Agent cannot be reached from anywhere. Wraps the system's own tool, and ties its lock to
//! this process so the lock ends with `start`, even when `start` is killed outright.

use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::time::Duration;

/// Holds the lock until dropped.
pub struct Awake {
    child: Child,
    _lifeline: Option<ChildStdin>,
}

impl Drop for Awake {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Whether sleep is turned off in systemd, by masking either target an idle suspend goes
/// through. Then the machine cannot sleep, and there is nothing to hold or warn about.
pub fn sleep_off() -> bool {
    ["sleep.target", "suspend.target"].iter().any(|target| {
        Command::new("systemctl")
            .args(["is-enabled", target])
            .output()
            .is_ok_and(|output| masked(&output.stdout))
    })
}

/// `systemctl is-enabled` says `masked`, or `masked-runtime` until the next boot.
fn masked(stdout: &[u8]) -> bool {
    String::from_utf8_lossy(stdout).trim().starts_with("masked")
}

/// The polkit rule that lets this account hold the sleep lock with nobody logged in, as a
/// boot service must. Without it, polkit asks for an admin password outside a session at
/// the machine, so the lock is refused and an idle login screen can suspend the Agent.
pub const SLEEP_RULE: &str = "/etc/polkit-1/rules.d/50-slingshot.rules";

/// Saved in the Agent's folder once the rule is written, since the rules folder is
/// readable only by root.
const SLEEP_RULE_MARK: &str = "sleep-rule";

pub fn rule_written(root: &Path) -> bool {
    root.join(SLEEP_RULE_MARK).exists()
}

pub fn mark_rule(root: &Path, written: bool) -> anyhow::Result<()> {
    let mark = root.join(SLEEP_RULE_MARK);
    match written {
        true => ally_core::storage::write_bytes(&mark, b""),
        false => match std::fs::remove_file(&mark) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        },
    }
}

/// The rule's text for `user`, or `None` for a name that is not a plain account name, since
/// it is written into the rule as a string.
pub fn sleep_rule(user: &str) -> Option<String> {
    let plain = !user.is_empty()
        && user
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_.-".contains(c));
    plain.then(|| {
        format!(
            "// Written by Slingshot: lets {user} keep this machine awake while Slingshot runs,\n\
             // even with nobody logged in. slingshot start --remove deletes it.\n\
             polkit.addRule(function(action, subject) {{\n\
             \x20   if ((action.id == \"org.freedesktop.login1.inhibit-block-sleep\" ||\n\
             \x20        action.id == \"org.freedesktop.login1.inhibit-block-idle\") &&\n\
             \x20       subject.user == \"{user}\") {{\n\
             \x20       return polkit.Result.YES;\n\
             \x20   }}\n\
             }});\n"
        )
    })
}

/// A tool that keeps the system awake. `lifeline` means it runs a command that must read
/// from this process to stay alive, which is how it learns that this process has ended.
struct Tool {
    command: Command,
    lifeline: bool,
}

/// Take the lock with whichever tool this system has. `None` means nothing could hold it,
/// and the Agent may sleep.
pub fn hold() -> Option<Awake> {
    [systemd_inhibit(), caffeinate()]
        .into_iter()
        .find_map(start)
}

/// logind's inhibitor lives as long as the command it runs, here a `cat` reading from us.
/// It never asks for a password: polkit wants one outside a session at the machine, such
/// as over ssh, and a prompt there is a surprise nobody asked for.
fn systemd_inhibit() -> Tool {
    let mut command = Command::new("systemd-inhibit");
    command.args([
        "--no-ask-password",
        "--what=idle:sleep",
        "--who=Slingshot",
        "--why=Reachable by paired Clients",
        "--mode=block",
        "cat",
    ]);
    Tool {
        command,
        lifeline: true,
    }
}

/// caffeinate watches this process and exits with it.
fn caffeinate() -> Tool {
    let mut command = Command::new("caffeinate");
    command.args(["-i", "-w", &std::process::id().to_string()]);
    Tool {
        command,
        lifeline: false,
    }
}

/// A tool that is missing, or was refused the lock, exits at once. One still running after
/// a moment holds it.
fn start(mut tool: Tool) -> Option<Awake> {
    let stdin = match tool.lifeline {
        true => Stdio::piped(),
        false => Stdio::null(),
    };
    let mut child = tool
        .command
        .stdin(stdin)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    std::thread::sleep(Duration::from_millis(300));
    if !matches!(child.try_wait(), Ok(None)) {
        return None;
    }
    let lifeline = child.stdin.take();
    Some(Awake {
        child,
        _lifeline: lifeline,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sleep_rule_allows_only_this_account_both_locks() {
        let rule = sleep_rule("ado").unwrap();
        assert!(rule.contains("subject.user == \"ado\""), "{rule}");
        assert!(rule.contains("org.freedesktop.login1.inhibit-block-sleep"));
        assert!(rule.contains("org.freedesktop.login1.inhibit-block-idle"));
        assert!(rule.contains("    if ((action.id"), "{rule}");
        assert_eq!(rule.matches('{').count(), rule.matches('}').count());
        assert!(sleep_rule("a\"; evil").is_none());
        assert!(sleep_rule("").is_none());
    }

    #[test]
    fn the_rule_mark_is_saved_and_cleared() {
        let root = crate::testing::Root::new();
        assert!(!rule_written(&root.0));
        mark_rule(&root.0, true).unwrap();
        assert!(rule_written(&root.0));
        mark_rule(&root.0, false).unwrap();
        mark_rule(&root.0, false).unwrap();
        assert!(!rule_written(&root.0));
    }

    #[test]
    fn only_a_masked_target_means_sleep_is_off() {
        assert!(masked(b"masked\n"));
        assert!(masked(b"masked-runtime\n"));
        assert!(!masked(b"static\n"));
        assert!(!masked(b"enabled\n"));
        assert!(!masked(b""));
    }
}
