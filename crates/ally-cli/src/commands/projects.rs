//! `ally projects`: the project copies on the Agent, and freeing their space.

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
    print!(
        "\n{}",
        render(&target.name, &projects, storage::now(), Style::stdout())
    );
    Ok(0)
}

pub async fn clean(agent: Option<String>, project: Option<String>) -> anyhow::Result<i32> {
    let config = Config::load()?;
    let target = config.resolve(agent.as_deref())?;
    let projects = fetch(target).await?;
    let found = choose(&projects, project.as_deref(), &target.name)?;
    if found.build == 0 {
        presentation::success(format!(
            "{} has no build output on {}",
            found.name, target.name
        ));
        return Ok(0);
    }
    let cleaning = step::start(format!(
        "Removing build output of {} on {}",
        found.name, target.name
    ));
    let request = Request::CleanProject {
        project: found.id.clone(),
    };
    let Response::Cleaned { bytes } = client::request(target, request).await? else {
        return Err(unexpected());
    };
    cleaning.done(format!(
        "Freed {} of build output from {} on {}",
        size(bytes),
        found.name,
        target.name
    ));
    presentation::detail("Next", "the next build or install makes it again");
    Ok(0)
}

pub async fn remove(agent: Option<String>, project: Option<String>) -> anyhow::Result<i32> {
    let config = Config::load()?;
    let target = config.resolve(agent.as_deref())?;
    let projects = fetch(target).await?;
    let found = choose(&projects, project.as_deref(), &target.name)?;
    eprintln!("\n  This removes {} from {}:\n", found.name, target.name);
    for line in listing(found, Style::stderr()) {
        eprint!("{line}");
    }
    anyhow::ensure!(
        crate::ssh::wants_terminal(),
        "Run ally projects remove in a terminal to confirm, since it deletes files"
    );
    if !presentation::confirm_deleting("Remove it?".to_string()).await? {
        presentation::success("Nothing was removed");
        return Ok(0);
    }
    let removing = step::start(format!("Removing {} from {}", found.name, target.name));
    let request = Request::RemoveProject {
        project: found.id.clone(),
    };
    let Response::Done = client::request(target, request).await? else {
        return Err(unexpected());
    };
    removing.done(format!("Removed {} from {}", found.name, target.name));
    Ok(0)
}

async fn fetch(target: &Agent) -> anyhow::Result<Vec<ProjectSummary>> {
    let Response::Projects(projects) = client::fetch(target, Request::Projects).await? else {
        return Err(unexpected());
    };
    Ok(projects)
}

/// The project a name, an ID prefix, or the current folder points to. Two Clients may
/// each have a project with the same name, so a name that matches more than one asks for
/// the ID instead.
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
                "{} has no copy on {agent}. See every project with ally projects",
                local.name
            )
        });
    };
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
        [] => anyhow::bail!(
            "No project named {query} on {agent}. See every project with ally projects"
        ),
        several => anyhow::bail!(
            "{} projects are named {query} on {agent}. Use the ID of the one you mean: {}",
            several.len(),
            several
                .iter()
                .map(|p| format!("{} from {}", storage::short_id(&p.id), p.client))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn listing(project: &ProjectSummary, style: Style) -> Vec<String> {
    let copy = match project.waiting {
        0 => size(project.copy),
        waiting => format!(
            "{}, including {}",
            size(project.copy),
            style.paint(
                format!("{} not brought back", presentation::plural(waiting, "edit")),
                Tone::Warning
            )
        ),
    };
    let mut lines = vec![
        presentation::row("Copy", copy),
        presentation::row("Build", size(project.build)),
        presentation::row("From", &project.client),
        presentation::row("Kept", "the project on its Client"),
    ];
    if project.waiting > 0 {
        lines.push(style.dim(presentation::row(
            "Keep edits",
            "run ally sync --pull in the project first",
        )));
    }
    lines
}

/// Columns widen to fit the longest name, up to a limit, so a long name cannot push the
/// rest of the table off screen.
const NAME_LIMIT: usize = 40;

pub fn render(agent: &str, projects: &[ProjectSummary], now: u64, style: Style) -> String {
    let mut output = format!("{}\n\n", style.heading(format!("Projects on {agent}")));
    if projects.is_empty() {
        output.push_str(&format!(
            "  None yet. {} in a project copies it\n",
            style.paint("ally sync", Tone::Info)
        ));
        return output;
    }
    let cut = |text: &str| match text.chars().count() > NAME_LIMIT {
        true => format!("{}…", text.chars().take(NAME_LIMIT - 1).collect::<String>()),
        false => text.to_string(),
    };
    let widest = |column: fn(&ProjectSummary) -> &str, heading: &str| {
        projects
            .iter()
            .map(|p| cut(column(p)).chars().count())
            .chain([heading.len()])
            .max()
            .unwrap_or(0)
    };
    let name = widest(|p| &p.name, "PROJECT");
    let client = widest(|p| &p.client, "CLIENT");
    let header = format!(
        "{:<name$}  {:<client$}  {:>8}  {:>8}  {:<9}  {}",
        "PROJECT", "CLIENT", "COPY", "BUILD", "LAST SYNC", "EDITS WAITING"
    );
    output.push_str(&format!("  {}\n", style.dim(header)));
    for project in projects {
        let synced = match project.synced {
            Some(time) => format!("{:<9}", ago(now.saturating_sub(time))),
            None => style.dim(format!("{:<9}", "never")),
        };
        let build = match project.build {
            0 => String::new(),
            bytes => size(bytes),
        };
        let waiting = match project.waiting {
            0 => String::new(),
            count => style.paint(count, Tone::Warning),
        };
        let line = format!(
            "  {:<name$}  {:<client$}  {:>8}  {:>8}  {}  {}",
            cut(&project.name),
            cut(&project.client),
            size(project.copy),
            build,
            synced,
            waiting
        );
        output.push_str(line.trim_end());
        output.push('\n');
    }
    let total: u64 = projects.iter().map(|p| p.copy + p.build).sum();
    output.push_str(&format!(
        "\n  {}, {}\n",
        presentation::plural(projects.len(), "project"),
        size(total)
    ));
    if projects.iter().any(|p| p.build > 0) {
        output.push_str(&format!(
            "  {}\n",
            style.dim(format!(
                "Free build output with {}",
                style.paint("ally projects clean <project>", Tone::Info)
            ))
        ));
    }
    output
}

/// Bytes in the largest unit that keeps the number short: `640 KiB`, `18 MiB`, `1.4 GiB`.
fn size(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    let bytes = bytes as f64;
    match bytes {
        b if b < KIB * KIB => format!("{:.0} KiB", (b / KIB).ceil()),
        b if b < KIB * KIB * KIB => format!("{:.0} MiB", b / (KIB * KIB)),
        b => format!("{:.1} GiB", b / (KIB * KIB * KIB)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(name: &str, client: &str, id: &str) -> ProjectSummary {
        ProjectSummary {
            id: id.into(),
            name: name.into(),
            client: client.into(),
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
        assert_eq!(size(18 * 1024 * 1024 + 300_000), "18 MiB");
        assert_eq!(size(1536 * 1024 * 1024), "1.5 GiB");
    }

    #[test]
    fn the_table_lines_up_and_marks_only_what_needs_attention() {
        let mut api = summary("api", "MacBook-Pro-3f9c2a", "bbbb2222");
        api.waiting = 4;
        api.build = 0;
        let mut notes = summary("notes", "archlaptop-a1b2c3", "cccc3333");
        notes.synced = None;
        let projects = [summary("app", "MacBook-Pro-3f9c2a", "aaaa1111"), api, notes];
        let text = render("archbox", &projects, 10_000, Style::new(false));
        assert!(text.starts_with("Projects on archbox\n\n"), "{text}");
        assert!(text.contains(
            "  PROJECT  CLIENT                  COPY     BUILD  LAST SYNC  EDITS WAITING\n"
        ));
        assert!(text.contains("  app      MacBook-Pro-3f9c2a     2 MiB   1.5 GiB  2h ago\n"));
        assert!(text.contains("  api      MacBook-Pro-3f9c2a     2 MiB            2h ago     4\n"));
        assert!(text.contains("  notes    archlaptop-a1b2c3      2 MiB   1.5 GiB  never\n"));
        assert!(text.contains("\n  3 projects, 3.0 GiB\n"));
        assert!(text.contains("Free build output with ally projects clean <project>"));
        assert!(!text.contains('\x1b'));
    }

    #[test]
    fn long_names_are_cut_with_an_ellipsis() {
        let long = "a".repeat(50);
        let text = render(
            "archbox",
            &[summary(&long, "laptop", "aaaa1111")],
            10_000,
            Style::new(false),
        );
        assert!(
            text.contains(&format!("  {}…  laptop", "a".repeat(39))),
            "{text}"
        );
    }

    #[test]
    fn an_empty_agent_says_how_to_add_a_project() {
        let text = render("archbox", &[], 0, Style::new(false));
        assert!(text.contains("None yet. ally sync in a project copies it"));
    }

    #[test]
    fn a_project_is_chosen_by_name_or_id_and_a_shared_name_asks_for_the_id() {
        let projects = [
            summary("app", "MacBook-Pro-3f9c2a", "aaaa1111"),
            summary("app", "archlaptop-a1b2c3", "bbbb2222"),
            summary("api", "MacBook-Pro-3f9c2a", "cccc3333"),
        ];
        let target = "archbox";
        assert_eq!(
            choose(&projects, Some("api"), target).unwrap().id,
            "cccc3333"
        );
        assert_eq!(
            choose(&projects, Some("bbbb"), target).unwrap().id,
            "bbbb2222"
        );
        let shared = choose(&projects, Some("app"), target)
            .unwrap_err()
            .to_string();
        assert!(
            shared.contains("2 projects are named app on archbox"),
            "{shared}"
        );
        assert!(
            shared.contains("aaaa1111 from MacBook-Pro-3f9c2a"),
            "{shared}"
        );
        let missing = choose(&projects, Some("web"), target)
            .unwrap_err()
            .to_string();
        assert!(missing.starts_with("No project named web on archbox"));
        assert!(choose(&projects, Some("cc"), target).is_err());
    }

    #[test]
    fn removing_warns_about_edits_not_brought_back() {
        let mut project = summary("api", "MacBook-Pro-3f9c2a", "aaaa1111");
        project.waiting = 4;
        let lines = listing(&project, Style::new(false)).concat();
        assert!(lines.contains("2 MiB, including 4 edits not brought back"));
        assert!(lines.contains("run ally sync --pull in the project first"));
        project.waiting = 0;
        assert!(
            !listing(&project, Style::new(false))
                .concat()
                .contains("sync --pull")
        );
    }
}
