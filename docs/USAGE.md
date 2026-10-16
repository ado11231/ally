# Usage

* Every command and option. Errors and fixes are in [TROUBLESHOOTING.md](TROUBLESHOOTING.md).

## Contents

1. [Words Used Here](#words-used-here)
2. [Install](#install)
3. [Set Up](#set-up)
4. [Run Work](#run-work)
5. [Keep Files In Step](#keep-files-in-step)
6. [Environment Files](#environment-files)
7. [Check On The Agent](#check-on-the-agent)
8. [Manage Jobs](#manage-jobs)
9. [Manage Projects](#manage-projects)
10. [The Menu Bar App](#the-menu-bar-app)
11. [More Than One Agent](#more-than-one-agent)
12. [Project Settings](#project-settings)
13. [What Slingshot Copies](#what-slingshot-copies)
14. [Remove Slingshot](#remove-slingshot)

## Words Used Here

| Word | Means |
| --- | --- |
| **Client** | The machine you work on. |
| **Agent** | The powerful machine that does the work. |
| **Project** | A folder in a Git repository, or with a `Cargo.toml`, `package.json`, `pyproject.toml`, `requirements.txt`, or `slingshot.toml`. |
| **Job** | One run or session on the Agent. |

## Install

* Steps for each system are in the [README](../README.md#setup).
* To update, run the install command again on both machines. Both must run the same version.
* If the Agent starts by itself, restart it after updating: `systemctl --user restart slingshot.service` on Linux, or log out and in on macOS.
* `sling` is a short name for `slingshot`.

## Set Up

### Start The Agent

```sh
slingshot start
```

* Checks the machine, then prints a pairing code and keeps the machine awake.
* The first time, offers to [start by itself](#start-by-itself) after a restart.
* A code works once, expires after 10 minutes, and burns after 3 wrong tries. It never crosses the network. Press Enter for a new one.
* Once a Client is linked, `start` lists linked Clients. Press Enter for a code.
* If one is already running on the account, `start` shows it and offers a code. Ctrl C only leaves.

| Option | Effect |
| --- | --- |
| `--name <name>` | Name shown for the Agent. Default is its hostname. |
| `--port <port>` | Pairing port. Default `7433`, or the next free one. |
| `--boot` | Start by itself, without asking. |
| `--remove` | Stop starting by itself, and stop the service. |

### Start By Itself

| Agent | File | Starts | Logs |
| --- | --- | --- | --- |
| Linux | `~/.config/systemd/user/slingshot.service`, plus `loginctl enable-linger` | At boot | `journalctl --user -u slingshot.service` |
| macOS | `~/Library/LaunchAgents/dev.slingshot.agent.plist` | At login | `~/Library/Application Support/slingshot/agent/daemon.log` |

* The service prints no code. Run `slingshot start` and press Enter for one. Ctrl C leaves the service running.
* The service keeps the PATH of the terminal it was set up from.
* If linger is refused, it starts at login, and Slingshot prints the `sudo` fix.

### Link The Client

```sh
slingshot link 192.168.1.9:7433:K7QW9ZR2
```

* Both machines need the same network or tailnet for this step only. A tailnet is a private network made by a VPN such as Tailscale.
* Each Client links under its own name, such as `MacBook-Pro-3f9c2a`. Linking again keeps it.
* `--name <name>` saves the Agent under your own name.

### Set Up Tools On The Agent

```sh
slingshot tools
```

* `link` runs this at the end. It offers each tool the Client has and the Agent lacks, plus what the project needs.

| Tool | Installed With |
| --- | --- |
| Git, Node and npm, Python | The package manager |
| Docker | The package manager, then starts it and adds you to the `docker` group |
| Rust | `rustup` |
| Claude Code | Its official installer |
| Codex | `npm` |
| Ollama | Its official installer, which adds a system service. Offered when the Agent has a GPU. |
| OpenCode | Its official installer |

* Lists every command, then asks once. `sudo` commands run in your terminal so you can type your password.
* Then signs in to Claude Code and Codex. Accounts are never copied from the Client.
* For Codex, open the printed link on the Client. Slingshot forwards port 1455 for it.
* Without `sudo`, it installs what it can and lists the rest under "Ask an admin to run".
* `~/.local/bin`, `~/.cargo/bin`, and `~/.opencode/bin` are on the PATH of everything Slingshot starts.
* Versions are not matched. `.nvmrc` or `rust-toolchain.toml` still decide them.

## Run Work

### Run A Single Command

```sh
slingshot run cargo build
```

* Copies changed files, then runs the command in the same folder of `~/Slingshot/<project>` on the Agent.
* The first line says where it runs: `▶ Running on archbox via local network · app · target → Agent disk`.
* Typing, colors, and Ctrl C work as usual. The last line shows time and exit code.
* Slingshot's options go before `run`: `slingshot --agent archbox run cargo test`.
* Outside a project, nothing is copied and it runs in the Agent's home folder.
* A run stops if the connection drops. Use a session for long work.

### Open A Session

```sh
slingshot attach [path]
```

* In a project, opens a session in `~/Slingshot/<project>`. Anywhere else, in the Agent's home folder.
* Every `attach` in a project copies your edits first. Leaving brings the Agent's edits back.
* A conflict is a warning, and nothing is overwritten.
* A lost connection brings nothing back. Run `slingshot sync --pull` when done.
* The session keeps running when you leave. Run `slingshot attach` to return.

| Inside A Session | How |
| --- | --- |
| Scroll | Trackpad or mouse. `q` returns to the bottom. |
| Copy | Drag to select. |
| Clear all output | Ctrl B, then K |
| Leave it running | Ctrl B, then D |
| End it | `exit`, or `slingshot stop <id>` |

## Keep Files In Step

* `run` and `attach` sync for you. `sync` does it alone.

| Command | Effect |
| --- | --- |
| `slingshot sync` | Copies your changes to the Agent. |
| `slingshot sync --pull` | Copies the Agent's changes back. |
| `--check` | Shows what would change. |
| `--overwrite` | Keeps this side in a conflict, with a backup, after asking. |

* Edits made on one machine only are kept.
* A file changed on both machines stops the sync. Use `--overwrite`, or fix it by hand.
* Sync works while a session is open.

## Environment Files

* `.env` files are never synced. Add them on purpose. They are stored outside the project copy.

```sh
slingshot env add .env
slingshot env list
slingshot env remove .env
```

| Option For `env add` | Effect |
| --- | --- |
| `--target <path>` | Where it appears, such as `api/.env.local`. |
| `--replace` | Replace a stored file. |

* The target must be an environment file name. Each file can be up to 1 MiB.

## Check On The Agent

| Command | Shows |
| --- | --- |
| `slingshot health` | System, CPU, RAM, GPU, VRAM, disk, and project space. |
| `slingshot health --watch` | The same every two seconds, with jobs. Q leaves. |

## Manage Jobs

| Command | Effect |
| --- | --- |
| `slingshot ps` | Lists running jobs. |
| `slingshot ps --all` | Adds the last 100 finished jobs. |
| `slingshot stop <id>` | Stops a job, forced after five seconds. A short ID works. |

* Finished runs show `Completed`, `Failed`, `Stopped`, `Interrupted`, or `Lost connection`.

## Manage Projects

| Command | Effect |
| --- | --- |
| `slingshot projects` | Lists each project on the Agent: its Client, copy size, build output size, last sync, and edits waiting. |
| `slingshot projects clean [project]` | Removes the project's build output, including a Python `.venv`. The next build or install makes it again. |
| `slingshot projects remove [project]` | Deletes the project from the Agent, after listing it and asking. Default is No. |

* Name a project as the list shows it, or leave it out for the current project.
* If two Clients have a project with the same name, use its ID from the error message.
* Edits waiting are files changed on the Agent that `slingshot sync --pull` has not brought back. Pull them before `remove` to keep them.
* `clean` and `remove` wait until the project has no run or session.
* The project on the Client is never touched.

## The Menu Bar App

| Command | Effect |
| --- | --- |
| `slingshot menubar` | Builds the app into `~/Applications`. Needs the Xcode command line tools. |
| `slingshot menubar --remove` | Lists, asks, then deletes the app and its settings. |

* Shows the Agent, connection, CPU, RAM, GPU, VRAM, and project space.
* When the Agent is unreachable, shows why and the fix.
* Notifies when a run of 10 seconds or more ends, a job is cut off by a lost connection or restart, the Agent goes offline or returns, or memory, disk, or GPU heat needs attention.
* No notifications? Turn them on in System Settings, Notifications, Slingshot.

## More Than One Agent

* With one Agent, it is the default. With several, pass `--agent archbox`, or set `default = "archbox"` in Slingshot's `config.toml`.

## Project Settings

* Optional `slingshot.toml` in the project's top folder. Patterns work like `.gitignore`, and cannot start with `!`.

```toml
[sync]
exclude = ["data/", "*.mp4"]
```

## What Slingshot Copies

* Source files only. Never `.gitignore` matches, `.git`, build output, environment files, or `sync.exclude` patterns.

| Project | Build Output |
| --- | --- |
| Rust | `target` goes to Agent storage. |
| Node | `node_modules` links to Agent storage. |
| Python | `.venv` stays in the Agent's copy. |

## Remove Slingshot

| Command | Run On | Effect |
| --- | --- | --- |
| `slingshot unlink` | Client | Removes this Client's key and environment files from the Agent. |
| `slingshot uninstall` | Either | Lists everything Slingshot added, asks once, then removes it. Default is No. |
| `cargo uninstall slingshot-cli` | Either | Removes the program, after `uninstall`. |

* On the Agent, `uninstall` removes the service, jobs, linked Client keys, and project copies. Your own keys and files stay.
* Run `slingshot sync --pull` first to keep edits made on the Agent.
* On Linux, linger stays on. Turn it off with `sudo loginctl disable-linger $USER`.
* To only stop the Agent, run `slingshot start --remove`.
