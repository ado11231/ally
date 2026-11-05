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
9. [Free Space On The Agent](#free-space-on-the-agent)
10. [The Menu Bar App](#the-menu-bar-app)
11. [More Than One Agent](#more-than-one-agent)
12. [Project Settings](#project-settings)
13. [What Ally Copies](#what-ally-copies)
14. [Shell Completions](#shell-completions)
15. [Remove Ally](#remove-ally)

## Words Used Here

| Word | Means |
| --- | --- |
| **Client** | The machine you work on. |
| **Agent** | The powerful machine that does the work. |
| **Project** | A folder in a Git repository, or with a `Cargo.toml`, `package.json`, `pyproject.toml`, `requirements.txt`, `compose.yaml`, `docker-compose.yml`, `Dockerfile`, or `ally.toml`. |
| **Job** | One run or session on the Agent. |

## Install

* Steps for each system are in the [README](../README.md#setup).
* To update, run the install command again on both machines. Both must run the same version.
* If the Agent starts by itself, restart it after updating: `systemctl --user restart ally.service` on Linux, or log out and in on macOS.

## Set Up

### Start The Agent

```sh
ally start
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
| `--boot` | Start by itself, without asking. When it already does, set it up again, which retries a step that failed, such as the sleep rule. |
| `--remove` | Stop starting by itself, and stop the service. |

### Start By Itself

| Agent | File | Starts | Logs |
| --- | --- | --- | --- |
| Linux | `~/.config/systemd/user/ally.service`, plus `loginctl enable-linger` | At boot | `journalctl --user -u ally.service` |
| macOS | `~/Library/LaunchAgents/dev.ally.agent.plist` | At login | `~/Library/Application Support/ally/agent/daemon.log` |

* The service prints no code. Run `ally start` and press Enter for one. Ctrl C leaves the service running.
* The service keeps the PATH of the terminal it was set up from.
* If linger is refused, it starts at login, and Ally prints the `sudo` fix.
* On a Linux Agent that can sleep, it also writes `/etc/polkit-1/rules.d/50-ally.rules` with `sudo`. The rule lets your account keep the machine awake with nobody logged in. `--remove` deletes it.

### Link The Client

```sh
ally link 192.168.1.9:7433:K7QW9ZR2
```

* Both machines need the same network or tailnet for this step only. A tailnet is a private network made by a VPN such as Tailscale.
* Each Client links under its own name, such as `MacBook-Pro-3f9c2a`. Linking again keeps it.
* `--name <name>` saves the Agent under your own name.

### Tools On The Agent

* Commands run with the Agent's own tools. Install what you need there, such as inside `ally attach`.
* `~/.local/bin`, `~/.cargo/bin`, and `~/.opencode/bin` are on the PATH of everything Ally starts.
* Runs use the PATH of the Agent account's login shell, such as Homebrew's folders from `~/.zprofile`. A tool set up only for interactive shells, in `~/.zshrc` or `~/.bashrc`, is found in sessions but not in runs.
* Versions are not matched. `.nvmrc` or `rust-toolchain.toml` still decide them.

## Run Work

### Run A Single Command

```sh
ally run cargo build
```

* Copies changed files, then runs the command in the same folder of `~/Ally/<project>` on the Agent.
* The first line says where it runs: `▶ Running on archbox via local network · app · target → Agent disk`.
* Typing, colors, and Ctrl C work as usual. The last line shows time and exit code.
* Ally's options go before `run`: `ally --agent archbox run cargo test`.
* Outside a project, nothing is copied and it runs in the Agent's home folder. The first line ends with `· ~ · not a project, nothing synced`.
* A run stops if the connection drops. Use a session for long work.

### Open A Session

```sh
ally attach [path]
```

* In a project, opens a session in `~/Ally/<project>`. Anywhere else, in the Agent's home folder.
* A cyan bar at the bottom names the Agent for as long as you are in the session.
* Leaving says which machine you are back on, such as `✓ Back on laptop. The session on archbox keeps running`.
* Every `attach` in a project copies your edits first. Leaving brings the Agent's edits back.
* A conflict is a warning, and nothing is overwritten.
* A lost connection brings nothing back. Run `ally sync --pull` when done.
* The session keeps running when you leave. Run `ally attach` to return.

| Inside A Session | How |
| --- | --- |
| Scroll | Trackpad or mouse. `q` returns to the bottom. |
| Copy | Drag to select. |
| Clear all output | Ctrl B, then K |
| Leave it running | Ctrl B, then D |
| End it | `exit`, or `ally stop <id>` |

### Open A Server In Your Browser

* While `ally run` or `ally attach` is connected, a server started on the Agent opens at the same port on the Client: `▶ Forwarding archbox:5173 to localhost:5173 · node`.
* This covers every port that Ally's work listens on, and every port a Docker container publishes.
* If the port is in use on the Client, the next free one is used, and a second line says so.
* In a session, the bar lists the ports, and a short message shows each new one.
* Forwards answer only on the Client itself, and end when the port closes or the command ends.
* With several Ally commands open, one of them forwards each port and prints its line.
* Programs that Ally did not start open only on request, with `ally forward`.

| Command | Effect |
| --- | --- |
| `ally forward <port>` | Opens one Agent port at `localhost` until Ctrl C, for a server left running after leaving its session, or one Ally did not start. |

### Choose The Path

```sh
ally --via iroh run cargo build
```

* Ally tries the local network, then the tailnet, then iroh, and takes the first that answers.
* `--via local`, `--via tailnet`, or `--via iroh` takes that path only, such as to test iroh while the local network works. Syncs take it too.
* If the chosen path does not answer, Ally stops and says so instead of trying another.

## Keep Files In Step

* `run` and `attach` sync for you. `sync` does it alone.

| Command | Effect |
| --- | --- |
| `ally sync` | Copies your changes to the Agent. |
| `ally sync --pull` | Copies the Agent's changes back. |
| `--overwrite` | Keeps this side in a conflict, with a backup, after asking. |

* Edits made on one machine only are kept.
* A file changed on both machines stops the sync. Use `--overwrite`, or fix it by hand.
* Sync works while a session is open.

## Environment Files

* `.env`, `.env.*`, `*.env`, and `.envrc` files are never synced.
* Make the ones the Agent needs there, such as inside `ally attach`, in `~/Ally/<project>`.

## Check On The Agent

| Command | Shows |
| --- | --- |
| `ally health` | System, CPU, RAM, GPU, VRAM, disk, and project space. |
| `ally health --watch` | The same every two seconds, with jobs. Q leaves. |

## Manage Jobs

| Command | Effect |
| --- | --- |
| `ally ps` | Lists running jobs. |
| `ally ps --all` | Adds the last 100 finished jobs. |
| `ally stop <id>` | Stops a job, forced after five seconds, then the containers its `docker run` commands started. A short ID works. |

* A container another job could also have started is left running, with the `docker stop` command to stop it.
* Finished runs show `Completed`, `Failed`, `Stopped`, `Interrupted`, or `Lost connection`.

## Free Space On The Agent

| Command | Effect |
| --- | --- |
| `ally storage` | Lists each project copy on the Agent: source size, build output size, and last sync. |
| `ally storage clean [project]` | Deletes a copy's build output, such as `target/` or `node_modules/`. The next build makes it again. |
| `ally storage remove [project]` | Deletes a whole copy, after listing it and asking. The default answer is No. |

* A copy is named by its folder in `~/Ally` on the Agent, such as `webtest` or `webtest-3d651d51`.
* Without a name, `clean` and `remove` use the current project.
* `unused` marks a copy no linked machine uses anymore, such as one left by a machine that unlinked.
* `remove` names anything that exists only on the Agent first, such as `.env` files and edits never brought back.
* Both refuse while a run or session uses the copy. Stop it with `ally stop <id>` first.

## The Menu Bar App

| Command | Effect |
| --- | --- |
| `ally menubar` | Builds the app into `~/Applications`. Needs the Xcode command line tools. |
| `ally menubar --remove` | Lists, asks, then deletes the app and its settings. |

* Shows the Agent, connection, CPU, RAM, GPU, VRAM, and project space.
* Lists forwarded ports under Ports. A click opens one in the browser.
* When the Agent is unreachable, shows why and the fix.
* Notifies when a run of 10 seconds or more ends, a job is cut off by a lost connection or restart, the Agent goes offline or returns, memory, disk, or GPU heat needs attention, or a server starts in a run or session. A click on a ready server opens it.
* No notifications? Turn them on in System Settings, Notifications, Ally.

## More Than One Agent

* With one Agent, it is the default. With several, pass `--agent archbox`, or set `default = "archbox"` in Ally's `config.toml`.

### An Agent That Stores Personal Files

* On a machine that keeps photos, backups, or other personal files, link a normal account without `sudo`, not your own.
* Why: linking lets the Client run anything as that account over ssh. The account is the limit of what Ally can reach. A normal account cannot change the system, and reads only files other accounts may read.
* Keep personal folders private to their owner, such as with `chmod 700 ~/Photos`. Some systems, such as Ubuntu, let every account read home folders.
* A storage machine rarely needs new tools, so an account without `sudo` loses little.

1. On the Agent, make a normal account the usual way for its system. On Linux, `sudo useradd -m -s /bin/bash allywork`, then `sudo passwd allywork`, and leave it out of the `sudo` and `wheel` groups. On macOS, add a Standard user in System Settings, Users And Groups.
2. Log in as that account, install Ally, and run `ally start`.
3. On the Client, link with a name of your own: `ally link <code> --name photos`.
4. Reach it with `--agent photos`, such as `ally --agent photos health`.

## Project Settings

* Optional `ally.toml` in the project's top folder. Patterns work like `.gitignore`, and cannot start with `!`.

```toml
[sync]
exclude = ["data/", "*.mp4"]
```

## What Ally Copies

* Source files only. Never `.gitignore` matches, `.git`, build output, environment files, or `sync.exclude` patterns.

| Project | Build Output |
| --- | --- |
| Rust | `target` goes to Agent storage. |
| Node | `node_modules` stays in the Agent's copy. |
| Python | `.venv` stays in the Agent's copy. |

## Shell Completions

* Tab completes Ally's commands and options. Nothing is offered among a run's own arguments.
* Add the line for your shell to its startup file, then open a new terminal:

| Shell | Line |
| --- | --- |
| zsh, in `~/.zshrc` after `compinit` | `source <(ally completions zsh)` |
| bash, in `~/.bashrc` | `source <(ally completions bash)` |
| fish | `ally completions fish > ~/.config/fish/completions/ally.fish`, once |
| elvish, in `~/.config/elvish/rc.elv` | `eval (ally completions elvish \| slurp)` |

## Remove Ally

| Command | Run On | Effect |
| --- | --- | --- |
| `ally unlink` | Client | Removes this Client's key from the Agent, and any environment files older versions of Ally stored. Project copies stay, and linking again uses them. |
| `ally uninstall` | Either | Lists everything Ally added, asks once, then removes it. Default is No. |
| `cargo uninstall ally-cli` | Either | Removes the program, after `uninstall`. |

* On the Agent, `uninstall` removes the service, jobs, linked Client keys, and project copies. Your own keys and files stay.
* Run `ally sync --pull` first to keep edits made on the Agent.
* On Linux, linger stays on. Turn it off with `sudo loginctl disable-linger $USER`.
* To only stop the Agent, run `ally start --remove`.
