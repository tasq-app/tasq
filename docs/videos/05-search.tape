# Search: tasks, notes and spaces at once
#
#   cargo build --release && vhs docs/videos/05-search.tape  →  docs/videos/05-search.gif
#
# Records on the demo list from docs/videos/setup.sh (its own config and
# notes under /tmp/tasq-showcase; your tasks are never touched).

Output docs/videos/05-search.gif

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

Ctrl+K
Sleep 1000ms
Type "chapter"
Sleep 2000ms
Down
Sleep 800ms
Enter
Sleep 2500ms
Ctrl+K
Sleep 800ms
Type "lisbon"
Sleep 1500ms
Enter
Sleep 2500ms
