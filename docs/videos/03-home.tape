# Home: everything at a glance
#
#   cargo build --release && vhs docs/videos/03-home.tape  →  docs/videos/03-home.gif
#
# Records on the demo list from docs/videos/setup.sh (its own config and
# notes under /tmp/tasq-showcase; your tasks are never touched).

Output docs/videos/03-home.gif

Set FontSize 14
Set FontFamily "JetBrains Mono, Symbols Nerd Font"
Set Width 1200
Set Height 720
Set Padding 16
Set Theme "Catppuccin Macchiato"
Set TypingSpeed 45ms
Set Shell "bash"

Hide
Type "source docs/videos/setup.sh && $TASQ_BIN $D/todo.txt"
Enter
Sleep 1500ms
Show
Sleep 1500ms

Type "0"
Sleep 2500ms
# Tab walks the tiles; arrows go through what's in them.
Tab
Sleep 900ms
Down
Sleep 700ms
Down
Sleep 900ms
Tab
Sleep 900ms
Tab
Sleep 900ms
Tab
Sleep 900ms
Tab
Sleep 1200ms
# Enter opens what you picked.
Enter
Sleep 2500ms
