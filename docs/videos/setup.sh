#!/usr/bin/env bash
# Seeds the demo list every showcase video records on.
#
#   source docs/videos/setup.sh   # sets $D, $TASQ_BIN and the env tasq reads
#
# Everything lives in /tmp/tasq-showcase: its own config, notes and todo.txt,
# so a recording never touches your real tasks. Dates are relative to the day
# you record on, so "today" in the video is always today.

export D=/tmp/tasq-showcase
# The build to record: run from the repo root after `cargo build --release`.
export TASQ_BIN="$PWD/target/release/tasq"
rm -rf "$D" && mkdir -p "$D/cfg/tasq" "$D/notes/tasks/thesis" "$D/notes/tasks/blog" "$D/data"

export XDG_CONFIG_HOME="$D/cfg" XDG_DATA_HOME="$D/data"
export TASQ_NO_UPDATE_CHECK=1 TERM_PROGRAM=ghostty COLORTERM=truecolor

d() { date -d "$1" +%F; }
T=$(d today)
Y=$(d yesterday)
created=$(d "-5 days")
sat=$(d "next saturday")
first=$(d "$(date -d 'next month' +%Y-%m-01)")

cat > "$D/cfg/tasq/config.toml" <<EOF
theme = Catppuccin Macchiato
density = comfortable
view = today
sort = priority
show_left = true
show_right = true
notes_dir = $D/notes
icons = nerd
hints = true
design = 6
start = "list"
EOF

cat > "$D/todo.txt" <<EOF
(A) $created Finish thesis chapter 3 +Uni/Thesis plan:$T at:09:00 dur:2h notes:thesis/
$created Team standup +Work plan:$T at:10:30 dur:15m rec:1w:mon,tue,wed,thu,fri
(B) $created Review pull requests +Work/Backend @laptop plan:$T at:14:00 dur:1h
$created Call mom @phone plan:$T at:19:00
(A) $created Algorithms exam +Uni/Exams due:$(d "+3 days") star:1
(C) $created Renew passport +Personal/Travel due:$Y
(B) $created Write blog post about tasq +Side due:$(d "+5 days") notes:blog/
$created Fix login bug +Work/Backend plan:$(d tomorrow) at:11:00 dur:90m
$created Dentist +Personal/Health @phone plan:$(d "+2 days") at:16:00
$created Book flights to Lisbon +Personal/Travel due:$(d "+10 days")
$created Plan Saturday hike +Personal plan:$sat
$created Pay rent +Personal due:$first rec:1m
$created Buy coffee beans @errands
$created Read Designing Data-Intensive Applications
x $T $created Gym +Personal/Health plan:$T at:07:00 dur:1h rec:1w:mon,wed,fri
x $Y $created Send invoice to client +Work plan:$Y
EOF

cat > "$D/notes/tasks/thesis/checklist.md" <<'EOF'
# Chapter 3

- [x] Outline the argument
- [x] Draft the introduction
- [ ] Results section
- [ ] Figures 3.1 to 3.4
- [ ] References
EOF

cat > "$D/notes/tasks/thesis/meeting.md" <<'EOF'
# Meeting with the supervisor

Move the related work before the method. Keep chapter 3 under 30 pages,
and send the draft by Friday.
EOF

cat > "$D/notes/tasks/blog/draft.md" <<'EOF'
# Why I moved my todos to the terminal

Plain text you own, a list that opens in a blink, and nothing to sync
unless you want to.
EOF

cd "$D" && clear
