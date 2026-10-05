# Checklists: steps inside a task
#
#   cargo build --release && vhs docs/videos/06-checklist.tape  →  docs/videos/06-checklist.gif
#
# Records on the demo list from docs/videos/setup.sh (its own config and
# notes under /tmp/tasq-showcase; your tasks are never touched).

Output docs/videos/06-checklist.gif

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
Sleep 1200ms
# Tab moves into the details.
Tab
Sleep 1000ms
Type "j"
Sleep 500ms
Type "j"
Sleep 800ms
Type "x"
Sleep 1200ms
Type "j"
Sleep 500ms
Type "x"
Sleep 1200ms
# a adds items, one after another.
Type "a"
Sleep 800ms
Type "Conclusion"
Sleep 600ms
Enter
Sleep 500ms
Type "Abstract"
Sleep 600ms
Enter
Sleep 800ms
Enter
Sleep 2500ms
