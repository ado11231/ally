# Troubleshooting

* Search this page for the words on your screen. Each row shows the exact message, what it means, and the fix.

## Contents

1. [Starting The Agent](#starting-the-agent)
2. [Starting By Itself](#starting-by-itself)
3. [Linking](#linking)
4. [Setting Up Tools](#setting-up-tools)
5. [Reaching The Agent](#reaching-the-agent)
6. [Running Commands](#running-commands)
7. [Syncing Files](#syncing-files)
8. [Jobs And GPU](#jobs-and-gpu)
9. [Projects](#projects)
10. [The Menu Bar App](#the-menu-bar-app)
11. [Removing Slingshot](#removing-slingshot)
12. [Getting Help](#getting-help)

## Starting The Agent

| Message | Meaning | Fix |
| --- | --- | --- |
| `SSH server not running` | All work goes over ssh, so the Agent needs an ssh server. The Client does not. | macOS: turn on System Settings, General, Sharing, Remote Login. Linux: `sudo systemctl enable --now sshd`. |
| `Tool not installed: rsync` or `Tool not installed: tmux` | The Agent needs `rsync` to copy files and `tmux` to keep sessions running. | Run the install command printed, then `slingshot start` again. |
| `Docker is installed, but this account cannot use it` | A warning. The account is not in the `docker` group, so Docker commands fail with "permission denied". | Run the command printed, such as `sudo usermod -aG docker ado`, then restart the machine. Running sessions and the service keep their old groups until then. |
| `Another slingshot start is already running for this account` | Two `slingshot start` commands began at once. | Run `slingshot start` again. It shows the one running. |
| `A different version of Slingshot is already running for this account` | The running `slingshot start` is from another version, usually from before an update. | Ctrl C where it runs, then `slingshot start` again. |
| `Slingshot is running for this account but did not answer` | Another `slingshot start` runs here but did not reply. | Ctrl C the other one, then `slingshot start` again. |
| `Slingshot is already running here as archbox, not <name>` | `--name` differs from the running Agent's name, which linked Clients use. | Run `slingshot start` without `--name`. |
| `! Port 7433 is in use, perhaps by another slingshot start, so this one uses 7434` | Another program or account has the pairing port, so the next free one is used. | Nothing. The printed link line has the new port. |
| `Ports 7433 to 7442 are all in use` | All ten ports tried were taken. | Pick another with `slingshot start --port <port>`. |
| `Could not stop this machine from sleeping` | A warning. A sleeping Agent cannot be reached. On Linux, a boot service with nobody logged in cannot block sleep, and GNOME's login screen suspends after about 15 minutes. Issue #68 tracks a fix. | Turn sleep off: `sudo systemctl mask sleep.target suspend.target hibernate.target hybrid-sleep.target`. Slingshot then prints `Sleep is turned off on this machine`. |
| `No iroh relay answered` | A warning. Other networks cannot reach the Agent yet. The same network still works, and Slingshot keeps trying. | Check the Agent's internet connection. |
| No message: unreachable after a restart | Slingshot does not start by itself on this Agent. | Run `slingshot start --boot` on the Agent. |

## Starting By Itself

* Service logs: `journalctl --user -u slingshot.service` on Linux, `~/Library/Application Support/slingshot/agent/daemon.log` on macOS.

| Message | Meaning | Fix |
| --- | --- | --- |
| `Slingshot is set to start by itself but is not running` | The service exists but stopped or failed. | Read the logs, then run `slingshot start --remove` and `slingshot start --boot`. |
| `Slingshot was set up to start at boot but did not start` | The service did not answer within 15 seconds. | Fix what the logs report, then `slingshot start --boot`. |
| `systemctl --user enable --now slingshot.service failed: <reason>`, or the same for `launchctl` | The service manager refused a step. | Fix the reason given, then `slingshot start --boot`. |
| `! Slingshot starts when you log in, not at boot` | Linux refused linger without an admin password. | Run the `sudo loginctl enable-linger <user>` command printed. |
| `Slingshot is already running for this account. Stop the other slingshot start, then run slingshot start --boot again` | A `slingshot start` in another terminal holds the Agent. | Ctrl C there, then `slingshot start --boot`. |
| `Run slingshot start --remove in a terminal to confirm` | Removing asks first, and there was no terminal. | Run it in a terminal on the Agent. |
| `Starting at boot works on Linux and macOS`, or `A line break cannot go in a systemd unit` | No supported service manager, or the Agent name or program path has a line break. | Run `slingshot start` by hand, or rename the Agent with `--name`. |

## Linking

| Message | Meaning | Fix |
| --- | --- | --- |
| `Could not reach 192.168.1.9:7433`, or `Could not reach the Agent at 192.168.1.9:7433` | The Client could not contact the Agent. | Check `slingshot start` runs on the Agent, and both machines share a network or tailnet. Linking across networks is not supported yet. |
| `That pairing code has expired or was already used` | A code works once, lasts 10 minutes, and is burned after 3 wrong tries. | Press Enter where `slingshot start` runs for a new code. |
| `That pairing code is not right` | The code does not match the Agent's. | Copy it again, exactly as printed. |
| `Wrong pairing code from 192.168.1.40 (1 of 3)`, or `3 wrong pairing codes from 192.168.1.40. The code no longer works` | A machine at that address tried a wrong code. If not you, someone on your network tried to pair. | Press Enter for a new code. Pair only with machines you know. |
| `Could not confirm that 192.168.1.9 is the Agent that printed this code` | The machine that answered could not prove it knows the code. Usually a typo; rarely another machine answering in the Agent's place. | Check the code and try again. If it keeps failing on a trusted network, open an issue. |
| `The Agent at 192.168.1.9 runs an older Slingshot`, or `This Agent needs a newer Slingshot` | Pairing changed, so both machines need the same version to link. Linked machines keep working. | Update the machine named, restart `slingshot start` if it is the Agent, then link again. |
| `Another machine is pairing right now` | The Agent pairs with one machine at a time. | Wait a few seconds and link again. |
| `That does not look like a pairing code` | A code has three parts separated by colons, such as `192.168.1.9:7433:K7QW9ZR2`. | Copy the whole code. |

## Setting Up Tools

| Message | Meaning | Fix |
| --- | --- | --- |
| `Could not set up tools on archbox` | Linking worked. Only the tools step failed, for the reason after the message. | Fix it, then run `slingshot tools`. |
| `No install command for brew. Install it on the Agent yourself`, or `No package manager found` | Slingshot only runs install commands it trusts on that Agent. | Install the tool by its own instructions, such as inside `slingshot attach`. |
| `Docker is still missing. Check the output above, then run slingshot tools again` | An install failed, or the tool landed outside the login PATH, `~/.local/bin`, and `~/.cargo/bin`. | Fix the install error, or add the tool's folder to PATH in the Agent's `~/.profile` or `~/.bash_profile`. Then `slingshot tools` again. |
| `Sign in to Codex did not finish` | The sign in was cancelled or failed. The tool is still installed. | Run the command in the message inside `slingshot attach`. For Codex, keep port 1455 free on the Client. |
| `permission denied while trying to connect to the Docker daemon socket` | The account is not in the `docker` group, or was added after sessions and the service started. | `sudo usermod -aG docker <user>` if needed, then restart the machine. `slingshot run` sees it at once, sessions after the restart. |

## Reaching The Agent

* Slingshot tries the local network, then the tailnet, then iroh. The message says why the last try failed.

| Message | Meaning | Fix |
| --- | --- | --- |
| `archbox is not reachable. It may be off or asleep, or slingshot start is not running there` | Nothing answered on any path. | Make sure the Agent is on and awake, and Slingshot runs there. |
| `Slingshot is not running on the Agent. Run slingshot start there` | The Agent is on, but Slingshot stopped. | Run `slingshot start` on the Agent. |
| `archbox no longer accepts this machine` | The Agent no longer knows this Client, usually after an unlink or reset. | Link again from the Agent's network. |
| `Could not reach archbox, because this machine cannot reach iroh's relays` | The Client has no working internet. | Check this machine's connection. |
| `archbox runs an older Slingshot 0.1.0 than this machine (0.2.0)`, or `This machine runs an older Slingshot (0.1.0) than archbox 0.2.0` | The versions cannot talk. The message names the older one. | Run `cargo install --locked --git https://github.com/ado11231/ss slingshot-cli` on it, and restart `slingshot start` if it is the Agent. |
| `! archbox runs Slingshot 0.1.0 and this machine runs 0.2.0` | They still work together, but the older one lacks fixes. | Update the older machine the same way. |
| `Slingshot was not found on the Agent` | The Agent's `slingshot` program was moved or removed. | Install it on the Agent again, then link again. |
| `Host key verification failed` | The Agent's ssh identity changed, usually after a reinstall. | Link again. |
| `No Agent configured yet` | This machine is not linked. | Run `slingshot start` on the Agent, then `slingshot link <code>` here. |
| `Several Agents configured` | More than one Agent is linked, and none is the default. | Add `--agent <name>`, or set `default = "<name>"` in the config file named. |

## Running Commands

| Message | Meaning | Fix |
| --- | --- | --- |
| `No project found` | `sync` and `env` need a project. `attach` works anywhere. | Move into a Git repository, or a folder with `Cargo.toml`, `package.json`, `pyproject.toml`, `requirements.txt`, or `slingshot.toml`. |
| `Lost connection to archbox`, then `The Agent stops the run once it notices` | A run stops when its connection drops. | `slingshot ps --all` shows it as `Lost connection`. Use `slingshot attach` for long work. |
| `cargo was not found on the Agent. Check the name, or install it there`, exit 127 | The program is not on the Agent, or the name has a typo. | Check the name, or run `slingshot tools` to install it. |
| `./build.sh is not allowed to run on the Agent. Check that it is executable`, exit 126 | The file exists but is not executable. | `chmod +x build.sh` on the Client, then run again. The sync copies the change. |
| `command not found` in a session, for a tool the Client has | Sessions use only tools on the Agent. | Run `slingshot tools`. |
| `Lost connection to archbox. The session keeps running there` | The session still runs on the Agent. | Run `slingshot attach` to return. |
| `! Did not bring back edits: NOTES.md changed on both this machine and archbox` | The same files changed in both places. Both versions are kept. | Follow the conflict row under [Syncing Files](#syncing-files), then `slingshot sync --pull`. |
| `RAM is 92% used` or `Only 1.2 GiB free on the Agent workspace disk` | A warning. The run still starts, but may be slow or fail. | Free memory or disk on the Agent. |
| `No swap configured` | No swap, the disk space used when RAM runs out. A large job may be stopped when memory fills. | Add swap on the Agent. |
| `Live views need an interactive terminal` | `slingshot health --watch` redraws the screen. | Run it in a terminal, or use `slingshot health` to pipe the output. |

## Syncing Files

| Message | Meaning | Fix |
| --- | --- | --- |
| `NOTES.md changed on both this machine and archbox`, or `3 files changed on both this machine and archbox` | A conflict: the same files changed differently on both machines. Nothing was copied. | Compare with `slingshot sync --check` and `slingshot sync --pull --check`. Keep one side with `slingshot sync --overwrite` or `slingshot sync --pull --overwrite`, which back up the replaced files. Or make the files match by hand and sync again. |
| `NOTES.md and notes.md differ only in case` | This machine's disk treats both names as one, as a Mac does, so pulling both would overwrite one. Nothing changed. | Rename or remove one on the Agent, then sync again. |
| `An interrupted sync of app needs recovery. Run slingshot sync to recover it` | A sync stopped partway, such as when the connection dropped. | Run `slingshot sync`. It finishes or undoes the earlier one. |
| `Could not recover an interrupted sync. Backups are in ...` | The sync could not be finished or undone. | Make each listed file match its backup, or delete it, then sync again. |
| `This project has no copy on the Agent yet. Run slingshot sync first` | A pull needs a copy on the Agent. | Run `slingshot sync` once. |
| `Unsupported source path` or `Unsafe source link` | A name or link cannot be copied safely, such as a link outside the project. | Remove it, or add it to `.gitignore`. |
| `Message is too large. The project may have too many files` | The file list is over the limit. | Leave out large generated or data folders with `.gitignore` or `sync.exclude`. |
| `sync.exclude in slingshot.toml cannot contain negated patterns` | Patterns starting with `!` are not allowed. | Remove them. |

## Jobs And GPU

| Message | Meaning | Fix |
| --- | --- | --- |
| `No running Slingshot job matches abc123` | No running job has that ID. Finished jobs cannot be stopped. | Copy the ID from `slingshot ps`. |
| `GPU reporting unavailable, because nvidia-smi is not installed` | A warning. GPU use is read with `nvidia-smi`, which comes with the NVIDIA driver. | Nothing without an NVIDIA GPU. With one, install the driver and `slingshot start` again. |
| `NVIDIA driver and library versions differ` | The driver was updated, but the old one is still loaded. | Restart the Agent. |
| `No GPU data available` | No NVIDIA card, or no `nvidia-smi`. | Nothing. Slingshot works without a GPU. |

## Projects

| Message | Meaning | Fix |
| --- | --- | --- |
| `No project named web on archbox` | No copy on the Agent has that name. | Check the name with `slingshot projects`. |
| `2 projects are named app on archbox. Use the ID of the one you mean` | Two Clients each have a project with this name. | Run the command again with the ID from the message. |
| `app has no copy on archbox` | The current project was never synced to this Agent. | Nothing to clean or remove. `slingshot sync` copies it. |
| `app has a run in progress`, or `a session in progress` | The project is in use. | Stop it with the `slingshot stop` command in the message, or wait. |
| `Run slingshot projects remove in a terminal to confirm, since it deletes files` | It asks first, and a script cannot answer. | Run it in a terminal. |

## The Menu Bar App

| Message | Meaning | Fix |
| --- | --- | --- |
| `Swift is needed to build the menu bar app` | The app builds with Swift, from the Xcode command line tools. | Run `xcode-select --install`, then `slingshot menubar` again. |
| `Could not build the menu bar app. Swift said:`, then Swift's last lines | The build failed. The app needs macOS 14 or later. | Update the Xcode command line tools in System Settings, General, Software Update, and run `slingshot menubar` again. If it still fails, open an issue with the full message. |
| Panel: `Slingshot speaks format <n>, and this app speaks format <m>` | The app is older than the `slingshot` program. | Run `slingshot menubar` to rebuild it. |
| Panel: `Can't find the slingshot program` | The saved path to `slingshot` was moved, deleted, or never saved. | Run `slingshot menubar`, and again after moving `slingshot`. |
| `! The app will run <path>, which cargo clean deletes` | `slingshot menubar` ran from a build in a cargo `target` folder. | Run `cargo install --path crates/slingshot-cli`, then `slingshot menubar` again. |
| `Could not take the app out of login items` | `--remove` could not start the app to leave login items. | Turn off Slingshot in System Settings, General, Login Items, then `slingshot menubar --remove` again. |
| No message: shown as offline | The app cannot reach the Agent. | Open the panel for the cause and fix. It updates by itself, or press Try again. |
| No message: no notifications | Notifications are off, or nothing needed one. Runs under 10 seconds never notify, and each warning is sent once until it clears. | Turn them on in System Settings, Notifications, Slingshot. |

## Removing Slingshot

| Message | Meaning | Fix |
| --- | --- | --- |
| `Could not reach archbox, so the key is still there` | `slingshot unlink` could not reach the Agent. | On the Agent, delete the line in `~/.ssh/authorized_keys` ending with the name in the message. |
| `Nothing else was removed`, or unlink refuses | A job is still running. | Stop each with `slingshot stop <id>`, then try again. |
| `Slingshot is running in a terminal on this machine. Stop it with Ctrl C there` | A `slingshot start` by hand would recreate files as they are removed. | Ctrl C where it runs, then `slingshot uninstall` again. |
| `Run slingshot uninstall in a terminal to confirm, since it deletes files` | It asks first, and a script cannot answer. | Run it in a terminal. |
| `Could not remove ... Delete it by hand` | A file could not be deleted, such as from its permissions. | Delete it yourself, then `slingshot uninstall` again. |

## Getting Help

* Open an issue on GitHub with the command you ran, what you expected, the full output, and each machine's operating system. [CONTRIBUTING.md](../CONTRIBUTING.md#reporting-a-problem) has the full list.
