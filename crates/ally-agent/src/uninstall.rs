//! What `ally uninstall` removes when this machine is an Agent: the boot service, its
//! running work, the access it gave Clients, the `~/Ally` links, and its data.

use crate::projects::size;
use crate::{boot, jobs, service};
use ally_core::control::{Job, JobKind};
use ally_core::keys;
use ally_core::presentation;
use anyhow::Context;
use std::fs;
use std::path::{Path, PathBuf};

/// Everything Ally keeps on this machine as an Agent.
pub struct Found {
    pub service: Option<PathBuf>,
    pub active: Vec<Job>,
    /// The Agent's data folder, how many project copies it holds, and its size in bytes.
    pub data: Option<(PathBuf, usize, u64)>,
    pub links: Option<PathBuf>,
    pub clients: usize,
}

impl Found {
    /// Looks without creating anything: the running check takes a lock file, so it only runs
    /// when the Agent's folder already exists.
    pub fn look() -> anyhow::Result<Found> {
        let root = service::root()?;
        let service = Some(boot::file()?).filter(|file| file.exists());
        anyhow::ensure!(
            service.is_some() || !root.exists() || !service::running(&root)?,
            "Ally is running in a terminal on this machine. Stop it with Ctrl C there, then run ally uninstall again"
        );
        let data = root.exists().then(|| {
            let projects = fs::read_dir(root.join("projects"))
                .map(|items| items.count())
                .unwrap_or(0);
            (root.clone(), projects, size(&root))
        });
        let links = directories::BaseDirs::new()
            .map(|dirs| dirs.home_dir().join("Ally"))
            .filter(|folder| folder.exists());
        Ok(Found {
            service,
            active: match root.exists() {
                true => jobs::list(&root, false)?,
                false => Vec::new(),
            },
            data,
            links,
            clients: keys::authorized_clients()?,
        })
    }

    pub fn is_empty(&self) -> bool {
        self.service.is_none()
            && self.active.is_empty()
            && self.data.is_none()
            && self.links.is_none()
            && self.clients == 0
    }

    /// One `label   value` pair per kind, for the list shown before asking.
    pub fn listing(&self) -> Vec<(&'static str, String)> {
        let mut lines = Vec::new();
        if let Some(file) = &self.service {
            let rule = match &self.data {
                Some((root, _, _)) if crate::awake::rule_written(root) => {
                    format!(" and {}, with sudo", crate::awake::SLEEP_RULE)
                }
                _ => String::new(),
            };
            lines.push((
                "Service",
                format!(
                    "stops it, and removes {}{rule}",
                    presentation::home_path(file)
                ),
            ));
        }
        if !self.active.is_empty() {
            lines.push(("Running", format!("ends {}", running(&self.active))));
        }
        if let Some((folder, projects, bytes)) = &self.data {
            lines.push((
                "Projects",
                format!(
                    "{} in {} ({}), including any edits not brought back to their Client",
                    copies(*projects),
                    presentation::home_path(folder),
                    presentation::capacity(mebibytes(*bytes))
                ),
            ));
        }
        if let Some(folder) = &self.links {
            lines.push(("Links", presentation::home_path(folder)));
        }
        if self.clients > 0 {
            lines.push(("Access", access(self.clients)));
        }
        lines
    }

    /// Remove everything listed, service first so nothing recreates files meanwhile.
    pub fn remove(&self) -> anyhow::Result<()> {
        if self.service.is_some() {
            boot::remove_service()?;
            presentation::success("Stopped Ally and its boot service");
        }
        if let Some((root, _, _)) = &self.data {
            for job in &self.active {
                jobs::stop(root, &job.id)?;
            }
            if !self.active.is_empty() {
                presentation::success(format!("Ended {}", running(&self.active)));
            }
        }
        if self.clients > 0 {
            let removed = keys::revoke_clients()?;
            presentation::success(format!(
                "Removed access for {}",
                presentation::plural(removed, "linked machine")
            ));
        }
        if let Some(folder) = &self.links {
            remove_links(folder)?;
        }
        if let Some((root, _, _)) = &self.data {
            fs::remove_dir_all(root).with_context(|| {
                format!(
                    "Could not remove {}. Delete it by hand",
                    presentation::home_path(root)
                )
            })?;
            presentation::success(format!("Removed {}", presentation::home_path(root)));
        }
        Ok(())
    }
}

fn access(clients: usize) -> String {
    match clients {
        1 => "1 linked machine loses access (its line in ~/.ssh/authorized_keys)".to_string(),
        _ => {
            format!("{clients} linked machines lose access (their lines in ~/.ssh/authorized_keys)")
        }
    }
}

/// Bytes to the nearest MiB, so a project of 2.9 MiB does not read as 2.0.
fn mebibytes(bytes: u64) -> u64 {
    (bytes + 512 * 1024) / (1024 * 1024)
}

fn copies(count: usize) -> String {
    match count {
        1 => "1 project copy".to_string(),
        _ => format!("{count} project copies"),
    }
}

/// `2 sessions and 1 run`, or just the one kind there is.
fn running(active: &[Job]) -> String {
    let sessions = active
        .iter()
        .filter(|job| job.kind == JobKind::Session)
        .count();
    let runs = active.len() - sessions;
    match (sessions, runs) {
        (_, 0) => presentation::plural(sessions, "session"),
        (0, _) => presentation::plural(runs, "run"),
        _ => format!(
            "{} and {}",
            presentation::plural(sessions, "session"),
            presentation::plural(runs, "run")
        ),
    }
}

/// Remove Ally's own links, then the folder if nothing else is in it. A file the owner
/// put there is kept and named.
fn remove_links(folder: &Path) -> anyhow::Result<()> {
    for item in fs::read_dir(folder)?.flatten() {
        if item.file_type().is_ok_and(|kind| kind.is_symlink()) {
            fs::remove_file(item.path())?;
        }
    }
    match fs::remove_dir(folder) {
        Ok(()) => presentation::success(format!("Removed {}", presentation::home_path(folder))),
        Err(_) => presentation::warning(format!(
            "Kept {}, since it holds files of your own",
            presentation::home_path(folder)
        )),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Root;

    fn job(kind: JobKind) -> Job {
        Job {
            id: "j".into(),
            kind,
            project: None,
            project_name: None,
            command: String::new(),
            state: ally_core::control::JobState::Running,
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
    fn counts_and_sizes_read_in_plain_english() {
        assert_eq!(copies(1), "1 project copy");
        assert_eq!(copies(3), "3 project copies");
        assert!(access(1).starts_with("1 linked machine loses access (its line"));
        assert!(access(2).starts_with("2 linked machines lose access (their lines"));
        assert_eq!(mebibytes(3_000_000), 3);
        assert_eq!(mebibytes(1024 * 1024), 1);
    }

    #[test]
    fn running_work_is_named_by_kind() {
        assert_eq!(running(&[job(JobKind::Session)]), "1 session");
        assert_eq!(running(&[job(JobKind::Run), job(JobKind::Run)]), "2 runs");
        assert_eq!(
            running(&[job(JobKind::Session), job(JobKind::Run)]),
            "1 session and 1 run"
        );
    }

    #[test]
    fn only_ally_links_are_removed_from_the_folder() {
        let home = Root::new();
        let folder = home.0.join("Ally");
        fs::create_dir(&folder).unwrap();
        std::os::unix::fs::symlink(&home.0, folder.join("app")).unwrap();
        remove_links(&folder).unwrap();
        assert!(!folder.exists());

        fs::create_dir(&folder).unwrap();
        std::os::unix::fs::symlink(&home.0, folder.join("app")).unwrap();
        fs::write(folder.join("notes.txt"), "mine").unwrap();
        remove_links(&folder).unwrap();
        assert!(!folder.join("app").exists());
        assert_eq!(
            fs::read_to_string(folder.join("notes.txt")).unwrap(),
            "mine"
        );
    }
}
