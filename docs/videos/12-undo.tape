# Undo and the trash: nothing is lost
#
#   cargo build --release && vhs docs/videos/12-undo.tape  →  docs/videos/12-undo.gif
#
# Records on the demo list from docs/videos/setup.sh (its own config and
# notes under /tmp/tasq-showcase; your tasks are never touched).

Output docs/videos/12-undo.gif

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
Sleep 400ms
Type "j"
Sleep 1000ms
Type "dd"
Sleep 2000ms
Type "u"
Sleep 2500ms
