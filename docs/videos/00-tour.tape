# The tour: tasq from start to finish, in one take
#
#   cargo build --release && vhs docs/videos/00-tour.tape  →  docs/videos/00-tour.gif
#
# Records on the demo list from docs/videos/setup.sh (its own config and
# notes under /tmp/tasq-showcase; your tasks are never touched).

Output docs/videos/00-tour.gif

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

# Today: your day, in order.
Sleep 2000ms

# Capture in plain words.
Type "n"
Sleep 500ms
Type "Prepare slides for Friday's seminar tomorrow at 10am for 2h in thesis"
Sleep 2500ms
Enter
Sleep 1000ms
Escape
Sleep 300ms
Escape
Sleep 1000ms

# Tick something off.
Type "j"
Sleep 400ms
Type "j"
Sleep 400ms
Type "j"
Sleep 800ms
Type "x"
Sleep 1800ms

# A checklist inside a task.
Type "k"
Sleep 800ms
Tab
Sleep 800ms
Type "j"
Sleep 400ms
Type "j"
Sleep 500ms
Type "x"
Sleep 1500ms
Escape
Sleep 800ms

# Every task, filtered with chips.
Type "3"
Sleep 1500ms
Type "f"
Sleep 800ms
Type "work"
Sleep 1200ms
Enter
Sleep 2000ms
Type "X"
Sleep 1200ms

# Search everything.
Ctrl+K
Sleep 800ms
Type "supervisor"
Sleep 1500ms
Escape
Sleep 600ms

# Notes, edited in place.
Type "N"
Sleep 2500ms
Escape
Sleep 600ms

# The week, then the month.
Type "5"
Sleep 2500ms
Type "6"
Sleep 2500ms

# Home.
Type "0"
Sleep 3000ms
Escape
Sleep 800ms

# A focus timer.
Type "P"
Sleep 3000ms
Ctrl+X
Sleep 1500ms

# And everything else is a key away.
Space
Sleep 3000ms
Escape
Sleep 1500ms
