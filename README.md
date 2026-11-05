<h3 align="center">ally</h3>

<p align="center"><img src="https://raw.githubusercontent.com/ado11231/ally/master/docs/media/ally.png" alt="Ally logo, a small blue pixel art creature" width="72"></p>

<p align="center">
  <a href="https://github.com/ado11231/ally/actions/workflows/ci.yml"><img src="https://github.com/ado11231/ally/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <img src="https://img.shields.io/badge/platform-macOS%20%7C%20Linux-lightgrey" alt="macOS and Linux">
</p>

<p align="center">run terminal processes on your stronger computer. use your gaming PC or mini computer as a server.</p>

<p align="center"><img src="https://raw.githubusercontent.com/ado11231/ally/master/docs/media/demo.gif" alt="Starting Ally on the powerful machine, linking the laptop, and opening a session" width="830"></p>

## What It Does

* **Your laptop stays light.** Builds, servers, and coding agents run on the powerful machine, with live output, colors, and Ctrl C as usual.
* **Your tools stay the same.** Keep your own editor, terminal, and browser.
* **Only code is copied.** Build files stay on the powerful machine, and work keeps running when the laptop closes.
* **Works anywhere, privately.** No account and no network setup. Linking codes work once, and everything runs as your own account.

## Setup

* The **Agent** is the powerful computer. The **Client** is the one you work on.

### 1. Install On Both Machines

Run the lines for each machine's system. When Rust's installer asks, press Enter, then open a new terminal.

<details>
<summary>Arch Linux</summary>

```sh
sudo pacman -S --needed base-devel rsync tmux openssh rustup
rustup default stable
```

</details>

<details>
<summary>Ubuntu or Debian</summary>

```sh
sudo apt install rsync tmux curl build-essential
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

</details>

<details>
<summary>Fedora</summary>

```sh
sudo dnf install rsync tmux gcc
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

</details>

<details>
<summary>macOS</summary>

```sh
xcode-select --install
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
brew install tmux
```

Finish the developer tools window before going on. `brew` needs [Homebrew](https://brew.sh).

</details>

<br>

Then install Ally:

```sh
cargo install --locked --git https://github.com/ado11231/ally ally-cli
```

If `ally` is not found, run `echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> ~/.zshrc` (or `~/.bashrc`), then open a new terminal.

### 2. Start The Agent

Turn on its ssh server first:

| System | Command |
| --- | --- |
| Arch Linux | `sudo systemctl enable --now sshd` |
| Fedora | `sudo dnf install openssh-server && sudo systemctl enable --now sshd` |
| Ubuntu or Debian | `sudo apt install openssh-server && sudo systemctl enable --now ssh` |
| macOS | System Settings, General, Sharing, Remote Login |

```sh
ally start
```

* Checks the machine and prints the fix for anything missing.
* Offers to start Ally after a restart. Say yes to keep the Agent ready.
* Prints a line such as `ally link 192.168.1.9:7433:K7QW9ZR2`. It works once, for 10 minutes.

### 3. Link The Client

```sh
ally link 192.168.1.9:7433:K7QW9ZR2
```

* Both machines need the same network or Tailscale for this step only. After that, Ally works from anywhere.
* On a Mac, it also offers the menu bar app.

### 4. Try It

In a project folder on the Client:

```sh
ally run cargo build
```

* Any command works, such as `npm run dev` or `python main.py`.
* Commands use the Agent's tools. Install what you need there, such as inside `ally attach`.

## Commands

| Command | Does |
| --- | --- |
| `ally run <command>` | Copies your changes, then runs the command on the Agent. |
| `ally attach` | Opens a terminal on the Agent that keeps running when you disconnect. |
| `ally sync --pull` | Brings edits made on the Agent back to the Client. |
| `ally health` | Shows the Agent's hardware and use. Add `--watch` to keep it live. |
| `ally ps` | Lists running jobs. `ally stop <id>` stops one. |
| `ally storage` | Lists the project copies on the Agent. `clean` frees build output, and `remove` deletes a copy. |
| `ally forward <port>` | Opens an Agent port at `localhost` here. Servers that `run` and `attach` start open by themselves. |
| `ally doctor` | Checks the whole setup, and prints the fix for each problem. |

Every command and option is in [USAGE.md](https://github.com/ado11231/ally/blob/master/docs/USAGE.md).

## Menu Bar

On a Mac, `link` offers a menu bar app showing how busy the Agent is and when a run finishes.

<p align="center"><img src="https://raw.githubusercontent.com/ado11231/ally/master/docs/media/menubar.png" alt="The Ally menu bar panel" width="360"></p>
