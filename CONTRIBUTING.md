# Contributing

* How to set up Slingshot, make a change, and get it merged.

## Contents

1. [Before You Start](#before-you-start)
2. [Setting Up](#setting-up)
3. [Building And Testing](#building-and-testing)
4. [Testing On Two Machines](#testing-on-two-machines)
5. [Making A Change](#making-a-change)
6. [Using AI Tools](#using-ai-tools)
7. [Code Standards](#code-standards)
8. [Output Standards](#output-standards)
9. [Writing Documentation](#writing-documentation)
10. [Commits And Pull Requests](#commits-and-pull-requests)
11. [Out Of Scope](#out-of-scope)
12. [Reporting A Problem](#reporting-a-problem)

## Before You Start

* Read [ARCHITECTURE.md](docs/ARCHITECTURE.md), [USAGE.md](docs/USAGE.md), and [ROADMAP.md](docs/ROADMAP.md).
* For anything bigger than a small fix, open an issue first, so the approach is agreed before you write code.

## Setting Up

| Tool | Machine | Purpose |
| --- | --- | --- |
| Rust, latest stable | Both | Builds Slingshot. |
| `rsync` | Both | Copies project files. |
| An ssh server | Agent | Carries all work and requests. |
| `tmux` | Agent | Keeps sessions running. |
| Xcode command line tools | Mac Client | Builds the menu bar app. |

```sh
git clone https://github.com/ado11231/ss.git slingshot
cd slingshot
cargo install --path crates/slingshot-cli
slingshot menubar
```

* `cargo install --path` installs your build as `slingshot`. `slingshot menubar` builds the app on a Mac, and rebuilds it when its source changed.

## Building And Testing

* All three must pass with no warnings before you push:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets
cargo test --workspace
```

* After changing the menu bar app, also run `swift build -c release --package-path crates/slingshot-cli/menubar`.
* CI runs the same checks on Linux and macOS for every pull request.
* Unit tests sit in a `tests` module at the bottom of the file they test. Tests of the finished program are in `crates/slingshot-cli/tests/`.
* Every rule or decision needs a test, such as which files are source, what a sync copies, when a notification is sent, or how an error is worded. Code that only starts another program, such as `ssh`, is tested on real machines.

## Testing On Two Machines

* Test changes to running, syncing, sessions, or connections on two real machines:

1. `slingshot start` on the Agent, and `slingshot link <code>` on the Client.
2. Use the changed feature, such as `slingshot run cargo build`, `slingshot sync`, or `slingshot attach`.
3. Test failures: stop `slingshot start`, drop the network, or restart the Agent.

* List exactly what you ran and what happened in the pull request. Never call something working unless you ran it.

## Making A Change

1. **Find the place** in the [file reference](docs/ARCHITECTURE.md#file-reference). Shared rules go in `slingshot-core`, Agent only code in `slingshot-agent`, and commands and Client code in `slingshot-cli`.
2. **Keep it focused.** The smallest change that solves the problem. Unrelated improvements go in another pull request.
3. **Match what is there:** naming, structure, and comment style.
4. **Test it,** and run the changed command at least once.
5. **Update the docs:**

| If You | Update |
| --- | --- |
| Add or move a file | The [file reference](docs/ARCHITECTURE.md#file-reference) |
| Change a command or option | [USAGE.md](docs/USAGE.md) |
| Add or change an error message | [TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md) |
| Change what works, or test something new | [ROADMAP.md](docs/ROADMAP.md) |

## Using AI Tools

* AI coding tools are welcome. **You are the author,** and responsible for every line.
* **Understand it.** You must be able to explain each change. If you cannot, the pull request will be closed.
* **Verify it.** Run the checks and the feature, and read the output. AI tools often call functions that do not exist, miss edge cases, or change more than asked.
* **Keep it small,** and cut unused code and comments that repeat the code.
* AI agents working in this repository directly must also follow [AGENTS.md](AGENTS.md).

## Code Standards

* `slingshot-core` uses neither other crate. `slingshot-agent` uses `slingshot-core`. `slingshot-cli` uses both.
* Wrap existing tools such as `ssh`, `rsync`, `tmux`, `docker`, and `iroh`. Never write your own.
* Say Client and Agent, never Mac and Linux. Only the menu bar app may be macOS specific.
* No fixed paths such as `/Users/...`. Use the `directories` crate.
* Never assume a package manager. Detect, print the install command, and ask once before running anything.
* Errors use `anyhow`, and every visible error says how to fix it. A failing user command is not a Slingshot error; pass its exit code through.
* Use `tokio` for async, `tracing` for logs, and `clap` for arguments. No `unwrap` outside tests.
* Quote each argument sent to the Agent on its own. Never stop a process by ID alone; check its start time.
* Changing a control message raises `control::VERSION`. Changing the menu bar line format raises `watch::event::VERSION`, and `Sources/Model.swift` changes with `watch/event.rs`.
* Swift only displays and notifies. Thresholds and rules stay in `crates/slingshot-cli/src/watch/`.
* Comments go above an item, only when its purpose is not obvious, and say why. No comments in function bodies, commented out code, or `TODO` notes.
* Explain every new crate in the pull request, and use its latest version.

## Output Standards

* Use `slingshot-core/src/presentation.rs` and `step.rs`. Never raw color codes.
* Slow work shows a spinner, then a line such as `✓ Synced 3 files  0.4s`.
* Symbols: `✓` success, `!` warning, `✗` failure, `▶` remote work. Every remote command says where it runs, such as `▶ Running on archbox via tailnet`.
* Color marks only what needs attention. Headings are bold, and details dimmed.
* Sentence case. CPU, RAM, GPU, and VRAM are always capitals.
* Every failed check prints the exact command that fixes it.

## Writing Documentation

* Plain words and short sentences. Explain a technical term the first time.
* Bullets, numbered steps, and tables, not paragraphs.
* Capitalize every word in titles.
* No em dashes, and no hyphens in ordinary text. Write "long lived", not the joined form. Hyphens are fine in code.
* Describe only what is true today.
* Cut anything that does not help the reader.

## Commits And Pull Requests

* Work on a branch, never on `master`.
* Commit messages use `type: summary`, with an optional body saying why. Types: `feat`, `fix`, `refactor` (same behavior), `docs`, and `test`.
* Each commit should make sense alone, build, and pass its tests.
* A pull request says what changed and why, how you tested it with the exact commands, whether two machines were used, and what is untested. Its template has a checklist.
* It is ready for review when all checks pass and the docs are updated.

## Out Of Scope

| Idea | Why It Does Not Fit |
| --- | --- |
| Sharing RAM over the network | A network is about a hundred thousand times slower than RAM. |
| Remote desktop or screen streaming | Sunshine and Moonlight already do this. |
| Copying or mounting build output | `target`, `node_modules`, virtual environments, and caches stay on the Agent. Copying them makes Slingshot too slow. |
| Syncing environment files as source | `.env`, `.env.*`, `*.env`, and `.envrc` often hold secrets. |
| Requiring an account or VPN | A VPN is used when present, never required. iroh needs no account and no open router port. |
| Custom `ssh`, `rsync`, `tmux`, or relay servers | Slingshot wraps these tools and never replaces them. |

## Reporting A Problem

1. Check [TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md) first.
2. Open an issue with the Problem template. It asks for the exact command, what you expected, the full output, each machine's system and how they connect, and `slingshot health` output if the Agent answers.
