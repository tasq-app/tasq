# Filters: chips you add and take off
#
#   cargo build --release && vhs docs/videos/04-filters.tape  →  docs/videos/04-filters.gif
#
# Records on the demo list from docs/videos/setup.sh (its own config and
# notes under /tmp/tasq-showcase; your tasks are never touched).

Output docs/videos/04-filters.gif

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

Type "3"
Sleep 1500ms
# f opens the filter box: spaces, tags, dates, priority.
Type "f"
Sleep 1200ms
Type "uni"
Sleep 1500ms
Enter
Sleep 2000ms
# Dates in plain words.
Type "f"
Sleep 800ms
Type "this week"
Sleep 1500ms
Enter
Sleep 2500ms
# X takes every filter off.
Type "X"
Sleep 2500ms
