# Calendar: the day, the week, the month
#
#   cargo build --release && vhs docs/videos/08-calendar.tape  →  docs/videos/08-calendar.gif
#
# Records on the demo list from docs/videos/setup.sh (its own config and
# notes under /tmp/tasq-showcase; your tasks are never touched).

Output docs/videos/08-calendar.gif

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

Type "4"
Sleep 3000ms
Type "5"
Sleep 3000ms
Type "6"
Sleep 3000ms
