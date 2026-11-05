#!/bin/sh
# Installs the ready to run ally program from the latest GitHub release into ~/.local/bin,
# with no Rust needed. Run it on both machines:
#   curl -fsSL https://raw.githubusercontent.com/ado11231/ally/master/install.sh | sh
# The tools Ally needs, such as rsync and tmux, are checked by ally start and ally link.
set -eu

fail() {
    echo "✗ $1" >&2
    exit 1
}

case "$(uname -s)" in
    Darwin) system=apple-darwin ;;
    Linux) system=unknown-linux-musl ;;
    *) fail "Ally runs on macOS and Linux, not $(uname -s)." ;;
esac

case "$(uname -m)" in
    x86_64 | amd64) cpu=x86_64 ;;
    arm64 | aarch64) cpu=aarch64 ;;
    *) fail "There is no ready to run Ally for $(uname -m). Build it with Rust: cargo install --locked --git https://github.com/ado11231/ally ally-cli" ;;
esac

if command -v sha256sum >/dev/null 2>&1; then
    checksum="sha256sum"
elif command -v shasum >/dev/null 2>&1; then
    checksum="shasum -a 256"
else
    fail "Checking the download needs sha256sum or shasum. Install coreutils, then run this again."
fi

command -v curl >/dev/null 2>&1 || fail "Downloading Ally needs curl. Install curl, then run this again."

archive="ally-$cpu-$system.tar.gz"
url="https://github.com/ado11231/ally/releases/latest/download/$archive"
bin="$HOME/.local/bin"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

echo "Downloading $archive"
curl -fsSL "$url" -o "$work/$archive" || fail "Could not download $url. Check the connection, then run this again."
curl -fsSL "$url.sha256" -o "$work/$archive.sha256" || fail "Could not download $url.sha256. Check the connection, then run this again."
(cd "$work" && $checksum -c "$archive.sha256" >/dev/null) || fail "The download does not match its checksum. Run this again."

tar -xzf "$work/$archive" -C "$work"
mkdir -p "$bin"
mv "$work/ally" "$bin/ally.new"
mv "$bin/ally.new" "$bin/ally"
echo "✓ Installed $("$bin/ally" --version) to $bin/ally"

case ":$PATH:" in
    *":$bin:"*) ;;
    *)
        echo "! $bin is not on PATH. Add it, then open a new terminal:"
        case "${SHELL:-}" in
            */zsh) echo "  echo 'export PATH=\"\$HOME/.local/bin:\$PATH\"' >> ~/.zshrc" ;;
            */fish) echo "  fish_add_path ~/.local/bin" ;;
            *) echo "  echo 'export PATH=\"\$HOME/.local/bin:\$PATH\"' >> ~/.bashrc" ;;
        esac
        ;;
esac

echo "Next: run ally start on the Agent, then ally link <code> on the Client."
