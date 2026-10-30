<h3 align="center">ally</h3>

<p align="center"><img src="https://raw.githubusercontent.com/ado11231/ally/master/docs/media/ally.png" alt="Ally logo, a small blue pixel art creature" width="72"></p>

<p align="center">
  <a href="https://github.com/ado11231/ally/actions/workflows/ci.yml"><img src="https://github.com/ado11231/ally/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://crates.io/crates/ally-cli"><img src="https://img.shields.io/crates/v/ally-cli" alt="crates.io"></a>
  <img src="https://img.shields.io/badge/platform-macOS%20%7C%20Linux-lightgrey" alt="macOS and Linux">
</p>

<p align="center">run terminal processes and local models on your stronger computer. use your gaming PC or mini computer as a server.</p>

<p align="center"><img src="https://raw.githubusercontent.com/ado11231/ally/master/docs/media/demo.gif" alt="Starting Ally on the powerful machine, linking the laptop, and opening a session" width="830"></p>

## What It Does

* **Use a stronger computer.** Builds and servers run on your powerful machine, started from your laptop.
* **Feels local.** Output, colors, and shortcuts work as usual.
* **Keep your tools.** Your editor, terminal, and browser stay the same.
* **Fast copying.** Only your code is copied. Build files stay on the powerful machine.
* **Keeps running.** Close your laptop, and your work carries on.
* **Works anywhere.** Home or away, with no account and no network setup.
* **Private.** One time linking codes, and everything runs under your own account.

## Setup

| Machine | What It Is |
| --- | --- |
| **Agent** | The powerful computer that does the work. |
| **Client** | The computer you work on, such as your laptop. |

### On The Agent

**1. Install what Ally needs.** Open your system and run the commands.

<details>
<summary>Arch Linux</summary>

```sh
sudo pacman -S --needed base-devel rsync tmux openssh rustup
rustup default stable
sudo systemctl enable --now sshd
```

</details>

<details>
<summary>Ubuntu or Debian</summary>

```sh
sudo apt install rsync tmux openssh-server curl build-essential
sudo systemctl enable --now ssh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

When asked, press Enter to install Rust, then open a new terminal.

</details>

<details>
<summary>Fedora</summary>

```sh
sudo dnf install rsync tmux openssh-server gcc
sudo systemctl enable --now sshd
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

When asked, press Enter to install Rust, then open a new terminal.

</details>

<details>
<summary>macOS</summary>

```sh
xcode-select --install
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
brew install tmux
```

A window opens to install Apple's developer tools. Finish it before going on.

When asked, press Enter to install Rust, then open a new terminal.

Install [Homebrew](https://brew.sh) first if you don't have it.

Turn on Remote Login in System Settings, General, Sharing.

</details>

<br>

**2. Install Ally.**

```sh
cargo install --locked --git https://github.com/ado11231/ally ally-cli
ally --version
```

If `command not found` shows, your terminal can't find Ally yet. Add it to your path with this line, then open a new terminal. On zsh, use `~/.zshrc` instead.

```sh
echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> ~/.bashrc
```

**3. Start Ally.**

```sh
ally start
```

Checks the machine and shows how to fix anything missing.

Asks whether to start Ally when the computer turns on. Answer yes to keep the Agent ready after a restart.

Prints a line such as `ally link 192.168.1.9:7433:K7QW9ZR2` for the Client.

Each line works once and expires after 10 minutes. Press Enter for a new one.

### On The Client

**1. Install what Ally needs.** Open your system and run the commands.

<details>
<summary>macOS</summary>

```sh
xcode-select --install
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

A window opens to install Apple's developer tools. Finish it before going on.

When asked, press Enter to install Rust, then open a new terminal.

</details>

<details>
<summary>Arch Linux</summary>

```sh
sudo pacman -S --needed base-devel rsync rustup
rustup default stable
```

</details>

<details>
<summary>Ubuntu or Debian</summary>

```sh
sudo apt install rsync curl build-essential
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

When asked, press Enter to install Rust, then open a new terminal.

</details>

<details>
<summary>Fedora</summary>

```sh
sudo dnf install rsync gcc
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

When asked, press Enter to install Rust, then open a new terminal.

</details>

<br>

**2. Install Ally**, the same way as on the Agent.

```sh
cargo install --locked --git https://github.com/ado11231/ally ally-cli
ally --version
```

If `command not found` shows, add Ally to your path the same way as on the Agent.

**3. Link to the Agent.** Run the line the Agent printed.

```sh
ally link 192.168.1.9:7433:K7QW9ZR2
```

Both machines need the same network or Tailscale for this step only. After that, Ally works from anywhere.

Offers to set up your tools on the Agent, such as Claude Code, and the menu bar app on a Mac. Asks before each one.

**4. Try it.** Open a terminal in your project folder, then run:

```sh
ally run cargo build
```

Any command works, such as `npm run dev` or `python main.py`.

## Commands

| Command | Does |
| --- | --- |
| `ally run <command>` | Copies your changes, then runs the command on the Agent. |
| `ally attach` | Opens a terminal on the Agent that keeps running when you disconnect. |
| `ally sync --pull` | Brings edits made on the Agent back to the Client. |
| `ally health` | Shows the Agent's hardware and use. Add `--watch` to keep it live. |
| `ally ps` | Lists running jobs. `ally stop <id>` stops one. |
| `ally projects` | Lists the projects on the Agent and their size. `clean` frees build output. |

Every command and option is in [USAGE.md](https://github.com/ado11231/ally/blob/master/docs/USAGE.md).

## Menu Bar

On a Mac, `link` offers a menu bar app showing how busy the Agent is and when a run finishes.

<p align="center"><img src="https://raw.githubusercontent.com/ado11231/ally/master/docs/media/menubar.png" alt="The Ally menu bar panel" width="360"></p>
