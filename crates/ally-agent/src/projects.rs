//! Project storage on the Agent: source copies, sync leases, build output, and the
//! environment files older builds stored. Each project ID gets separate folders for each
//! kind of data.

use crate::jobs;
use ally_core::artifacts::{self, Layout};
use ally_core::control::{Job, JobKind, ProjectInfo, ProjectRef, ProjectSummary, Snapshot};
use ally_core::source::{self, Manifest, Rules};
use ally_core::sync::{self, StateDir};
use ally_core::{stack, storage};
use anyhow::{Context, bail, ensure};
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::path::{Path, PathBuf};

pub struct Paths {
    pub dir: PathBuf,
    pub source: PathBuf,
    pub artifacts: PathBuf,
    pub environment: PathBuf,
    pub state: PathBuf,
}

/// Build output folders that may sit inside the source copy rather than in `artifacts`.
const IN_SOURCE: [&str; 2] = [".venv", "node_modules"];

impl Paths {
    pub fn new(root: &Path, id: &str) -> anyhow::Result<Paths> {
        storage::check_id(id)?;
        let dir = root.join("projects").join(id);
        Ok(Paths {
            source: dir.join("source"),
            artifacts: dir.join("artifacts"),
            environment: dir.join("environment"),
            state: dir.join("state"),
            dir,
        })
    }

    fn metadata(&self) -> PathBuf {
        self.dir.join("project.json")
    }

    fn lock(&self) -> PathBuf {
        self.state.join("work.lock")
    }

    fn staging(&self) -> PathBuf {
        self.state.join("staging")
    }

    fn hashes(&self) -> PathBuf {
        self.state.join("hashes.json")
    }

    fn names(&self) -> PathBuf {
        self.environment.join("names.json")
    }

    /// The exclude patterns the Client sent with its last sync, so the Agent can count
    /// waiting edits by the same rules without the Client.
    fn excludes(&self) -> PathBuf {
        self.state.join("excludes.json")
    }

    /// Build output that sits in the source copy as a real folder: a Python `.venv`, which
    /// refuses a link, and `node_modules`, which npm turns from Ally's link back into a
    /// folder. Neither ever syncs, but both count as build output and `clean` removes them.
    fn output_in_source(&self) -> Vec<PathBuf> {
        IN_SOURCE
            .iter()
            .map(|name| self.source.join(name))
            .filter(|path| fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir()))
            .collect()
    }

    fn sync_state(&self) -> StateDir {
        StateDir::new(&self.state)
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Metadata {
    pub id: String,
    pub name: String,
    pub client: String,
    pub created: u64,
}

/// Create a project's folders if needed. Opening never copies or changes source.
pub fn open(root: &Path, project: &ProjectRef) -> anyhow::Result<ProjectInfo> {
    let paths = Paths::new(root, &project.id)?;
    for text in [&project.name, &project.client] {
        ensure!(
            !text.is_empty() && text.len() <= 255 && !text.chars().any(char::is_control),
            "Invalid project or Client name"
        );
    }
    storage::private_dir(&root.join("projects"))?;
    for dir in [
        &paths.dir,
        &paths.source,
        &paths.artifacts,
        &paths.environment,
        &paths.state,
    ] {
        storage::private_dir(dir)?;
    }
    let existing: Option<Metadata> = storage::read_json(&paths.metadata())?;
    let metadata = Metadata {
        id: project.id.clone(),
        name: project.name.clone(),
        client: project.client.clone(),
        created: existing
            .as_ref()
            .map(|m| m.created)
            .unwrap_or_else(storage::now),
    };
    if existing
        .as_ref()
        .is_none_or(|m| m.name != metadata.name || m.client != metadata.client)
    {
        storage::write_json(&paths.metadata(), &metadata)?;
    }
    Ok(ProjectInfo {
        id: metadata.id,
        name: metadata.name,
        initialized: paths.sync_state().baseline_file().exists(),
    })
}

pub fn load(root: &Path, id: &str) -> anyhow::Result<(Paths, Metadata)> {
    let paths = Paths::new(root, id)?;
    let metadata: Option<Metadata> = storage::read_json(&paths.metadata())?;
    let metadata =
        metadata.context("This project has no copy on the Agent yet. Run ally sync first")?;
    Ok((paths, metadata))
}

/// Which active jobs stop other work on a project. A run owns the project copy while it
/// works, so nothing else may change it. A session is a place to work, like a terminal on
/// the Client, so syncing and running beside it are allowed. Unlinking waits for both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blocking {
    Runs,
    AllJobs,
}

fn blocker(active: &[Job], blocking: Blocking) -> Option<&Job> {
    active
        .iter()
        .find(|job| blocking == Blocking::AllJobs || job.kind == JobKind::Run)
}

/// Hold the project lock and confirm no job that `blocking` names is active.
pub fn acquire(
    root: &Path,
    paths: &Paths,
    metadata: &Metadata,
    blocking: Blocking,
) -> anyhow::Result<File> {
    let active = jobs::active_for(root, &metadata.id)?;
    let lock = storage::try_lock(&paths.lock())?;
    let describe = |kind: JobKind| match kind {
        JobKind::Run => "a run",
        JobKind::Session => "a session",
    };
    if let Some(job) = blocker(&active, blocking) {
        bail!(
            "{} has {} in progress (job {}). Stop it with ally stop {} or wait for it to finish",
            metadata.name,
            describe(job.kind),
            storage::short_id(&job.id),
            storage::short_id(&job.id)
        );
    }
    let Some(lock) = lock else {
        bail!(
            "{} is busy with a sync or another run. Try again when it finishes",
            metadata.name
        );
    };
    ensure!(
        blocker(&jobs::active_for(root, &metadata.id)?, blocking).is_none(),
        "{} started other work just now. Try again",
        metadata.name
    );
    Ok(lock)
}

/// Refuse work while an interrupted sync is waiting to be recovered.
pub fn ensure_ready(paths: &Paths, metadata: &Metadata) -> anyhow::Result<()> {
    let state = paths.sync_state();
    ensure!(
        !state.interrupted(),
        "An interrupted sync of {} needs recovery. Run ally sync to recover it",
        metadata.name
    );
    ensure!(
        state.baseline_file().exists(),
        "{} has not been copied to the Agent yet. Run ally sync first",
        metadata.name
    );
    Ok(())
}

/// A source transfer in progress. Dropping it releases the project lock and removes
/// the staging folder, which happens when the control connection ends for any reason.
pub struct Lease {
    pub token: String,
    project: String,
    pull: bool,
    excludes: Vec<String>,
    before: Manifest,
    stage: PathBuf,
    _lock: File,
}

impl Drop for Lease {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.stage);
    }
}

pub fn begin(
    root: &Path,
    id: &str,
    excludes: Vec<String>,
    pull: bool,
) -> anyhow::Result<(Lease, Snapshot)> {
    let (paths, metadata) = load(root, id)?;
    let lock = acquire(root, &paths, &metadata, Blocking::Runs)?;
    let state = paths.sync_state();
    sync::recover(&paths.source, &state)?;
    storage::write_json(&paths.excludes(), &excludes)?;
    let rules = Rules::new(&paths.source, &excludes)?;
    let baseline = state.baseline()?;
    let before = source::scan(&paths.source, &rules, &baseline, Some(&paths.hashes()))?;

    let _ = fs::remove_dir_all(paths.staging());
    storage::private_dir(&paths.staging())?;
    let token = storage::new_id();
    let stage = paths.staging().join(&token);
    storage::private_dir(&stage)?;

    let transfer_path = match pull {
        true => &paths.source,
        false => &stage,
    };
    let snapshot = Snapshot {
        token: Some(token.clone()),
        manifest: before.clone(),
        baseline,
        transfer_path: transfer_path
            .to_str()
            .context("Agent storage path is not UTF 8")?
            .to_string(),
    };
    let lease = Lease {
        token,
        project: id.to_string(),
        pull,
        excludes,
        before,
        stage,
        _lock: lock,
    };
    Ok((lease, snapshot))
}

pub fn inspect(root: &Path, id: &str, excludes: &[String]) -> anyhow::Result<Snapshot> {
    let (paths, metadata) = load(root, id)?;
    let state = paths.sync_state();
    ensure!(
        !state.interrupted(),
        "An interrupted sync of {} needs recovery. Run ally sync to recover it",
        metadata.name
    );
    storage::write_json(&paths.excludes(), &excludes)?;
    let rules = Rules::new(&paths.source, excludes)?;
    let baseline = state.baseline()?;
    let manifest = source::scan(&paths.source, &rules, &baseline, Some(&paths.hashes()))?;
    Ok(Snapshot {
        token: None,
        manifest,
        baseline,
        transfer_path: String::new(),
    })
}

/// Complete a transfer. A push applies staged files after checking them against the
/// Client's manifest. A pull records only the paths where both copies now agree.
/// The Client's claims are never trusted without checking.
/// Apply a finished transfer. `overwrite` lists the conflicts the owner chose to settle in
/// the Client's favor; any other conflict refuses the whole sync.
pub fn finish(
    root: &Path,
    lease: Lease,
    token: &str,
    manifest: Manifest,
    overwrite: &[String],
) -> anyhow::Result<usize> {
    ensure!(
        lease.token == token,
        "Sync token does not match this transfer"
    );
    let (paths, _) = load(root, &lease.project)?;
    let state = paths.sync_state();
    let rules = Rules::new(&paths.source, &lease.excludes)?;
    let baseline = state.baseline()?;
    let current = source::scan(&paths.source, &rules, &baseline, Some(&paths.hashes()))?;
    ensure!(
        current == lease.before,
        "Agent source changed during the transfer. Run the sync again"
    );

    for name in manifest.keys() {
        storage::relative(name)?;
        ensure!(
            !rules.excluded(name, |f| manifest.contains_key(f)),
            "The Client sent an excluded path: {name}"
        );
    }

    if lease.pull {
        let agreed = sync::agreed(&baseline, &lease.before, &manifest);
        let changed = agreed
            .iter()
            .filter(|(k, v)| baseline.get(*k) != Some(v))
            .count()
            + baseline.keys().filter(|k| !agreed.contains_key(*k)).count();
        storage::write_json(&state.baseline_file(), &agreed)?;
        return Ok(changed);
    }

    let plan = sync::overwrite(
        sync::plan(&baseline, &manifest, &lease.before),
        &manifest,
        overwrite,
    );
    if !plan.conflicts.is_empty() {
        bail!(
            "Both machines changed these paths:\n{}",
            plan.conflicts.join("\n")
        );
    }
    let applied_manifest = sync::merge(&lease.before, &plan.changes, &manifest);
    source::check_links(&applied_manifest, &rules)?;
    sync::apply(
        &paths.source,
        &lease.stage,
        &state,
        &plan,
        &lease.before,
        &manifest,
        token,
    )
}

/// Prepare build output folders and return the environment for work in this project.
pub fn prepare_artifacts(paths: &Paths) -> anyhow::Result<Vec<(String, String)>> {
    let project = stack::Project {
        root: paths.source.clone(),
        stacks: stack::stacks_in(&paths.source),
    };
    let layout = Layout {
        source: paths.source.clone(),
        artifacts: paths.artifacts.clone(),
    };
    let rules = artifacts::rules(&project, &layout);
    storage::private_dir(&paths.artifacts)?;
    for (key, value) in artifacts::variables(&rules) {
        fs::create_dir_all(&value).with_context(|| format!("Could not prepare {key}"))?;
    }
    for (name, target) in artifacts::redirects(&rules) {
        fs::create_dir_all(target)?;
        let link = paths.source.join(name);
        match fs::symlink_metadata(&link) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                std::os::unix::fs::symlink(target, &link)?
            }
            Err(e) => return Err(e.into()),
            Ok(_) => {}
        }
    }
    Ok(artifacts::variables(&rules))
}

/// The file name a stored environment file gets: its target in hex, so a nested target such
/// as `api/.env` becomes one flat name that cannot leave the environment folder.
fn stored_name(target: &str) -> String {
    target.bytes().map(|b| format!("{b:02x}")).collect()
}

/// Remove the link only when it is still Ally's own, then the stored copy.
fn remove_environment(paths: &Paths, target: &str) -> anyhow::Result<()> {
    let stored = paths.environment.join(stored_name(target));
    if let Ok(link) = storage::safe_path(&paths.source, target)
        && let Ok(meta) = fs::symlink_metadata(&link)
        && meta.file_type().is_symlink()
        && fs::read_link(&link).is_ok_and(|t| t == stored)
    {
        fs::remove_file(&link)?;
    }
    match fs::remove_file(&stored) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
        _ => {}
    }
    let mut names: Vec<String> = storage::read_json(&paths.names())?;
    names.retain(|n| n != target);
    storage::write_json(&paths.names(), &names)
}

/// Remove environment files for the projects one Client registered with this Agent.
/// Every project must be idle first. Source copies and backups are preserved.
pub fn unlink(root: &Path, ids: &[String]) -> anyhow::Result<usize> {
    let mut held = Vec::new();
    for id in ids {
        let paths = Paths::new(root, id)?;
        let metadata: Option<Metadata> = storage::read_json(&paths.metadata())?;
        let Some(metadata) = metadata else {
            continue;
        };
        let lock = acquire(root, &paths, &metadata, Blocking::AllJobs)?;
        held.push((paths, lock));
    }
    let mut removed = 0;
    for (paths, _lock) in &held {
        let names: Vec<String> = storage::read_json(&paths.names())?;
        for name in names {
            remove_environment(paths, &name)?;
            removed += 1;
        }
    }
    Ok(removed)
}

/// Every project copy on the Agent. A project that cannot be read is left out rather than
/// failing the whole list. `home` holds the `~/Ally` folder that names each copy.
pub fn list(root: &Path, home: Option<&Path>) -> anyhow::Result<Vec<ProjectSummary>> {
    let Ok(items) = fs::read_dir(root.join("projects")) else {
        return Ok(Vec::new());
    };
    let linked = crate::clients::names(root)?;
    let folders: Vec<(String, PathBuf)> = home
        .and_then(|home| fs::read_dir(home.join("Ally")).ok())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|item| {
            let target = fs::read_link(item.path()).ok()?;
            Some((item.file_name().to_str()?.to_string(), target))
        })
        .collect();
    let mut summaries: Vec<ProjectSummary> = items
        .flatten()
        .filter_map(|item| load(root, item.file_name().to_str()?).ok())
        .map(|(paths, metadata)| {
            let folder = folders
                .iter()
                .find(|(_, target)| *target == paths.source)
                .map(|(name, _)| name.clone());
            let linked = linked.contains(&metadata.client);
            summary(&paths, metadata, folder, linked)
        })
        .collect();
    summaries.sort_by(|a, b| a.name.cmp(&b.name).then(a.client.cmp(&b.client)));
    Ok(summaries)
}

fn summary(
    paths: &Paths,
    metadata: Metadata,
    folder: Option<String>,
    linked: bool,
) -> ProjectSummary {
    let in_source: u64 = paths.output_in_source().iter().map(|path| size(path)).sum();
    let baseline = paths.sync_state().baseline_file();
    let synced = fs::metadata(&baseline)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|age| age.as_secs());
    ProjectSummary {
        id: metadata.id,
        name: metadata.name,
        client: metadata.client,
        folder,
        linked,
        environment: environment_files(&paths.source, &paths.source),
        copy: size(&paths.source).saturating_sub(in_source),
        build: size(&paths.artifacts) + in_source,
        waiting: match synced {
            Some(_) => waiting(paths).unwrap_or(0),
            None => 0,
        },
        synced,
    }
}

/// Environment files under `dir`, as paths from `top`. Links are not followed, so build
/// output linked into the copy is never searched.
fn environment_files(top: &Path, dir: &Path) -> Vec<String> {
    let Ok(items) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for item in items.flatten() {
        let path = item.path();
        let Ok(kind) = item.file_type() else {
            continue;
        };
        let name = item.file_name();
        if kind.is_dir() && !IN_SOURCE.iter().any(|skip| name == *skip) {
            found.extend(environment_files(top, &path));
        } else if kind.is_file()
            && source::is_environment_name(&name.to_string_lossy())
            && let Ok(relative) = path.strip_prefix(top)
        {
            found.push(relative.to_string_lossy().to_string());
        }
    }
    found.sort();
    found
}

fn waiting(paths: &Paths) -> anyhow::Result<usize> {
    let state = paths.sync_state();
    ensure!(!state.interrupted(), "An interrupted sync needs recovery");
    let excludes: Vec<String> = storage::read_json(&paths.excludes())?;
    let rules = Rules::new(&paths.source, &excludes)?;
    let baseline = state.baseline()?;
    let current = source::scan(&paths.source, &rules, &baseline, Some(&paths.hashes()))?;
    Ok(changed(&baseline, &current))
}

/// Paths added, changed, or deleted since the baseline.
fn changed(baseline: &Manifest, current: &Manifest) -> usize {
    current
        .iter()
        .filter(|(name, entry)| baseline.get(*name) != Some(entry))
        .count()
        + baseline
            .keys()
            .filter(|name| !current.contains_key(*name))
            .count()
}

/// Remove a project's build output, and return how many bytes it held. Waits for every job,
/// since a session may be using the output.
pub fn clean(root: &Path, id: &str) -> anyhow::Result<u64> {
    let (paths, metadata) = load(root, id)?;
    let _lock = acquire(root, &paths, &metadata, Blocking::AllJobs)?;
    let in_source = paths.output_in_source();
    let bytes = size(&paths.artifacts) + in_source.iter().map(|path| size(path)).sum::<u64>();
    fs::remove_dir_all(&paths.artifacts)
        .or_else(|e| match e.kind() {
            std::io::ErrorKind::NotFound => Ok(()),
            _ => Err(e),
        })
        .with_context(|| format!("Could not remove the build output of {}", metadata.name))?;
    storage::private_dir(&paths.artifacts)?;
    for path in in_source {
        fs::remove_dir_all(&path)
            .with_context(|| format!("Could not remove {} in {}", path.display(), metadata.name))?;
    }
    Ok(bytes)
}

/// Delete a project from the Agent, with its `~/Ally` link. Waits for every job.
pub fn remove(root: &Path, home: Option<&Path>, id: &str) -> anyhow::Result<()> {
    let (paths, metadata) = load(root, id)?;
    let _lock = acquire(root, &paths, &metadata, Blocking::AllJobs)?;
    if let Some(Ok(items)) = home.map(|home| fs::read_dir(home.join("Ally"))) {
        for item in items.flatten() {
            if fs::read_link(item.path()).is_ok_and(|target| target == paths.source) {
                fs::remove_file(item.path())?;
            }
        }
    }
    fs::remove_dir_all(&paths.dir)
        .with_context(|| format!("Could not remove {} from the Agent", metadata.name))
}

/// The bytes under `path`, without following links, so a project link never counts twice.
pub fn size(path: &Path) -> u64 {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return 0;
    };
    match meta.is_dir() {
        true => fs::read_dir(path)
            .map(|items| items.flatten().map(|item| size(&item.path())).sum())
            .unwrap_or(0),
        false => meta.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Root;
    use std::os::unix::fs::PermissionsExt;

    fn active(kind: JobKind) -> Job {
        crate::jobs::new_job(kind, Some(("p", "app")), "work".into())
    }

    #[test]
    fn a_session_blocks_only_unlinking_while_a_run_blocks_everything() {
        let session = [active(JobKind::Session)];
        assert!(blocker(&session, Blocking::Runs).is_none());
        assert!(blocker(&session, Blocking::AllJobs).is_some());
        let both = [active(JobKind::Session), active(JobKind::Run)];
        assert_eq!(
            blocker(&both, Blocking::Runs).map(|job| job.kind),
            Some(JobKind::Run)
        );
        assert!(blocker(&[], Blocking::AllJobs).is_none());
    }

    fn project(root: &Path) -> String {
        let id = storage::new_id();
        open(
            root,
            &ProjectRef {
                id: id.clone(),
                name: "app".into(),
                client: "laptop".into(),
            },
        )
        .unwrap();
        id
    }

    fn initialize(root: &Path, id: &str) -> Paths {
        let paths = Paths::new(root, id).unwrap();
        storage::write_json(&paths.sync_state().baseline_file(), &Manifest::new()).unwrap();
        paths
    }

    #[test]
    fn projects_keep_data_in_separate_private_folders() {
        let root = Root::new();
        let a = project(&root.0);
        let b = project(&root.0);
        assert_ne!(
            Paths::new(&root.0, &a).unwrap().source,
            Paths::new(&root.0, &b).unwrap().source
        );
        let paths = Paths::new(&root.0, &a).unwrap();
        for dir in [
            &paths.source,
            &paths.artifacts,
            &paths.environment,
            &paths.state,
        ] {
            assert_eq!(
                fs::metadata(dir).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
        assert!(Paths::new(&root.0, "../../etc").is_err());
    }

    #[test]
    fn a_lease_blocks_other_work_until_dropped() {
        let root = Root::new();
        let id = project(&root.0);
        let (lease, snapshot) = begin(&root.0, &id, Vec::new(), false).unwrap();
        assert!(Path::new(&snapshot.transfer_path).is_dir());
        assert!(begin(&root.0, &id, Vec::new(), false).is_err());
        let stage = PathBuf::from(&snapshot.transfer_path);
        drop(lease);
        assert!(!stage.exists());
        assert!(begin(&root.0, &id, Vec::new(), false).is_ok());
    }

    #[test]
    fn a_push_installs_verified_files_and_rejects_excluded_paths() {
        let root = Root::new();
        let id = project(&root.0);
        let (lease, snapshot) = begin(&root.0, &id, Vec::new(), false).unwrap();
        let stage = PathBuf::from(&snapshot.transfer_path);
        fs::create_dir_all(stage.join("src")).unwrap();
        fs::write(stage.join("src/main.rs"), "fn main() {}").unwrap();
        let client = TempDirLike::with(&[("src/main.rs", "fn main() {}")]);
        let manifest = source::scan(
            &client.0,
            &Rules::new(&client.0, &[]).unwrap(),
            &Manifest::new(),
            None,
        )
        .unwrap();
        let token = lease.token.clone();
        assert_eq!(
            finish(&root.0, lease, &token, manifest.clone(), &[]).unwrap(),
            1
        );
        let paths = Paths::new(&root.0, &id).unwrap();
        assert_eq!(
            fs::read_to_string(paths.source.join("src/main.rs")).unwrap(),
            "fn main() {}"
        );
        assert_eq!(paths.sync_state().baseline().unwrap(), manifest);

        let (lease, _) = begin(&root.0, &id, Vec::new(), false).unwrap();
        let mut sneaky = manifest.clone();
        sneaky.insert(".env".into(), manifest["src/main.rs"].clone());
        let token = lease.token.clone();
        let error = finish(&root.0, lease, &token, sneaky, &[])
            .unwrap_err()
            .to_string();
        assert!(error.contains("excluded"), "{error}");
        assert!(!paths.source.join(".env").exists());
    }

    #[test]
    fn a_push_refuses_links_that_leave_the_project() {
        let root = Root::new();
        let id = project(&root.0);
        let (lease, _) = begin(&root.0, &id, Vec::new(), false).unwrap();
        let mut manifest = Manifest::new();
        manifest.insert(
            "escape".into(),
            source::Entry {
                hash: "x".into(),
                executable: false,
                link: Some("../../../../etc/passwd".into()),
            },
        );
        let token = lease.token.clone();
        assert!(finish(&root.0, lease, &token, manifest, &[]).is_err());
        assert!(
            fs::symlink_metadata(Paths::new(&root.0, &id).unwrap().source.join("escape")).is_err()
        );
    }

    #[test]
    fn a_pull_records_only_agreed_paths() {
        let root = Root::new();
        let id = project(&root.0);
        let paths = initialize(&root.0, &id);
        fs::write(paths.source.join("agent.txt"), "from agent").unwrap();
        let (lease, snapshot) = begin(&root.0, &id, Vec::new(), true).unwrap();
        assert_eq!(snapshot.transfer_path, paths.source.to_str().unwrap());
        let token = lease.token.clone();
        finish(&root.0, lease, &token, Manifest::new(), &[]).unwrap();
        assert!(paths.sync_state().baseline().unwrap().is_empty());
        let (lease, snapshot) = begin(&root.0, &id, Vec::new(), true).unwrap();
        let token = lease.token.clone();
        finish(&root.0, lease, &token, snapshot.manifest.clone(), &[]).unwrap();
        assert_eq!(paths.sync_state().baseline().unwrap(), snapshot.manifest);
    }

    /// An environment file stored the way older builds of `ally env add` left them.
    fn environment_file(paths: &Paths) -> PathBuf {
        let stored = paths.environment.join(stored_name(".env"));
        fs::write(&stored, "SECRET=1").unwrap();
        std::os::unix::fs::symlink(&stored, paths.source.join(".env")).unwrap();
        storage::write_json(&paths.names(), &[".env"]).unwrap();
        stored
    }

    #[test]
    fn unlink_removes_environment_files_but_keeps_source() {
        let root = Root::new();
        let id = project(&root.0);
        let other = project(&root.0);
        let paths = initialize(&root.0, &id);
        initialize(&root.0, &other);
        fs::write(paths.source.join("main.rs"), "code").unwrap();
        let stored = environment_file(&paths);
        let kept = environment_file(&Paths::new(&root.0, &other).unwrap());
        assert_eq!(unlink(&root.0, std::slice::from_ref(&id)).unwrap(), 1);
        assert!(paths.source.join("main.rs").exists());
        assert!(fs::symlink_metadata(paths.source.join(".env")).is_err());
        assert!(!stored.exists());
        assert!(kept.exists());
    }

    #[test]
    fn size_counts_files_but_not_through_links() {
        let root = Root::new();
        fs::write(root.0.join("a"), "12345").unwrap();
        fs::create_dir(root.0.join("d")).unwrap();
        fs::write(root.0.join("d/b"), "123").unwrap();
        std::os::unix::fs::symlink(root.0.join("d"), root.0.join("link")).unwrap();
        let link_itself = fs::symlink_metadata(root.0.join("link")).unwrap().len();
        assert_eq!(size(&root.0), 8 + link_itself);
    }

    #[test]
    fn the_list_counts_copy_build_and_waiting_edits() {
        let root = Root::new();
        let id = project(&root.0);
        let unsynced = project(&root.0);
        let paths = initialize(&root.0, &id);
        fs::write(paths.source.join("main.rs"), "12345").unwrap();
        fs::create_dir(paths.source.join(".venv")).unwrap();
        fs::write(paths.source.join(".venv").join("lib"), "123").unwrap();
        fs::write(paths.artifacts.join("app"), "1234567").unwrap();
        let list = list(&root.0, None).unwrap();
        let synced = list.iter().find(|p| p.id == id).unwrap();
        assert_eq!(synced.client, "laptop");
        assert_eq!(synced.build, 7 + size(&paths.source.join(".venv")));
        assert_eq!(
            synced.copy,
            size(&paths.source) - size(&paths.source.join(".venv"))
        );
        assert!(synced.synced.is_some());
        assert_eq!(synced.waiting, 1);
        let fresh = list.iter().find(|p| p.id == unsynced).unwrap();
        assert_eq!((fresh.synced, fresh.waiting), (None, 0));
    }

    /// The list names each copy by its `~/Ally` folder, says whether its Client is still
    /// linked, and finds environment files at any depth without entering linked folders.
    #[test]
    fn the_list_names_the_folder_the_client_and_environment_files() {
        let root = Root::new();
        let home = Root::new();
        let id = project(&root.0);
        let paths = initialize(&root.0, &id);
        fs::write(paths.source.join(".env"), "SECRET=1").unwrap();
        fs::create_dir(paths.source.join("web")).unwrap();
        fs::write(paths.source.join("web/.env.local"), "SECRET=2").unwrap();
        fs::write(paths.artifacts.join(".env"), "not source").unwrap();
        std::os::unix::fs::symlink(&paths.artifacts, paths.source.join("node_modules")).unwrap();
        let folder = home.0.join("Ally");
        fs::create_dir(&folder).unwrap();
        std::os::unix::fs::symlink(&paths.source, folder.join("app-1234")).unwrap();

        let unlinked = &list(&root.0, Some(&home.0)).unwrap()[0];
        assert_eq!(unlinked.folder.as_deref(), Some("app-1234"));
        assert!(!unlinked.linked);
        assert_eq!(unlinked.environment, [".env", "web/.env.local"]);

        let key = iroh::SecretKey::generate().public().to_string();
        crate::clients::allow(&root.0, "laptop", &key).unwrap();
        assert!(list(&root.0, None).unwrap()[0].linked);
        assert_eq!(list(&root.0, None).unwrap()[0].folder, None);
    }

    #[test]
    fn waiting_edits_count_additions_changes_and_deletions() {
        let entry = |hash: &str| source::Entry {
            hash: hash.into(),
            executable: false,
            link: None,
        };
        let baseline = Manifest::from([("a".into(), entry("1")), ("b".into(), entry("2"))]);
        let current = Manifest::from([("a".into(), entry("9")), ("c".into(), entry("3"))]);
        assert_eq!(changed(&baseline, &current), 3);
        assert_eq!(changed(&baseline, &baseline), 0);
    }

    #[test]
    fn clean_removes_build_output_and_the_venv_but_keeps_source() {
        let root = Root::new();
        let id = project(&root.0);
        let paths = initialize(&root.0, &id);
        fs::write(paths.source.join("main.rs"), "code").unwrap();
        fs::create_dir(paths.source.join(".venv")).unwrap();
        fs::write(paths.source.join(".venv").join("lib"), "123").unwrap();
        fs::write(paths.artifacts.join("app"), "1234567").unwrap();
        let expected = 7 + size(&paths.source.join(".venv"));
        assert_eq!(clean(&root.0, &id).unwrap(), expected);
        assert!(paths.artifacts.is_dir());
        assert_eq!(fs::read_dir(&paths.artifacts).unwrap().count(), 0);
        assert!(!paths.source.join(".venv").exists());
        assert!(paths.source.join("main.rs").exists());
    }

    /// npm replaces Ally's `node_modules` link with a real folder. It is still build
    /// output: counted as such, removed by clean, and never searched for environment files.
    #[test]
    fn a_node_modules_folder_in_the_copy_is_build_output() {
        let root = Root::new();
        let id = project(&root.0);
        let paths = initialize(&root.0, &id);
        let modules = paths.source.join("node_modules");
        fs::create_dir_all(modules.join("pkg")).unwrap();
        fs::write(modules.join("pkg/.env.example"), "1234567890").unwrap();
        fs::write(paths.source.join("index.js"), "1").unwrap();
        let held = size(&modules);
        let listed = &list(&root.0, None).unwrap()[0];
        assert_eq!(listed.build, held);
        assert!(listed.environment.is_empty());
        assert_eq!(clean(&root.0, &id).unwrap(), held);
        assert!(!modules.exists());
        assert!(paths.source.join("index.js").exists());
    }

    #[test]
    fn remove_deletes_the_project_and_only_its_own_link() {
        let root = Root::new();
        let home = Root::new();
        let id = project(&root.0);
        let paths = initialize(&root.0, &id);
        let folder = home.0.join("Ally");
        fs::create_dir(&folder).unwrap();
        std::os::unix::fs::symlink(&paths.source, folder.join("app")).unwrap();
        std::os::unix::fs::symlink(&home.0, folder.join("other")).unwrap();
        remove(&root.0, Some(&home.0), &id).unwrap();
        assert!(!paths.dir.exists());
        assert!(fs::symlink_metadata(folder.join("app")).is_err());
        assert!(fs::symlink_metadata(folder.join("other")).is_ok());
        assert!(list(&root.0, None).unwrap().is_empty());
    }

    #[test]
    fn clean_and_remove_wait_for_running_work() {
        let root = Root::new();
        let id = project(&root.0);
        initialize(&root.0, &id);
        let run = crate::jobs::new_job(JobKind::Run, Some((&id, "app")), "cargo build".into());
        crate::jobs::save(&root.0, &run).unwrap();
        assert!(clean(&root.0, &id).is_err());
        assert!(remove(&root.0, None, &id).is_err());
    }

    struct TempDirLike(PathBuf);

    impl TempDirLike {
        fn with(files: &[(&str, &str)]) -> TempDirLike {
            let path = std::env::temp_dir().join(format!("ally-client-{}", storage::new_id()));
            for (name, body) in files {
                let file = path.join(name);
                fs::create_dir_all(file.parent().unwrap()).unwrap();
                fs::write(file, body).unwrap();
            }
            TempDirLike(path)
        }
    }

    impl Drop for TempDirLike {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}
