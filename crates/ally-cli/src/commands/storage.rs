//! `ally storage`: the project copies Ally keeps on the Agent, and freeing their space.

use super::ps::ago;
use crate::client::{self, unexpected};
use crate::project;
use ally_core::config::{Agent, Config};
use ally_core::control::{ProjectSummary, Request, Response};
use ally_core::presentation::{self, Style, Tone};
use ally_core::{step, storage};

pub async fn list(agent: Option<String>) -> anyhow::Result<i32> {
    let config = Config::load()?;
    let target = config.resolve(agent.as_deref())?;
    let projects = fetch(target).await?;
    let unused = unused(target, &projects)?;
    print!(
        "\n{}",
        render(
            &target.name,
            &projects,
            &unused,
            storage::now(),
            Style::stdout()
        )
    );
    Ok(0)
}

pub async fn clean(agent: Option<String>, project: Option<String>) -> anyhow::Result<i32> {
    let config = Config::load()?;
    let target = config.resolve(agent.as_deref())?;
    let projects = fetch(target).await?;
    let found = choose(&projects, project.as_deref(), &target.name)?;
    let shown = shown(found);
    if found.build == 0 {
        presentation::success(format!("{shown} has no build output on {}", target.name));
        return Ok(0);
    }
    let cleaning = step::start(format!(
        "Freeing the build output of {shown} on {}",
        target.name
    ));
    let request = Request::CleanProject {
        project: found.id.clone(),
    };
    let Response::Cleaned { bytes } = client::request(target, request).await? else {
        return Err(unexpected());
    };
    cleaning.done(format!(
        "Freed {} on {} · {shown}",
        size(bytes),
        target.name
    ));
    eprintln!("  The next build there starts from scratch");
    Ok(0)
}

pub async fn remove(agent: Option<String>, project: Option<String>) -> anyhow::Result<i32> {
    let config = Config::load()?;
    let target = config.resolve(agent.as_deref())?;
    let projects = fetch(target).await?;
    let found = choose(&projects, project.as_deref(), &target.name)?;
    let shown = shown(found);
    eprintln!("\nThis removes {shown} from {}:\n", target.name);
    for line in listing(found, Style::stderr()) {
        eprintln!("{line}");
    }
    anyhow::ensure!(
        crate::ssh::wants_terminal(),
        "Run ally storage remove in a terminal to confirm, since it deletes files"
    );
    if !presentation::confirm_deleting("Remove it?".to_string()).await? {
        presentation::success("Nothing was removed");
        return Ok(0);
    }
    let removing = step::start(format!("Removing {shown} from {}", target.name));
    let request = Request::RemoveProject {
        project: found.id.clone(),
    };
    let Response::Done = client::request(target, request).await? else {
        return Err(unexpected());
    };
    removing.done(format!("Removed {shown} from {}", target.name));
    Ok(0)
}

async fn fetch(target: &Agent) -> anyhow::Result<Vec<ProjectSummary>> {
    let Response::Projects(projects) = client::fetch(target, Request::Projects).await? else {
        return Err(unexpected());
    };
    Ok(projects)
}

/// Which copies no linked machine uses: their Client is no longer linked, or they are
/// this machine's but it no longer knows their ID, as after linking again with an older
/// version.
fn unused(target: &Agent, projects: &[ProjectSummary]) -> anyhow::Result<Vec<bool>> {
    let known = project::for_agent(&project::client_root()?, &target.name)?;
    let me = target.client_name();
    Ok(projects
        .iter()
        .map(|p| !p.linked || (p.client == me && !known.iter().any(|k| k.id == p.id)))
        .collect())
}

/// A copy's name as the person sees it: its `~/Ally` folder, which tells apart copies
/// that share a project name.
fn shown(project: &ProjectSummary) -> &str {
    project.folder.as_deref().unwrap_or(&project.name)
}

/// The copy a folder name, a project name, an ID prefix, or the current folder points to.
/// A project name shared by several copies asks for the folder name instead.
fn choose<'a>(
    projects: &'a [ProjectSummary],
    query: Option<&str>,
    agent: &str,
) -> anyhow::Result<&'a ProjectSummary> {
    let Some(query) = query else {
        let local = project::require(None)?;
        let registered = project::for_agent(&project::client_root()?, agent)?;
        let id = registered
            .iter()
            .find(|p| p.root == local.root)
            .map(|p| &p.id);
        return projects.iter().find(|p| Some(&p.id) == id).ok_or_else(|| {
            anyhow::anyhow!(
                "{} has no copy on {agent}. See every copy with ally storage",
                local.name
            )
        });
    };
    if let Some(found) = projects.iter().find(|p| p.folder.as_deref() == Some(query)) {
        return Ok(found);
    }
    let named: Vec<&ProjectSummary> = projects.iter().filter(|p| p.name == query).collect();
    let matches = match named.is_empty() && query.len() >= 4 {
        true => projects
            .iter()
            .filter(|p| p.id.starts_with(query))
            .collect(),
        false => named,
    };
    match matches.as_slice() {
        [found] => Ok(found),
        [] => anyhow::bail!("No copy named {query} on {agent}. See every copy with ally storage"),
        several => anyhow::bail!(
            "{} copies are named {query} on {agent}. Use one of these names: {}",
            several.len(),
            several
                .iter()
                .map(|p| p
                    .folder
                    .clone()
                    .unwrap_or_else(|| storage::short_id(&p.id).to_string()))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// What `remove` deletes, and anything that exists only on the Agent, in yellow.
fn listing(project: &ProjectSummary, style: Style) -> Vec<String> {
    let mut lines = vec![
        presentation::detail_line(style, "Source", size(project.copy)),
        presentation::detail_line(style, "Build output", build(project.build)),
    ];
    let mut only_there = project.environment.clone();
    if project.waiting > 0 {
        only_there.push(format!(
            "{} never brought back",
            presentation::plural(project.waiting, "edit")
        ));
    }
    if !only_there.is_empty() {
        lines.push(presentation::detail_line(
            style,
            "Only there",
            style.paint(only_there.join(", "), Tone::Warning),
        ));
    }
    lines
}

/// Columns widen to fit the longest name, up to a limit, so a long name cannot push the
/// rest of the table off screen.
const NAME_LIMIT: usize = 40;

pub fn render(
    agent: &str,
    projects: &[ProjectSummary],
    unused: &[bool],
    now: u64,
    style: Style,
) -> String {
    if projects.is_empty() {
        return format!(
            "{}\n\n{} in a project copies it there\n",
            style.heading(format!("{agent} keeps no project copies yet")),
            style.paint("ally sync", Tone::Info)
        );
    }
    let total: u64 = projects.iter().map(|p| p.copy + p.build).sum();
    let copies = match projects.len() {
        1 => "1 project copy".to_string(),
        count => format!("{count} project copies"),
    };
    let mut output = format!(
        "{}\n\n",
        style.heading(format!("{agent} keeps {} for {copies}", size(total)))
    );
    let cut = |text: &str| match text.chars().count() > NAME_LIMIT {
        true => format!("{}…", text.chars().take(NAME_LIMIT - 1).collect::<String>()),
        false => text.to_string(),
    };
    let width = projects
        .iter()
        .map(|p| cut(shown(p)).chars().count())
        .chain(["PROJECT".len()])
        .max()
        .unwrap_or(0);
    let header = format!(
        "{:<width$}  {:<9}  {:<12}  LAST SYNC",
        "PROJECT", "SOURCE", "BUILD OUTPUT"
    );
    output.push_str(&format!("  {}\n", style.heading(header)));
    for (project, unused) in projects.iter().zip(unused) {
        let synced = match project.synced {
            Some(time) => ago(now.saturating_sub(time)),
            None => "never".to_string(),
        };
        let mut line = format!(
            "  {:<width$}  {:<9}  {:<12}  {:<9}",
            cut(shown(project)),
            size(project.copy),
            build(project.build),
            synced
        );
        if *unused {
            line.push_str(&format!("  {}", style.paint("unused", Tone::Warning)));
        }
        output.push_str(line.trim_end());
        output.push('\n');
    }
    output.push_str(&format!(
        "\nFree build output with {}\nRemove a whole copy with {}\n",
        style.paint("ally storage clean <project>", Tone::Info),
        style.paint("ally storage remove <project>", Tone::Info)
    ));
    output
}

fn build(bytes: u64) -> String {
    match bytes {
        0 => "none".to_string(),
        bytes => size(bytes),
    }
}

/// Bytes in the largest unit that keeps the number short: `640 KiB`, `9.1 MiB`, `1.4 GiB`.
fn size(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    let bytes = bytes as f64;
    match bytes {
        b if b < KIB * KIB => format!("{:.0} KiB", (b / KIB).ceil()),
        b if b < KIB * KIB * KIB => format!("{:.1} MiB", b / (KIB * KIB)),
        b => format!("{:.1} GiB", b / (KIB * KIB * KIB)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(name: &str, folder: &str, id: &str) -> ProjectSummary {
        ProjectSummary {
            id: id.into(),
            name: name.into(),
            client: "laptop".into(),
            folder: Some(folder.into()),
            linked: true,
            environment: Vec::new(),
            copy: 2 * 1024 * 1024,
            build: 1536 * 1024 * 1024,
            synced: Some(10_000 - 7_200),
            waiting: 0,
        }
    }

    #[test]
    fn sizes_use_the_unit_that_keeps_them_short() {
        assert_eq!(size(0), "0 KiB");
        assert_eq!(size(1), "1 KiB");
        assert_eq!(size(640 * 1024), "640 KiB");
        assert_eq!(size(9 * 1024 * 1024 + 100_000), "9.1 MiB");
        assert_eq!(size(1536 * 1024 * 1024), "1.5 GiB");
        assert_eq!(build(0), "none");
    }

    #[test]
    fn the_table_names_folders_and_marks_unused_copies() {
        let mut web = summary("webtest", "webtest", "bbbb2222");
        web.build = 0;
        let old = summary("webtest", "webtest-01523eb5", "aaaa1111");
        let mut fresh = summary("notes", "notes", "cccc3333");
        fresh.synced = None;
        let projects = [web, old, fresh];
        let text = render(
            "archbox",
            &projects,
            &[false, true, false],
            10_000,
            Style::new(false),
        );
        assert!(
            text.starts_with("archbox keeps 3.0 GiB for 3 project copies\n\n"),
            "{text}"
        );
        assert!(text.contains("  PROJECT           SOURCE     BUILD OUTPUT  LAST SYNC\n"));
        assert!(text.contains("  webtest           2.0 MiB    none          2h ago\n"));
        assert!(text.contains("  webtest-01523eb5  2.0 MiB    1.5 GiB       2h ago     unused\n"));
        assert!(text.contains("  notes             2.0 MiB    1.5 GiB       never\n"));
        assert!(text.contains("Free build output with ally storage clean <project>"));
        assert!(text.contains("Remove a whole copy with ally storage remove <project>"));
        assert!(!text.contains('\x1b'));
    }

    #[test]
    fn an_empty_agent_says_how_to_add_a_copy() {
        let text = render("archbox", &[], &[], 0, Style::new(false));
        assert!(text.contains("archbox keeps no project copies yet"));
        assert!(text.contains("ally sync in a project copies it there"));
    }

    #[test]
    fn a_copy_is_chosen_by_folder_name_or_id_and_a_shared_name_asks_for_the_folder() {
        let projects = [
            summary("app", "app", "aaaa1111"),
            summary("app", "app-bbbb2222", "bbbb2222"),
            summary("api", "api", "cccc3333"),
        ];
        let target = "archbox";
        assert_eq!(
            choose(&projects, Some("app-bbbb2222"), target).unwrap().id,
            "bbbb2222"
        );
        assert_eq!(
            choose(&projects, Some("app"), target).unwrap().id,
            "aaaa1111"
        );
        assert_eq!(
            choose(&projects, Some("cccc"), target).unwrap().id,
            "cccc3333"
        );
        let mut unnamed = projects.clone();
        unnamed[0].folder = None;
        let shared = choose(&unnamed, Some("app"), target)
            .unwrap_err()
            .to_string();
        assert!(
            shared.contains("2 copies are named app on archbox"),
            "{shared}"
        );
        assert!(shared.contains("aaaa1111, app-bbbb2222"), "{shared}");
        let missing = choose(&projects, Some("web"), target)
            .unwrap_err()
            .to_string();
        assert!(missing.starts_with("No copy named web on archbox"));
        assert!(choose(&projects, Some("cc"), target).is_err());
    }

    #[test]
    fn removing_lists_what_exists_only_on_the_agent() {
        let mut project = summary("api", "api", "aaaa1111");
        let plain = listing(&project, Style::new(false)).join("\n");
        assert!(plain.contains("Source       2.0 MiB"), "{plain}");
        assert!(!plain.contains("Only there"));
        project.environment = vec![".env".into()];
        project.waiting = 2;
        let warned = listing(&project, Style::new(false)).join("\n");
        assert!(
            warned.contains("Only there   .env, 2 edits never brought back"),
            "{warned}"
        );
    }
}
