# Focus: a timer for the task in hand
#
#   cargo build --release && vhs docs/videos/09-focus.tape  →  docs/videos/09-focus.gif
#
# Records on the demo list from docs/videos/setup.sh (its own config and
# notes under /tmp/tasq-showcase; your tasks are never touched).

Output docs/videos/09-focus.gif

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

Type "j"
Sleep 400ms
Type "j"
Sleep 1000ms
Type "P"
Sleep 4000ms
# Ctrl-X stops it.
Ctrl+X
Sleep 2500ms
