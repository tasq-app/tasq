# Capture: write it the way you say it
#
#   cargo build --release && vhs docs/videos/01-capture.tape  →  docs/videos/01-capture.gif
#
# Records on the demo list from docs/videos/setup.sh (its own config and
# notes under /tmp/tasq-showcase; your tasks are never touched).

Output docs/videos/01-capture.gif

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

# The new-task box reads dates, times, durations and spaces as you type.
Type "n"
Sleep 600ms
Type "Study for the algorithms exam tomorrow at 6pm for 1.5h in exams"
Sleep 2500ms
Enter
Sleep 1200ms

# The box stays open for the next one. Repeats too.
Type "Gym every mon wed fri at 7am for 1h in health"
Sleep 2500ms
Enter
Sleep 1000ms
# Esc twice: once to normal mode, once to close.
Escape
Sleep 300ms
Escape
Sleep 800ms

# Both land where they belong: Upcoming.
Type "2"
Sleep 3000ms
