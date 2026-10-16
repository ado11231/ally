# Roadmap

* What works, what was tested, and what comes next.
* A feature is complete only after it ran on real machines.
* Last updated October 15, 2026.

## Contents

1. [Status](#status)
2. [Complete](#complete)
3. [Planned](#planned)
4. [Known Limitations](#known-limitations)
5. [Test Record](#test-record)

## Status

| Phase | Goal | Status |
| --- | --- | --- |
| 1 | Pairing, remote commands, and hardware details on a local network | Complete |
| 2 | Project recognition, and build output on the Agent | Complete |
| 3 | Source copies, sync, sessions, jobs, environment files, and live health | Complete |
| 4 | Reaching the Agent from any network | Complete |
| 5 | Menu bar app, notifications, and port forwarding | In progress |
| 6 | Local AI models, pairing across networks, and more platforms | Planned |
| 7 | Installers, packages, and a public release | Planned |

## Complete

### Phase 1: Pairing And Remote Commands

* `slingshot start` checks for an ssh server, `rsync`, and `tmux`, and prints a one time pairing code.
* `slingshot link` installs the Client's key on the Agent. The Client needs no ssh server.
* `slingshot run` runs a command with live output, typing, Ctrl C, and the real exit code.
* `slingshot health` shows the Agent's hardware and use.

### Phase 2: Mounting The Project

* Built September 7 and 8, 2026. The Agent mounted the Client's folder over SSHFS, a network file system over ssh.
* Replaced by phase 3 on September 13, because the Client needed its own ssh server, every file operation crossed the network, and a stale mount could hang a build.
* Phase 3 kept its project recognition and separate build output storage. The last mount code was removed September 20.

### Phase 3: Source Copies, Sessions, And Live Status

* The Agent keeps its own copy of each project, kept in step by three way sync. Conflicts stop the sync, so no edit is lost.
* Build output for Rust, Node, and Python stays in separate Agent storage.
* `attach` opens one lasting session per project. `sync` pushes, pulls with `--pull`, and previews with `--check`.
* `env` stores environment files apart from the source copy. `ps` and `stop` manage jobs. `health --watch` refreshes live. `unlink` removes this Client's key and files.

### Phase 4: Reaching The Agent From Any Network

* The Client tries the local network, then the tailnet, then iroh, and every run names its path.
* iroh connects directly when it can, and through a relay when not. A relay sees only encrypted data.
* The Agent accepts iroh only from paired Clients, stays awake while `slingshot start` runs, and closes iroh cleanly when stopped, so Clients reconnect at once.
* Errors tell an Agent that is off apart from a Client that is no longer paired.
* **Tested:** a connection lost for 4 minutes during a run over iroh ends the run with a clear message, and the Agent stops it.

### Phase 5: Built So Far

| Area | What Works |
| --- | --- |
| Menu bar app | CPU, RAM, GPU, VRAM, and workspace space; an offline screen with the cause, fix, and Try again; always a login item; follows a new `slingshot` by itself; moves to a better connection within a minute. `slingshot menubar` builds, signs, installs, and removes it. |
| Notifications | Finished, failed, and interrupted runs; the Agent going offline or returning; low memory, low disk, or a hot GPU. Shown with the Slingshot icon. |
| Sessions | `attach` from anywhere. Syncs first, and pulls edits back on leaving. Login shell, full color, `UTF-8`, mouse scrolling, a colored bar, `~/Slingshot/<project>`, and the user folders on PATH. Works from terminals the Agent may not know, such as Kitty. |
| Tools | `link` ends by offering the Client's tools on the Agent, then signs in to Claude Code and Codex. `slingshot tools` repeats it, offers sign in when signed out, and lists `sudo` steps for an admin on accounts without it. |
| Starting | A systemd user service with linger, or a launchd agent, offered on the first `slingshot start`. `--boot` and `--remove` manage it. Moves to the next free port when the pairing port is taken. Warns when Docker is unusable, and says when sleep is off. |
| Sync | `--overwrite` keeps one side of a conflict, with a backup. Pulls refuse names that differ only in case. A Python `.venv` works on the Agent. |
| Output | Problems on one line with dimmed fixes. `link` and `tools` in sections. Stopped runs are not failed, and `ps --all` shows `Lost connection`. Version errors name the older machine. |
| Removal | `slingshot uninstall` lists everything on a Client or Agent, asks once with No as the default, and keeps the owner's own files and keys. |
| Projects | `slingshot projects` lists each copy on the Agent with its size, build output, last sync, and edits waiting. `clean` frees build output, and `remove` deletes a copy after asking. |

## Planned

| Phase | Work | Issue |
| --- | --- | --- |
| 5 | Port forwarding, plus ready and waiting notifications | [#9](https://github.com/ado11231/ss/issues/9) |
| 6 | Local AI models through Ollama | [#20](https://github.com/ado11231/ss/issues/20) |
| 6 | Pairing across networks, keeping the code safe through a relay | [#19](https://github.com/ado11231/ss/issues/19) |
| 6 | Watching several Agents from the menu bar | [#109](https://github.com/ado11231/ss/issues/109) |
| 6 | A setting to use your own iroh relay, and stronger Linux Client support | |
| 7 | Ready to run programs for macOS and Linux, on Intel and ARM | |
| 7 | Publishing to crates.io, AUR, and Homebrew | [#17](https://github.com/ado11231/ss/issues/17) |
| 7 | A one line installer, and `slingshot doctor` to check a setup | |

* Phase 7 is done when a newcomer goes from install to a working `slingshot run` in under five minutes, with no help.

## Known Limitations

* On Linux, a boot service with nobody logged in cannot block sleep, so an Agent that can sleep may suspend after about 15 minutes idle ([#68](https://github.com/ado11231/ss/issues/68)).
* Starting by itself on a macOS Agent is untested.
* After the Agent loses power or crashes, the first iroh connection can wait about 20 seconds for the old one to time out.
* Edits in a session whose connection dropped stay on the Agent until `slingshot sync --pull`.
* The tools step installs Docker only with `pacman`, `apt`, `dnf`, and `zypper`, and Git, Node, and Python only with package managers it knows.
* Untested: file watchers in sessions, two Clients on one Agent account at once, and large Node and Python projects.
* `slingshot.toml` supports only `sync.exclude`.
* Linking needs a shared network or tailnet.
* The back online notification and resource warnings have not been seen on real machines.
* A dev server or container on the Agent cannot open in the Client's browser yet ([#9](https://github.com/ado11231/ss/issues/9)).
* The tools step on an account without `sudo` is checked only by unit tests.

## Test Record

* Every test ran between a Mac Client and an Arch Linux Agent named archbox.

### Phase 3: September 13, 2026, Same Network

1. The first `slingshot run cargo build` copied 47 files in 9 seconds. The second started in 1 second.
2. Subfolders, exit codes, typing, and Ctrl C behaved as local. The copy held no `.git` or `target`.
3. Push, pull, preview, an Agent only edit, and a conflict worked. The conflict was refused and nothing changed.
4. Environment files were private to the Agent account, visible to `run`, and never synced.
5. A release build in `attach` kept running through four minutes offline, and `attach` returned to it.
6. Restarting `slingshot start` kept the session. Restarting the Agent marked it interrupted.
7. `unlink` refused during a session, then removed the key and files after `stop`.

### Phase 4: September 22, 2026

1. `run` worked over the home network, the tailnet from a hotspot, and iroh with Tailscale off.
2. Over iroh, the first ssh connection took about 1.5 seconds, later ones about 0.3.
3. With `slingshot start` stopped, `run` failed after about 20 seconds, naming the cause.
4. An unpaired Client was refused over iroh, and a paired one accepted. `sync` and `attach` worked over iroh.

### Phase 5: September 23 And 24, 2026

1. The menu bar app built, installed, and showed live values. The offline screen named the cause and recovered by itself.
2. Runs gave the right notifications, and none for a quick `echo hi`.
3. `attach` outside a project opened `~` with color, `UTF-8`, mouse scrolling, Claude Code, and `docker ps` working. In a project it synced first.
4. `cargo build` in a session ran on archbox while the Mac stayed quiet. `sync --pull` worked with the session open.
5. `slingshot menubar` built in 12 seconds, skipped the build when nothing changed, and rebuilt after a Swift change.

### Safe Pairing: September 25, 2026, Mac Only

* A relay in front of a test Agent recorded every byte on the pairing port.

1. Three wrong codes were refused and counted, and the third burned the code.
2. The recording held neither the code nor any guess.
3. An old style request, and a link to an older Agent, were refused with an instruction to update.

### Fresh Install: September 25, 2026

* A new account, `slingtest`, on archbox played a first time user. Both machines followed only the README.

1. `cargo install` needed `rustup default stable` and `~/.cargo/bin` on PATH, both from Arch Linux's `rustup` package.
2. `slingshot start` passed every check, found the GPU, and reached iroh. Pairing took 0.1 seconds.
3. Codex installed and signed in through the forwarded port. Claude Code installed into `~/.local/bin` but was reported missing; fixed the same day.
4. `cargo build --release` took 54 seconds on archbox against 3 minutes 20 on the Mac. The second run took 0.6 seconds.
5. Wi Fi off for 15 seconds during a run over iroh did not interrupt it. After restarting `slingshot start`, the Mac reconnected without linking again.

### Menu Bar And Sign In: September 28, 2026

1. The app stayed a login item, and switched to a changed `slingshot` within 3 to 9 seconds in four of five tries. Once it took 20 seconds, cause not found.
2. `slingshot menubar --remove` listed, asked, and removed the app.
3. `link` paired again and restarted the app. `health` showed the hardware heading above live numbers.
4. Closing the terminal running `slingshot start` had made the next iroh run take 22 seconds. After the fix, 1.4 to 1.6 seconds.
5. `slingshot tools` noticed Codex was signed out and offered to sign in.
6. A second `slingshot start` under another home moved to port 7434 and said so.

### Start Screen And Starting By Itself: October 6, 2026

1. A new build beside an older running `slingshot start` refused with `A different version of Slingshot is already running for this account`.
2. With a linked Client, `slingshot start` showed the Client and no code. Enter printed a code.
3. The first `slingshot start` offered the service and asked once. After `sudo reboot` with nobody logged in, `run` worked 20 seconds after boot. Masked sleep kept archbox reachable.
4. `--remove` and `--boot` removed and reinstalled the service.
5. Leaving `attach` pulled a file written on archbox. A conflict left each machine its own version.

### Fresh Install And Full Test: October 7, 2026

* Both machines were cleaned and installed from the README. The Agent was a new account without `sudo`. Full results are on [#56](https://github.com/ado11231/ss/issues/56).

1. The Mac install took 2m 43s. `link` paired, installed and signed in to the tools, and built the menu bar app in one flow.
2. A Rust release build took 2.6s on archbox and 7.3s on the Mac. `node_modules` and `dist` stayed on the Agent. `.env` never synced.
3. Runs worked over the local network, the tailnet, and iroh. A 4 minute outage ended the run with a clear message.
4. After a reboot with nobody logged in, archbox answered in under a minute.
5. Found and fixed: `.venv` (#83), Claude Code in `attach` (#82), Compose names (#84), Docker group warning (#85), Kitty (#86), case conflicts (#89), leftover files after removal (#91), and the menu bar keeping iroh (#90).

### Fixes, Sessions, And Sync: October 7, 2026

1. A Python venv worked and survived syncs. Compose named the project `compose-app`.
2. `attach` worked from Kitty. Leaving named the edits brought back.
3. Mouse scrolling, selection, and Ctrl B, K worked in a real terminal.
4. A pull with `NOTES.md` and `notes.md` printed `NOTES.md and notes.md differ only in case` and changed nothing.
5. `sync --overwrite` and `sync --pull --overwrite` each kept a backup. Answering no changed nothing.
6. The menu bar switched paths within 30 to 50 seconds when the Mac changed networks.

### Projects: October 15, 2026

1. `slingshot projects` listed three projects on archbox with their sizes, last sync, and edits waiting.
2. `clean` on Slingshot itself freed 3.9 GiB, and the next `cargo build` made it again in 28 seconds.
3. A throwaway project showed 2 edits waiting. `remove` listed them, kept everything on Enter, and on `y` deleted the copy and its `~/Slingshot` link.
