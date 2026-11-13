//! Project discovery from Rust, Node, Python, and Ally marker files.

use std::path::{Path, PathBuf};

/// A supported stack with rules for local Agent build output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stack {
    Rust,
    Node,
    Python,
}

/// Project markers. An ally.toml or a container file also marks a project without a
/// known stack, so a folder holding only a compose file still syncs before a run.
const MARKERS: &[(&str, Option<Stack>)] = &[
    ("ally.toml", None),
    ("compose.yaml", None),
    ("compose.yml", None),
    ("docker-compose.yml", None),
    ("docker-compose.yaml", None),
    ("Dockerfile", None),
    ("Cargo.toml", Some(Stack::Rust)),
    ("package.json", Some(Stack::Node)),
    ("pyproject.toml", Some(Stack::Python)),
    ("requirements.txt", Some(Stack::Python)),
];

/// A project root and its detected stacks. Mixed projects need every applicable split.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub root: PathBuf,
    pub stacks: Vec<Stack>,
}

impl Project {
    /// Whether this project needs a given split.
    pub fn uses(&self, stack: Stack) -> bool {
        self.stacks.contains(&stack)
    }
}

/// Every stack with a marker file sitting in this directory, in a stable order and
/// with no repeats. Python has two markers and only wants counting once.
pub fn stacks_in(dir: &Path) -> Vec<Stack> {
    let mut found = Vec::new();

    for (marker, stack) in MARKERS {
        let Some(stack) = stack else { continue };

        if dir.join(marker).exists() && !found.contains(stack) {
            found.push(*stack);
        }
    }

    found
}

fn is_project_root(dir: &Path) -> bool {
    MARKERS.iter().any(|(marker, _)| dir.join(marker).exists())
}

/// Find the nearest project marker at or above start.
/// A workspace member is selected before its parent workspace.
pub fn find(start: &Path) -> Option<Project> {
    let mut dir = start;

    loop {
        if is_project_root(dir) {
            return Some(Project {
                root: dir.to_path_buf(),
                stacks: stacks_in(dir),
            });
        }

        dir = dir.parent()?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::testing::TempDir;

    /// Each python marker alone is enough, both together still mean one stack so the split
    /// is not applied twice, and a project with two languages reports both, or `target`
    /// would stay in the source copy.
    #[test]
    fn markers_name_each_stack_once() {
        let cases: [(&[&str], &[Stack]); 7] = [
            (&["Cargo.toml"], &[Stack::Rust]),
            (&["package.json"], &[Stack::Node]),
            (&["pyproject.toml"], &[Stack::Python]),
            (&["requirements.txt"], &[Stack::Python]),
            (&["pyproject.toml", "requirements.txt"], &[Stack::Python]),
            (&["Cargo.toml", "package.json"], &[Stack::Rust, Stack::Node]),
            (&["README.md", "src/Cargo.toml"], &[]),
        ];
        for (files, expected) in cases {
            let dir = TempDir::new("stack");
            for file in files {
                dir.write(file, "");
            }
            let mut stacks = stacks_in(dir.path());
            stacks.sort_by_key(|stack| format!("{stack:?}"));
            let mut expected = expected.to_vec();
            expected.sort_by_key(|stack| format!("{stack:?}"));
            assert_eq!(stacks, expected, "{files:?}");
        }
    }

    #[test]
    fn an_empty_directory_is_not_a_project() {
        let dir = TempDir::new("stack-empty");

        assert!(stacks_in(dir.path()).is_empty());
        assert_eq!(find(dir.path()), None);
    }

    #[test]
    fn the_root_is_found_from_a_directory_below_it() {
        let dir = TempDir::new("stack-nested");
        dir.write("Cargo.toml", "");
        let deep = dir.path().join("src/commands/inner");
        std::fs::create_dir_all(&deep).unwrap();

        let project = find(&deep).expect("should find the project above");

        assert_eq!(project.root, dir.path());
        assert!(project.uses(Stack::Rust));
    }

    /// A workspace member is the project. You typed the command in there, so that
    /// is the thing you meant.
    #[test]
    fn the_nearest_root_wins() {
        let dir = TempDir::new("stack-workspace");
        dir.write("Cargo.toml", "");
        dir.write("crates/inner/Cargo.toml", "");

        let project = find(&dir.path().join("crates/inner")).expect("should find inner");

        assert_eq!(project.root, dir.path().join("crates/inner"));
    }

    /// An ally.toml or a container file marks a project even when nothing else does, so
    /// an override can name a stack ally would not have detected.
    #[test]
    fn a_settings_or_container_file_alone_marks_a_project() {
        for marker in [
            "ally.toml",
            "compose.yaml",
            "compose.yml",
            "docker-compose.yml",
            "docker-compose.yaml",
            "Dockerfile",
        ] {
            let dir = TempDir::new("stack-marker");
            dir.write(marker, "");

            let project = find(dir.path()).expect("the file should mark a root");

            assert_eq!(project.root, dir.path(), "{marker}");
            assert!(project.stacks.is_empty(), "{marker}");
        }
    }
}
