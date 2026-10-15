# The README recording. Needs a linked Agent reachable on the local network, with Slingshot
# running there. Run from the repository root, then draw the window over it (edge, title
# bar, and magenta corners) and make the magenta see through:
#   vhs docs/media/demo.tape
#   ffmpeg -i docs/media/demo-raw.gif -i docs/media/window.png -filter_complex "[0][1]overlay=0:0:format=rgb,split[a][b];[a]palettegen[p];[b][p]paletteuse" -y docs/media/demo-keyed.gif
#   python3 docs/media/corners.py docs/media/demo-keyed.gif docs/media/demo.gif
#   rm docs/media/demo-raw.gif docs/media/demo-keyed.gif

Output docs/media/demo-raw.gif

Set Shell zsh
# Twice the size the README shows it at, so text stays sharp on high density screens.
Set FontSize 24
Set Width 1660
Set Height 1100
Set Padding 30
Set WindowBar Colorful
Set WindowBarSize 56
Set Theme { "name": "Slingshot Gray", "background": "#262628", "foreground": "#e6e6e6", "cursor": "#e6e6e6", "selection": "#44444a", "black": "#1c1c1e", "brightBlack": "#7a7a80", "red": "#ff5c6c", "brightRed": "#ff7b88", "green": "#3ee07a", "brightGreen": "#6af59a", "yellow": "#ffcc33", "brightYellow": "#ffdb66", "blue": "#4da3ff", "brightBlue": "#7bbcff", "magenta": "#e070ff", "brightMagenta": "#ea94ff", "cyan": "#2fe0e0", "brightCyan": "#66f0f0", "white": "#d0d0d0", "brightWhite": "#ffffff" }
Set TypingSpeed 45ms

Hide
Type "tmux -f docs/media/demo.tmux.conf new-session -s demo"
Enter
Sleep 1s
Type `tmux select-pane -T 'archbox · Agent' && ssh -i ~/.ssh/slingshot_ed25519 -o IdentitiesOnly=yes -o "UserKnownHostsFile=\"$HOME/Library/Application Support/slingshot/known_hosts\"" ado@10.0.0.193`
Enter
Sleep 3s
Type `unset PROMPT_COMMAND; PS1='\[\e[34m\]❯\[\e[0m\] '; clear`
Enter
Ctrl+A
Type '"'
Sleep 1s
Type "tmux select-pane -T 'laptop · Client' && PROMPT='%F{blue}❯%f ' && clear"
Enter
Ctrl+A
Up
Sleep 500ms
Show

Type "slingshot start"
Sleep 400ms
Enter
Wait+Screen@30s /Press Enter for a code/
Sleep 2s
Enter
Wait+Screen@30s /slingshot link/
Sleep 3s

Ctrl+A
Down
Hide
Type `code=$(tmux capture-pane -p -t :.0 | grep 'local network' | grep -oE '[0-9.]+:7433:[A-Z0-9]+'); clear; print -z "slingshot link $code"`
Enter
Sleep 500ms
Show
Sleep 1.5s
Enter
Wait+Screen@30s /Try it/
Sleep 4s

Type "clear && slingshot attach"
Sleep 400ms
Enter
Wait+Screen@30s /Ctrl B, D to detach/
Hide
Type `unset PROMPT_COMMAND; PS1='\[\e[34m\]❯\[\e[0m\] '; clear`
Enter
Sleep 500ms
Show
Sleep 1.5s
Type "nvidia-smi -L && cargo build --release"
Sleep 400ms
Enter
Wait+Screen@120s /Finished/
Sleep 3s
Ctrl+B
Type "d"
Wait+Screen@15s /It keeps running/
Sleep 3s

Hide
Type "tmux kill-session -t demo"
Enter
