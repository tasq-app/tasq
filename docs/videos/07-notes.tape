# Notes: search them, edit them in place
#
#   cargo build --release && vhs docs/videos/07-notes.tape  →  docs/videos/07-notes.gif
#
# Records on the demo list from docs/videos/setup.sh (its own config and
# notes under /tmp/tasq-showcase; your tasks are never touched).

Output docs/videos/07-notes.gif

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

Type "N"
Sleep 2000ms
Type "/"
Sleep 500ms
Type "friday"
Sleep 1500ms
Enter
Sleep 1500ms
# Enter edits right here, the vim way; :wq saves.
Enter
Sleep 1200ms
Type "G"
Sleep 400ms
Type "o"
Sleep 400ms
Type "Ask whether figures count towards the 30 pages."
Sleep 800ms
Escape
Sleep 500ms
Type ":wq"
Sleep 600ms
Enter
Sleep 2500ms
