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
    let mut p = Pen {
        buf,
        r: inner,
        y: inner.y,
        bg: Style::default().bg(ibg),
        marks: Vec::new(),
    };
    let Some(t) = app.cur_task() else {
        p.text(0, "no task selected", p.bg.fg(theme.dim));
        return;
    };
    title(&mut p, t, theme);
    p.y += 1;
    facts(&mut p, t, app, theme);
    let notes = app.task_notes(t);
    p.y += 1;
    let focus = app
        .inspector_focus
        .then(|| app.inspector_current())
        .flatten();
    checklist(&mut p, &notes, focus.as_ref(), theme);
    note_cards(&mut p, t, &notes, focus.as_ref(), theme);
    let used = p.y;
    // In Today, your day at a glance at the foot of the column.
    if app.prefs.scope == crate::app::Scope::Today && app.calendar.is_none() {
        your_day(p.buf, inner, used, app, theme, ibg);
    }
    for (r, h) in p.marks {
        app.hits.add(r, h);
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
}

impl Pen<'_> {
    fn room(&self) -> bool {
        self.y < self.r.bottom()
    }

    /// Write `s` at column `dx` of the current row; returns its width.
    fn text(&mut self, dx: u16, s: &str, style: Style) -> u16 {
        if !self.room() || dx >= self.r.width {
            return 0;
        }
        let x = self.r.x + dx;
        let max = usize::from(self.r.width - dx);
        let (end, _) = self.buf.set_stringn(x, self.y, s, max, style);
        end.saturating_sub(x)
    }

    /// Paint the current row (and a column either side) in `bg`.
    fn fill(&mut self, bg: Color) {
        if !self.room() {
            return;
        }
        let right = (self.r.right() + 1).min(self.buf.area.right());
        for x in self.r.x.saturating_sub(1)..right {
            if let Some(c) = self.buf.cell_mut((x, self.y)) {
                c.set_symbol(" ");
                c.set_bg(bg);
            }
        }
    }

    /// The `▎` marker left of the focused row.
    fn bar(&mut self, theme: &Theme) {
        if !self.room() || self.r.x < 2 {
            return;
        }
        if let Some(c) = self.buf.cell_mut((self.r.x - 2, self.y)) {
            c.set_symbol("▎");
            c.set_fg(theme.accent);
        }
    }

    /// Highlight the current row as the cursor; returns its style.
    fn cursor(&mut self, here: bool, theme: &Theme) -> Style {
        if !here {
            return self.bg;
        }
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
    p.text(KEY_W, value, p.bg.fg(color));
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
        p.text(KEY_W + 2, &crate::core::spaces::display(s), p.bg.fg(color));
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
        p.text(2, &item.text, bg.fg(theme.fg));
        let row = Rect {
            y: p.y,
            height: 1,
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
        if p.y + h > p.r.bottom() {
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
    let h = (8 + shown.len() as u16).min(screen.height.saturating_sub(2));
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
    let mut p = Pen {
        buf,
        r: Rect {
            x: r.x + 3,
            y: r.y + 1,
            width: r.width.saturating_sub(6),
            height: r.height.saturating_sub(2),
        },
        y: r.y + 1,
        bg,
        marks: Vec::new(),
    };
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
    let y = p.y;
    let spans = crate::ui::dialog::draft_cursor_spans(
        app.draft.text(),
        app.draft.cursor(),
        theme.fg,
        theme.panel,
    );
    let line = ratatui::text::Line::from(spans);
    p.buf.set_line(x, y, &line, p.r.width.saturating_sub(2));
    p.y += 2;
    p.text(0, "Enter add · paste a list · Esc done", bg.fg(theme.dim));
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
    fn titles_wrap_on_words() {
        assert_eq!(wrap("Teoría AII y más", 9), vec!["Teoría", "AII y más"]);
        assert_eq!(wrap("", 9), vec![String::new()]);
    }
}
