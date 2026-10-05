# Today: your day, in order
#
#   cargo build --release && vhs docs/videos/02-today.tape  →  docs/videos/02-today.gif
#
# Records on the demo list from docs/videos/setup.sh (its own config and
# notes under /tmp/tasq-showcase; your tasks are never touched).

Output docs/videos/02-today.gif

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

# Morning and afternoon, the time each thing takes, and your day on the right.
Sleep 1500ms
Type "j"
Sleep 700ms
Type "j"
Sleep 700ms
Type "j"
Sleep 1200ms
# Done: it's ticked off and the day's progress moves.
Type "x"
Sleep 2500ms
Type "j"
Sleep 600ms
Type "x"
Sleep 3000ms
