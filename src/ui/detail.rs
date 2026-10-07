//! The inspector: the current task in full on the right — its title, when
//! it happens and where it belongs, its checklist, and the notes linked to
//! it as cards. `Tab` gives it the keyboard.

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use crate::app::{App, InspectorRow, TaskNotes};
use crate::theme::Theme;
use crate::todo::Task;
use crate::ui::task_row::{chip_date, due_label, tint};

/// Width of the labels column.
const KEY_W: u16 = 9;

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let theme = app.theme();
    // Halfway between the sidebar's shade and the list's.
    let ibg = tint(theme.panel, theme.bg, 0.5).unwrap_or(theme.panel);
    super::fill_bg(frame, area, Style::default().bg(ibg));
    if area.width < 12 || area.height < 3 {
        return;
    }
    let buf = frame.buffer_mut();
    // A hairline between the list and the inspector.
    for y in area.top()..area.bottom() {
        if let Some(c) = buf.cell_mut((area.x, y)) {
            c.set_symbol("│");
            c.set_style(Style::default().fg(theme.border).bg(ibg));
        }
    }
    let inner = Rect {
        x: area.x + 3,
        y: area.y + 1,
        width: area.width - 4,
        height: area.height - 1,
    };
    let Some(t) = app.cur_task() else {
        let mut p = Pen::new(buf, inner, Style::default().bg(ibg));
        p.text(0, "no task selected", p.bg.fg(theme.dim));
        app.inspector_scroll.set(0);
        return;
    };
    let notes = app.task_notes(t);
    let focus = app
        .inspector_focus
        .then(|| app.inspector_current())
        .flatten();
    let body = |p: &mut Pen| {
        title(p, t, theme);
        p.y += 1;
        facts(p, t, app, theme);
        p.y += 1;
        checklist(p, &notes, focus.as_ref(), theme);
        note_cards(p, t, &notes, focus.as_ref(), theme);
    };
    // A long checklist runs off the bottom: a dry run finds where the
    // cursor lands, and the column scrolls to keep it in view.
    let off = if focus.is_some() {
        let mut dry = Pen::new(buf, inner, Style::default().bg(ibg));
        dry.draw = false;
        body(&mut dry);
        let line = dry.cursor_y.map(|y| usize::from(y - inner.y));
        let total = usize::from(dry.y - inner.y);
        super::keep_cursor_visible(app.inspector_scroll.get(), line, inner.height, total)
    } else {
        0
    };
    app.inspector_scroll.set(off);
    let mut p = Pen::new(buf, inner, Style::default().bg(ibg));
    p.off = off;
    body(&mut p);
    let used = p.y;
    // In Today, your day at a glance at the foot of the column.
    if off == 0 && app.prefs.scope == crate::app::Scope::Today && app.calendar.is_none() {
        your_day(p.buf, inner, used, app, theme, ibg);
    }
    for (r, h) in p.marks {
        // Marks are in the column's own rows; on screen they shift by the
        // scroll, and what scrolled away can't be clicked.
        let top = r.y.saturating_sub(off).max(inner.y);
        let bottom = (r.y + r.height).saturating_sub(off).min(inner.bottom());
        if bottom > top {
            app.hits.add(
                Rect {
                    y: top,
                    height: bottom - top,
                    ..r
                },
                h,
            );
        }
    }
    // The edge between the list and the inspector, to drag wider.
    app.hits.add(
        Rect {
            x: area.x,
            width: 1,
            ..area
        },
        crate::app::Hit::DetailsEdge,
    );
}

/// Writes down the inspector, a row at a time, clipped to its area.
struct Pen<'a> {
    buf: &'a mut Buffer,
    r: Rect,
    y: u16,
    bg: Style,
    /// What was drawn where, for the mouse.
    marks: Vec<(Rect, crate::app::Hit)>,
    /// Rows scrolled off the top: `y` counts the column's own rows, and
    /// row `y` lands on screen at `y - off`.
    off: u16,
    /// False for a dry run that only measures.
    draw: bool,
    /// The first row the cursor was drawn on.
    cursor_y: Option<u16>,
}

impl<'a> Pen<'a> {
    fn new(buf: &'a mut Buffer, r: Rect, bg: Style) -> Self {
        Self {
            buf,
            r,
            y: r.y,
            bg,
            marks: Vec::new(),
            off: 0,
            draw: true,
            cursor_y: None,
        }
    }
}

impl Pen<'_> {
    /// The screen row the current row lands on, if it's in view.
    fn screen_y(&self) -> Option<u16> {
        let y = self.y.checked_sub(self.off)?;
        (self.draw && y >= self.r.y && y < self.r.bottom()).then_some(y)
    }

    /// The last row there's room for, in the column's own rows.
    fn bottom(&self) -> u16 {
        if self.draw {
            self.r.bottom().saturating_add(self.off)
        } else {
            u16::MAX
        }
    }

    /// Write `s` at column `dx` of the current row; returns its width.
    fn text(&mut self, dx: u16, s: &str, style: Style) -> u16 {
        let Some(y) = self.screen_y() else {
            return 0;
        };
        if dx >= self.r.width {
            return 0;
        }
        let x = self.r.x + dx;
        let max = usize::from(self.r.width - dx);
        let (end, _) = self.buf.set_stringn(x, y, s, max, style);
        end.saturating_sub(x)
    }

    /// Paint the current row (and a column either side) in `bg`.
    fn fill(&mut self, bg: Color) {
        let Some(y) = self.screen_y() else {
            return;
        };
        let right = (self.r.right() + 1).min(self.buf.area.right());
        for x in self.r.x.saturating_sub(1)..right {
            if let Some(c) = self.buf.cell_mut((x, y)) {
                c.set_symbol(" ");
                c.set_bg(bg);
            }
        }
    }

    /// Write `s` from column `dx`, wrapping onto the rows below at the
    /// same column; leaves the pen on the last row written.
    fn wrapped(&mut self, dx: u16, s: &str, style: Style) {
        let width = usize::from(self.r.width.saturating_sub(dx)).max(4);
        let lines = wrap(s, width);
        let n = lines.len();
        for (i, line) in lines.iter().enumerate() {
            self.text(dx, line, style);
            if i + 1 < n {
                self.y += 1;
            }
        }
    }

    /// The `▎` marker left of the focused row.
    fn bar(&mut self, theme: &Theme) {
        let Some(y) = self.screen_y() else {
            return;
        };
        if self.r.x < 2 {
            return;
        }
        if let Some(c) = self.buf.cell_mut((self.r.x - 2, y)) {
            c.set_symbol("▎");
            c.set_fg(theme.accent);
        }
    }

    /// Highlight the current row as the cursor; returns its style.
    fn cursor(&mut self, here: bool, theme: &Theme) -> Style {
        if !here {
            return self.bg;
        }
        self.cursor_y.get_or_insert(self.y);
        let bg = tint(theme.accent, theme.panel, 0.22).unwrap_or(theme.selected);
        self.fill(bg);
        self.bar(theme);
        self.bg.bg(bg)
    }
}

fn title(p: &mut Pen, t: &Task, theme: &Theme) {
    let body = crate::todo::body_only(&t.raw);
    let style = if t.done {
        p.bg.fg(theme.done).add_modifier(Modifier::CROSSED_OUT)
    } else {
        p.bg.fg(theme.fg).add_modifier(Modifier::BOLD)
    };
    let width = usize::from(p.r.width).max(8);
    for line in wrap(&body, width).iter().take(3) {
        p.text(0, line, style);
        p.y += 1;
    }
}

/// One `label  value` row.
fn fact(p: &mut Pen, key: &str, value: &str, color: Color, theme: &Theme) {
    p.text(0, key, p.bg.fg(theme.dim));
    p.wrapped(KEY_W, value, p.bg.fg(color));
    p.y += 1;
}

fn facts(p: &mut Pen, t: &Task, app: &App, theme: &Theme) {
    let today = app.today();
    let raw_body = crate::todo::body_after_priority(&t.raw);
    let time = crate::todo::find_kv(raw_body, "at");
    let minutes = t
        .duration
        .as_deref()
        .and_then(crate::duration::parse_minutes);

    // When: the planned day, the time and how long, as one phrase.
    let mut when: Vec<String> = Vec::new();
    if let Some(d) = &t.planned {
        when.push(chip_date(d, today));
    }
    match (&time, minutes) {
        (Some(at), Some(m)) => when.push(format!("{at} – {}", end_time(at, m))),
        (Some(at), None) => when.push(at.clone()),
        (None, Some(m)) => when.push(format!("takes {}", crate::duration::describe(m))),
        (None, None) => {}
    }
    if !when.is_empty() {
        fact(p, "when", &when.join(" · "), theme.fg, theme);
    }
    if let Some(due) = &t.due {
        let color = if !t.done && due.as_str() <= today {
            theme.overdue
        } else {
            theme.due
        };
        let (day, rel) = (chip_date(due, today), due_label(due, today));
        let text = if day == rel {
            day
        } else {
            format!("{day} · {rel}")
        };
        fact(p, "due", &text, color, theme);
    }
    for s in &t.projects {
        let color = app.space_color(s);
        p.text(0, "space", p.bg.fg(theme.dim));
        p.text(KEY_W, "●", p.bg.fg(color));
        p.wrapped(KEY_W + 2, &crate::core::spaces::display(s), p.bg.fg(color));
        p.y += 1;
    }
    if let Some(pri) = t.priority {
        let word = match pri {
            'A' => "high".to_string(),
            'B' => "medium".to_string(),
            'C' => "low".to_string(),
            other => format!("({other})"),
        };
        let color = theme.priority_color(pri);
        fact(p, "priority", &format!("⚑ {word}"), color, theme);
    }
    if let Some(list) = t
        .reminders
        .as_deref()
        .and_then(crate::duration::parse_reminders)
    {
        let text: Vec<String> = list.iter().map(|m| crate::duration::describe(*m)).collect();
        let text = format!("{} before", text.join(", "));
        fact(p, "remind", &text, theme.fg, theme);
    }
    if let Some(rec) = &t.rec {
        let mut text = format!("↻ {}", crate::app::describe_rec(rec));
        if let Some(until) = &t.until {
            text.push_str(&format!(" until {}", chip_date(until, today)));
        }
        if let Some(n) = &t.times {
            text.push_str(&format!(" · {n} left"));
        }
        fact(p, "repeat", &text, theme.pri_other, theme);
    }
    if !t.contexts.is_empty() {
        let tags: Vec<String> = t.contexts.iter().map(|c| format!("@{c}")).collect();
        fact(p, "tags", &tags.join(" "), theme.context, theme);
    }
    if t.done {
        let on = t
            .done_date
            .as_deref()
            .map_or_else(String::new, |d| chip_date(d, today));
        fact(p, "done", &format!("☑ {on}"), theme.ok, theme);
    }
}

/// YOUR DAY: today's timed blocks in their space's colour, the free
/// stretches between them, and how full the next three days are. Drawn
/// at the foot of `r`, below `used` (the details above it).
fn your_day(buf: &mut Buffer, r: Rect, used: u16, app: &App, theme: &Theme, bg: Color) {
    use crate::core::calendar;
    let today = app.today_naive();
    let tasks = app.tasks();
    let occs = calendar::occurrences(tasks, today, today, today);
    let mut timed: Vec<&calendar::Occurrence> = occs
        .iter()
        .filter(|o| o.start.is_some() && !o.late)
        .collect();
    timed.sort_by_key(|o| o.start);
    // The rows: a block per timed thing, a "free until" line for a gap of
    // an hour or more.
    enum Row {
        Block(u32, String, Color, bool),
        Free(u32, u32),
    }
    let mut rows: Vec<Row> = Vec::new();
    let mut prev_end: Option<u32> = None;
    for o in &timed {
        let start = o.start.unwrap_or(0);
        if let Some(end) = prev_end
            && start >= end + 60
        {
            rows.push(Row::Free(end, start));
        }
        let t = &tasks[o.abs];
        let color = t
            .projects
            .first()
            .map_or(theme.accent, |p| app.space_color(p));
        rows.push(Row::Block(
            start,
            crate::todo::body_only(&t.raw),
            color,
            t.done,
        ));
        prev_end = Some(prev_end.map_or(o.end().unwrap_or(start), |e| {
            e.max(o.end().unwrap_or(start))
        }));
    }
    let need = 2 + rows.len().max(1) as u16 + 3;
    if r.bottom() < need || r.bottom() - need <= used + 1 {
        return;
    }
    let mut y = r.bottom() - need;
    let base = Style::default().bg(bg);
    let hm = |m: u32| format!("{:02}:{:02}", m / 60, m % 60);
    let put = |buf: &mut Buffer, x: u16, y: u16, s: &str, max: u16, st: Style| {
        if max > 0 {
            buf.set_stringn(x, y, s, usize::from(max), st);
        }
    };
    put(buf, r.x, y, "YOUR DAY", r.width, base.fg(theme.dim));
    y += 1;
    if rows.is_empty() {
        put(
            buf,
            r.x,
            y,
            "nothing with a time today",
            r.width,
            base.fg(theme.dim),
        );
        y += 1;
    }
    let block_x = r.x + 6;
    let block_w = r.width.saturating_sub(7);
    for row in &rows {
        match row {
            Row::Block(start, title, color, done) => {
                put(buf, r.x, y, &hm(*start), 5, base.fg(theme.dim));
                let tint = crate::ui::task_row::tint(*color, theme.bg, 0.2).unwrap_or(bg);
                let st = Style::default().bg(tint).fg(*color);
                put(
                    buf,
                    block_x,
                    y,
                    &" ".repeat(usize::from(block_w)),
                    block_w,
                    st,
                );
                put(buf, block_x, y, "▌", 1, st);
                let label = if *done {
                    format!("{title} ✓")
                } else {
                    title.clone()
                };
                put(buf, block_x + 2, y, &label, block_w.saturating_sub(3), st);
            }
            Row::Free(from, until) => {
                put(buf, r.x, y, &hm(*from), 5, base.fg(theme.dim));
                put(
                    buf,
                    block_x,
                    y,
                    &format!("· free until {}", hm(*until)),
                    block_w,
                    base.fg(theme.dim),
                );
            }
        }
        y += 1;
    }
    y += 1;
    put(buf, r.x, y, "NEXT 3 DAYS", r.width, base.fg(theme.dim));
    y += 1;
    let mut x = r.x;
    for k in 1..=3u64 {
        let Some(d) = today.checked_add_days(chrono::Days::new(k)) else {
            continue;
        };
        let n = calendar::occurrences(tasks, d, d, today)
            .iter()
            .filter(|o| !tasks[o.abs].done)
            .count();
        let day = d.format("%a").to_string().to_lowercase();
        let text = if k == 1 {
            format!("{day} · {n} {}", if n == 1 { "task" } else { "tasks" })
        } else {
            format!("{day} · {n}")
        };
        let w = text.chars().count() as u16;
        if x + w > r.right() {
            break;
        }
        put(buf, x, y, &text, w, base.fg(theme.status_fg));
        x += w + 2;
    }
}

/// `at` plus `minutes`, as `HH:MM`.
fn end_time(at: &str, minutes: u32) -> String {
    let Some((h, m)) = at.split_once(':') else {
        return String::new();
    };
    let (Ok(h), Ok(m)) = (h.parse::<u32>(), m.parse::<u32>()) else {
        return String::new();
    };
    let end = (h * 60 + m + minutes) % (24 * 60);
    format!("{:02}:{:02}", end / 60, end % 60)
}

fn checklist(p: &mut Pen, notes: &TaskNotes, focus: Option<&InspectorRow>, theme: &Theme) {
    // Nothing to show and nothing being added: no section at all.
    if notes.items.is_empty() && focus.is_none() {
        return;
    }
    let heading = p.bg.fg(theme.dim);
    match notes.progress() {
        Some((done, total)) => p.text(0, &format!("CHECKLIST · {done}/{total}"), heading),
        None => p.text(0, "CHECKLIST", heading),
    };
    p.y += 1;
    for (i, item) in notes.items.iter().enumerate() {
        let bg = p.cursor(focus == Some(&InspectorRow::Item(i)), theme);
        let (glyph, color) = if item.done {
            ("☑", theme.ok)
        } else {
            ("☐", theme.dim)
        };
        p.text(0, glyph, bg.fg(color));
        let top = p.y;
        let width = usize::from(p.r.width.saturating_sub(2)).max(4);
        for (k, line) in wrap(&item.text, width).iter().enumerate() {
            if k > 0 {
                p.y += 1;
                // A long item's cursor shade runs down its wrapped rows.
                if focus == Some(&InspectorRow::Item(i)) {
                    p.cursor(true, theme);
                }
            }
            p.text(2, line, bg.fg(theme.fg));
        }
        let row = Rect {
            y: top,
            height: p.y - top + 1,
            ..p.r
        };
        p.marks.push((row, crate::app::Hit::CheckItem(i)));
        p.y += 1;
    }
    // Adding is a key away once the inspector has the keyboard.
    if focus.is_some() {
        let here = focus == Some(&InspectorRow::AddItem);
        let bg = p.cursor(here, theme);
        p.text(
            0,
            "+ add item",
            bg.fg(if here { theme.accent } else { theme.dim }),
        );
        p.y += 1;
    }
    p.y += 1;
}

fn note_cards(
    p: &mut Pen,
    t: &Task,
    notes: &TaskNotes,
    focus: Option<&InspectorRow>,
    theme: &Theme,
) {
    for line in &t.notes {
        p.text(0, line, p.bg.fg(theme.status_fg));
        p.y += 1;
    }
    let w = p.r.width;
    let inner = usize::from(w.saturating_sub(4));
    let line = "─".repeat(usize::from(w.saturating_sub(2)));
    // A card: the list's own shade, a hairline round it, the title and a
    // couple of lines of what's in it.
    let card = Style::default().bg(theme.bg);
    for (i, n) in notes.notes.iter().enumerate() {
        let preview = wrap(&n.preview, inner);
        let lines: Vec<&String> = preview.iter().filter(|l| !l.is_empty()).take(2).collect();
        let h = 3 + lines.len() as u16;
        if p.y + h > p.bottom() {
            let left = notes.notes.len() - i;
            p.text(0, &format!("+{left} more"), p.bg.fg(theme.dim));
            break;
        }
        let here = focus == Some(&InspectorRow::Note(i));
        let border = card.fg(if here { theme.accent } else { theme.border });
        p.marks.push((
            Rect {
                y: p.y,
                height: h,
                ..p.r
            },
            crate::app::Hit::NoteCard(i),
        ));
        p.text(0, &format!("╭{line}╮"), border);
        p.y += 1;
        let body_row = |p: &mut Pen| {
            p.text(
                0,
                &format!("│{}│", " ".repeat(usize::from(w.saturating_sub(2)))),
                border,
            );
        };
        body_row(p);
        let title = format!("≡ {}", n.title);
        p.text(
            2,
            &fit(&title, inner),
            card.fg(theme.fg).add_modifier(Modifier::BOLD),
        );
        p.y += 1;
        for l in &lines {
            body_row(p);
            p.text(2, &fit(l, inner), card.fg(theme.status_fg));
            p.y += 1;
        }
        p.text(0, &format!("╰{line}╯"), border);
        p.y += 2;
    }
}

fn fit(s: &str, w: usize) -> String {
    if s.chars().count() <= w {
        return s.to_string();
    }
    if w == 0 {
        return String::new();
    }
    s.chars().take(w - 1).collect::<String>() + "…"
}

/// Wrap `s` on spaces to lines of at most `width` characters.
fn wrap(s: &str, width: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    for word in s.split_whitespace() {
        let need = cur.chars().count() + usize::from(!cur.is_empty()) + word.chars().count();
        if need > width && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(word);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

/// The checklist prompt: the task, what's on its list already, and a line
/// to type the next item on. It stays open for item after item.
pub fn render_checklist_prompt(frame: &mut Frame, screen: Rect, app: &App) {
    let theme = app.theme();
    let Some(t) = app.cur_task() else {
        return;
    };
    let notes = app.task_notes(t);
    let shown: Vec<&crate::app::CheckItem> = notes
        .items
        .iter()
        .rev()
        .take(8)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let w = 64.min(screen.width.saturating_sub(4));
    // A long item wraps onto more rows instead of running off the box.
    let input_w = usize::from(w.saturating_sub(8)).max(4);
    let input_rows = wrap_input(app.draft.text(), app.draft.cursor(), input_w);
    let h = (7 + shown.len() as u16 + input_rows.len() as u16).min(screen.height.saturating_sub(2));
    let r = super::centered_in(screen, w, h);
    frame.render_widget(ratatui::widgets::Clear, r);
    let buf = frame.buffer_mut();
    let bg = Style::default().bg(theme.panel);
    for y in r.top()..r.bottom() {
        for x in r.left()..r.right() {
            if let Some(c) = buf.cell_mut((x, y)) {
                let last_x = x == r.right() - 1;
                let last_y = y == r.bottom() - 1;
                let sym = match (y == r.top(), last_y, x == r.left(), last_x) {
                    (true, _, true, _) => "╭",
                    (true, _, _, true) => "╮",
                    (_, true, true, _) => "╰",
                    (_, true, _, true) => "╯",
                    (true, ..) | (_, true, ..) => "─",
                    (_, _, true, _) | (_, _, _, true) => "│",
                    _ => " ",
                };
                c.set_symbol(sym);
                c.set_style(bg.fg(theme.accent));
            }
        }
    }
    let mut p = Pen::new(
        buf,
        Rect {
            x: r.x + 3,
            y: r.y + 1,
            width: r.width.saturating_sub(6),
            height: r.height.saturating_sub(2),
        },
        bg,
    );
    p.text(
        0,
        "☐ ADD TO CHECKLIST",
        bg.fg(theme.accent).add_modifier(Modifier::BOLD),
    );
    p.y += 1;
    let title = crate::todo::body_only(&t.raw);
    p.text(0, &fit(&title, usize::from(p.r.width)), bg.fg(theme.dim));
    p.y += 2;
    for item in &shown {
        let (g, c) = if item.done {
            ("☑", theme.ok)
        } else {
            ("☐", theme.dim)
        };
        p.text(0, g, bg.fg(c));
        p.text(
            2,
            &fit(&item.text, usize::from(p.r.width) - 2),
            bg.fg(theme.fg),
        );
        p.y += 1;
    }
    // The line you're typing on.
    p.text(0, "☐", bg.fg(theme.accent));
    let x = p.r.x + 2;
    for (row, cursor) in &input_rows {
        if p.y >= p.r.bottom() {
            break;
        }
        let mut spans = Vec::new();
        for (i, c) in row.chars().enumerate() {
            let style = if *cursor == Some(i) {
                Style::default().fg(theme.panel).bg(theme.fg)
            } else {
                Style::default().fg(theme.fg)
            };
            spans.push(ratatui::text::Span::styled(c.to_string(), style));
        }
        if *cursor == Some(row.chars().count()) {
            spans.push(ratatui::text::Span::styled(
                "█",
                Style::default().fg(theme.fg),
            ));
        }
        let line = ratatui::text::Line::from(spans);
        let y = p.y;
        p.buf.set_line(x, y, &line, p.r.width.saturating_sub(2));
        p.y += 1;
    }
    p.y += 1;
    p.text(0, "Enter add · paste a list · Esc done", bg.fg(theme.dim));
}

/// `text` cut into rows of `width` chars (after spaces where it can), each
/// with the cursor's column when it's on that row.
fn wrap_input(text: &str, cursor: usize, width: usize) -> Vec<(String, Option<usize>)> {
    let chars: Vec<char> = text.chars().collect();
    let cursor = text[..cursor.min(text.len())].chars().count();
    let mut rows = Vec::new();
    let mut start = 0;
    while chars.len() - start > width {
        let mut end = start + width;
        if let Some(sp) = (start + 1..end).rev().find(|&i| chars[i - 1] == ' ') {
            end = sp;
        }
        rows.push((start, end));
        start = end;
    }
    rows.push((start, chars.len()));
    let last = rows.len() - 1;
    let mut out: Vec<(String, Option<usize>)> = rows
        .iter()
        .enumerate()
        .map(|(n, &(a, b))| {
            let here = cursor >= a && (cursor < b || (n == last && cursor == b));
            (chars[a..b].iter().collect(), here.then(|| cursor - a))
        })
        .collect();
    // The cursor at the end of a full row starts the next one.
    if let Some((row, Some(col))) = out.last()
        && *col >= width
        && row.chars().count() >= width
    {
        if let Some(r) = out.last_mut() {
            r.1 = None;
        }
        out.push((String::new(), Some(0)));
    }
    out
}

/// "Every step is done — is the task?": a small box over everything, `y`
/// or `Enter` marks it done, any other key leaves it.
/// "Only this one, or this and the ones after?" for a change to a
/// repeating task.
pub fn render_series_ask(frame: &mut Frame, screen: Rect, app: &App, ask: &crate::app::SeriesAsk) {
    use crate::app::SeriesOp;
    let theme = app.theme();
    let title = app
        .tasks()
        .get(ask.occ.abs)
        .map(|t| crate::todo::body_only(&t.raw))
        .unwrap_or_default();
    let verb = match ask.op {
        SeriesOp::Edit => "Edit",
        SeriesOp::Delete => "Delete",
        SeriesOp::Resize(_) => "Change the length of",
        _ => "Move",
    };
    let day = ask
        .occ
        .origin
        .format("%a %-d %b")
        .to_string()
        .to_lowercase();
    let w = 56.min(screen.width.saturating_sub(4));
    let r = Rect {
        x: screen.x + (screen.width.saturating_sub(w)) / 2,
        y: screen.y + screen.height.saturating_sub(8) / 2,
        width: w,
        height: 7.min(screen.height),
    };
    frame.render_widget(ratatui::widgets::Clear, r);
    let block = ratatui::widgets::Block::bordered()
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(Style::default().fg(theme.accent))
        .style(Style::default().bg(theme.panel));
    let inner = block.inner(r);
    frame.render_widget(block, r);
    let key = |k: &'static str| {
        ratatui::text::Span::styled(
            k,
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        )
    };
    let text = |t: String| ratatui::text::Span::styled(t, Style::default().fg(theme.fg));
    let lines = vec![
        ratatui::text::Line::styled(
            fit(
                &format!("{verb} \u{201c}{title}\u{201d}"),
                usize::from(inner.width),
            ),
            Style::default().fg(theme.fg).add_modifier(Modifier::BOLD),
        ),
        ratatui::text::Line::styled("It repeats.", Style::default().fg(theme.dim)),
        ratatui::text::Line::raw(""),
        ratatui::text::Line::from(vec![key("o "), text(format!("only this one ({day})"))]),
        ratatui::text::Line::from(vec![
            key("f "),
            text("this one and the ones after".into()),
            ratatui::text::Span::styled("   Esc cancel", Style::default().fg(theme.dim)),
        ]),
    ];
    frame.render_widget(
        ratatui::widgets::Paragraph::new(lines).style(Style::default().bg(theme.panel)),
        inner,
    );
}

pub fn render_confirm_done(frame: &mut Frame, screen: Rect, app: &App, abs: usize) {
    let theme = app.theme();
    let title = app
        .tasks()
        .get(abs)
        .map(|t| crate::todo::body_only(&t.raw))
        .unwrap_or_default();
    let w = 52.min(screen.width.saturating_sub(4));
    let r = Rect {
        x: screen.x + (screen.width.saturating_sub(w)) / 2,
        y: screen.y + screen.height.saturating_sub(7) / 2,
        width: w,
        height: 6.min(screen.height),
    };
    frame.render_widget(ratatui::widgets::Clear, r);
    let block = ratatui::widgets::Block::bordered()
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(Style::default().fg(theme.accent))
        .style(Style::default().bg(theme.panel));
    let inner = block.inner(r);
    frame.render_widget(block, r);
    let lines = vec![
        ratatui::text::Line::styled(
            "Every step is done.",
            Style::default().fg(theme.ok).add_modifier(Modifier::BOLD),
        ),
        ratatui::text::Line::styled(
            fit(
                &format!("Mark \u{201c}{title}\u{201d} done too?"),
                usize::from(inner.width),
            ),
            Style::default().fg(theme.fg),
        ),
        ratatui::text::Line::raw(""),
        ratatui::text::Line::styled(
            "y / Enter  done · any other key  not yet",
            Style::default().fg(theme.dim),
        ),
    ];
    frame.render_widget(
        ratatui::widgets::Paragraph::new(lines).style(Style::default().bg(theme.panel)),
        inner,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_block_ends_when_its_duration_says() {
        assert_eq!(end_time("09:00", 120), "11:00");
        assert_eq!(end_time("23:30", 60), "00:30");
        assert_eq!(end_time("x", 60), "");
    }

    #[test]
    #[allow(clippy::unwrap_used)]
    fn a_long_checklist_scrolls_to_follow_the_cursor() {
        use crate::app::test_support::build_app_with_config;
        use ratatui::{Terminal, backend::TestBackend};
        let dir = std::env::temp_dir().join(format!(
            "tasq-inspector-scroll-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let cfg = crate::config::Config {
            notes_dir: Some(dir.to_string_lossy().into_owned()),
            ..Default::default()
        };
        let mut app = build_app_with_config("Backlog +work\n", cfg);
        app.inspector_focus_on();
        for i in 0..40 {
            app.add_check_item(&format!("item number {i}"));
        }
        let screen = |app: &App| {
            let mut term = Terminal::new(TestBackend::new(120, 24)).unwrap();
            term.draw(|f| crate::ui::draw(f, app)).unwrap();
            let buf = term.backend().buffer().clone();
            (0..buf.area.height)
                .map(|y| {
                    (0..buf.area.width)
                        .map(|x| buf[(x, y)].symbol().to_string())
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        app.inspector_cursor = 0;
        let top = screen(&app);
        assert!(top.contains("item number 0"), "{top}");
        assert!(!top.contains("item number 39"), "{top}");
        for _ in 0..39 {
            app.inspector_move(true);
            screen(&app);
        }
        let bottom = screen(&app);
        assert!(bottom.contains("item number 39"), "{bottom}");
        assert!(!bottom.contains("item number 0 "), "{bottom}");
        // Back up again, and the top comes back into view.
        for _ in 0..39 {
            app.inspector_move(false);
            screen(&app);
        }
        assert!(screen(&app).contains("item number 0"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_long_checklist_item_wraps_as_you_type() {
        let rows = wrap_input("buy milk and eggs", 17, 10);
        assert_eq!(rows[0].0, "buy milk ");
        assert_eq!(rows[1], ("and eggs".to_string(), Some(8)));
        assert_eq!(wrap_input("", 0, 10), vec![(String::new(), Some(0))]);
        // A full row with the cursor at its end: the cursor starts a new row.
        let rows = wrap_input("abcd", 4, 4);
        assert_eq!(rows.last(), Some(&(String::new(), Some(0))));
    }

    #[test]
    fn titles_wrap_on_words() {
        assert_eq!(wrap("Teoría AII y más", 9), vec!["Teoría", "AII y más"]);
        assert_eq!(wrap("", 9), vec![String::new()]);
    }
}
