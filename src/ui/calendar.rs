//! The calendar screen: a bar of view tabs and the date, then the day,
//! week or month view.
//!
//! Drawn cell by cell on the buffer: blocks are filled rectangles tinted in
//! their space's colour, with a stronger edge on the left — dashed for a
//! future repeat — the way the design mock-up draws them.

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use chrono::Datelike;

use crate::app::{App, CalView};
use crate::core::calendar::{self, Occurrence};
use crate::theme::Theme;
use crate::ui::task_row::{chip_date, tint};

/// Width of the day view's side panel, when there's room for it.
const SIDE_W: u16 = 32;
/// Width of the hour labels.
const GUTTER_W: u16 = 8;
/// The hours always shown, widened to fit earlier or later blocks.
const DAY_FROM: u32 = 7;
const DAY_TO: u32 = 22;

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let theme = app.theme();
    let buf = frame.buffer_mut();
    fill(buf, area, Style::default().bg(theme.bg));
    let Some(cal) = &app.calendar else {
        return;
    };
    app.cal_cols.borrow_mut().clear();
    if area.height < 4 || area.width < 30 {
        return;
    }
    bar(buf, Rect { height: 1, ..area }, app, theme);
    hline(
        buf,
        area.x,
        area.y + 1,
        area.width,
        Style::default().fg(theme.border),
    );
    let body = Rect {
        y: area.y + 2,
        height: area.height - 2,
        ..area
    };
    match cal.view {
        CalView::Day => day(buf, body, app, theme),
        CalView::Week => {
            // The blocks need about ten columns a day; narrower, the agenda.
            let blocks = cal.week_style == crate::app::CalStyle::Blocks && body.width >= 80;
            if blocks {
                week_blocks(buf, body, app, theme);
            } else {
                week_agenda(buf, body, app, theme);
            }
        }
        CalView::Month => month(buf, body, app, theme),
    }
}

// ---------------------------------------------------------------------------
// Drawing helpers
// ---------------------------------------------------------------------------

fn fill(buf: &mut Buffer, r: Rect, style: Style) {
    let r = r.intersection(buf.area);
    for y in r.top()..r.bottom() {
        for x in r.left()..r.right() {
            if let Some(c) = buf.cell_mut((x, y)) {
                c.set_symbol(" ");
                c.set_style(style);
            }
        }
    }
}

fn hline(buf: &mut Buffer, x: u16, y: u16, w: u16, style: Style) {
    for i in 0..w {
        if let Some(c) = buf.cell_mut((x + i, y)) {
            c.set_symbol("─");
            c.set_style(style);
        }
    }
}

/// Write `s` at `(x, y)`, at most `max` columns; returns the columns used.
fn put(buf: &mut Buffer, x: u16, y: u16, s: &str, max: u16, style: Style) -> u16 {
    if max == 0 || !buf.area.contains((x, y).into()) {
        return 0;
    }
    let (end, _) = buf.set_stringn(x, y, s, usize::from(max), style);
    end.saturating_sub(x)
}

/// `s` cut to `w` columns with an ellipsis when it doesn't fit.
fn fit(s: &str, w: usize) -> String {
    if s.chars().count() <= w {
        return s.to_string();
    }
    if w == 0 {
        return String::new();
    }
    let mut out: String = s.chars().take(w - 1).collect();
    out.push('…');
    out
}

fn hhmm(m: u32) -> String {
    format!("{:02}:{:02}", m / 60, m % 60)
}

fn minutes_label(m: u32) -> String {
    match (m / 60, m % 60) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h {m}m"),
    }
}

/// The colour a task is painted in: its space's, else the accent.
fn task_color(app: &App, abs: usize) -> Color {
    app.tasks()
        .get(abs)
        .and_then(|t| t.projects.first())
        .map_or(app.theme().accent, |p| app.space_color(p))
}

/// The task's title: its words without tags, keys or priority.
fn title(app: &App, abs: usize) -> String {
    let Some(t) = app.tasks().get(abs) else {
        return String::new();
    };
    crate::todo::body_after_priority(&t.clean_raw)
        .split_whitespace()
        .filter(|w| {
            let tag = w.starts_with('+') || w.starts_with('@') || crate::todo::is_star_token(w);
            let key = w.split_once(':').is_some_and(|(k, v)| {
                !k.is_empty() && !v.is_empty() && k.chars().all(|c| c.is_ascii_lowercase())
            });
            !tag && !key
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// An occurrence's title: a diamond for an event, and which day it is of
/// one lasting several.
fn occ_title(app: &App, o: &Occurrence) -> String {
    let mut t = title(app, o.abs);
    if app.tasks().get(o.abs).is_some_and(|t| t.event) {
        t = format!("◆ {t}");
    }
    if o.span > 1 {
        t.push_str(&format!(" · {}/{}", o.day + 1, o.span));
    }
    t
}

/// An all-day item across a run of day columns: one day, or the visible
/// part of something lasting several (`more_before` / `more_after` when it
/// goes on past the columns).
struct Band<'a> {
    occ: &'a Occurrence,
    c0: usize,
    c1: usize,
    more_before: bool,
    more_after: bool,
    row: usize,
}

/// The all-day items of `days` as bands, each on the first row free across
/// its days: the ones lasting several days first, longest first, so they
/// run straight across.
fn all_day_bands<'a>(occs: &[&'a Occurrence], days: &[chrono::NaiveDate]) -> Vec<Band<'a>> {
    let col = |d: chrono::NaiveDate| days.iter().position(|x| *x == d);
    let mut bands: Vec<Band> = Vec::new();
    let mut seen: Vec<(usize, chrono::NaiveDate, bool)> = Vec::new();
    for o in occs.iter().copied().filter(|o| o.start.is_none()) {
        let Some(c) = col(o.date) else {
            continue;
        };
        if o.span > 1 {
            let key = (o.abs, o.origin, o.projected);
            if seen.contains(&key) {
                continue;
            }
            seen.push(key);
            let parts: Vec<&Occurrence> = occs
                .iter()
                .copied()
                .filter(|p| (p.abs, p.origin, p.projected) == key && p.start.is_none())
                .collect();
            let c0 = parts.iter().filter_map(|p| col(p.date)).min().unwrap_or(c);
            let c1 = parts.iter().filter_map(|p| col(p.date)).max().unwrap_or(c);
            let first = parts.iter().map(|p| p.day).min().unwrap_or(0);
            let last = parts.iter().map(|p| p.day).max().unwrap_or(0);
            // The band stands for its first visible day.
            let occ = parts
                .iter()
                .copied()
                .find(|p| col(p.date) == Some(c0))
                .unwrap_or(o);
            bands.push(Band {
                occ,
                c0,
                c1,
                more_before: first > 0,
                more_after: last + 1 < o.span,
                row: 0,
            });
        } else {
            bands.push(Band {
                occ: o,
                c0: c,
                c1: c,
                more_before: false,
                more_after: false,
                row: 0,
            });
        }
    }
    let mut order: Vec<usize> = (0..bands.len()).collect();
    order.sort_by_key(|&i| {
        let b = &bands[i];
        (b.c0 == b.c1, b.c0, std::cmp::Reverse(b.c1 - b.c0))
    });
    let mut taken: Vec<Vec<bool>> = Vec::new();
    for i in order {
        let (c0, c1) = (bands[i].c0, bands[i].c1);
        let row = (0..)
            .find(|&r| {
                taken
                    .get(r)
                    .is_none_or(|cols: &Vec<bool>| !cols[c0..=c1].iter().any(|x| *x))
            })
            .unwrap_or(0);
        while taken.len() <= row {
            taken.push(vec![false; days.len()]);
        }
        for x in &mut taken[row][c0..=c1] {
            *x = true;
        }
        bands[i].row = row;
    }
    bands
}

/// Draw `bands` in `rows` rows from `y`, column `c` at `x_of(c)` and
/// `w_of(c)` wide; a column with more than fits says how many more in its
/// last row.
#[allow(clippy::too_many_arguments)]
fn draw_bands(
    buf: &mut Buffer,
    app: &App,
    theme: &Theme,
    bands: &[Band],
    y: u16,
    rows: usize,
    x_of: &dyn Fn(usize) -> u16,
    w_of: &dyn Fn(usize) -> u16,
    selected: Option<&Occurrence>,
    cols: usize,
) {
    if rows == 0 {
        return;
    }
    let hidden = |c: usize| {
        bands
            .iter()
            .filter(|b| b.c0 <= c && c <= b.c1 && b.row >= rows)
            .count()
    };
    for b in bands {
        if b.row >= rows {
            continue;
        }
        // The last row of a column with more hidden says "+n more".
        let lose_last = (b.c0..=b.c1).any(|c| hidden(c) > 0) && b.row + 1 == rows;
        if lose_last && b.c0 == b.c1 {
            continue;
        }
        let x = x_of(b.c0) + 1;
        let right = x_of(b.c1) + 1 + w_of(b.c1);
        let w = right.saturating_sub(x);
        let color = occ_color(app, theme, b.occ);
        let sel = selected.is_some_and(|s| {
            s.abs == b.occ.abs && s.origin == b.occ.origin && s.projected == b.occ.projected
        });
        let mut style = Style::default().fg(color);
        if let Some(bg) = tint(color, theme.bg, if sel { 0.42 } else { 0.22 }) {
            style = style.bg(bg);
        }
        if sel {
            style = style.add_modifier(Modifier::BOLD);
        }
        let o = b.occ;
        let prefix = if o.late {
            "late "
        } else if o.deadline {
            "◷ "
        } else if o.projected && o.span == 1 {
            "↻ "
        } else {
            ""
        };
        let name = if o.span > 1 {
            let t = title(app, o.abs);
            if app.tasks().get(o.abs).is_some_and(|t| t.event) {
                format!("◆ {t}")
            } else {
                t
            }
        } else {
            occ_title(app, o)
        };
        let left = if b.more_before { "◂ " } else { " " };
        let tail = if b.more_after { "▸" } else { "" };
        let inner = usize::from(w).saturating_sub(tail.chars().count());
        let text = fit(&format!("{left}{prefix}{name}"), inner);
        let text = format!("{text:<inner$}{tail}");
        put(buf, x, y + b.row as u16, &text, w, style);
    }
    for c in 0..cols {
        let n = hidden(c);
        if n == 0 {
            continue;
        }
        // The ones in the last row are hidden too.
        let n = n + bands
            .iter()
            .filter(|b| b.c0 == b.c1 && b.c0 == c && b.row + 1 == rows)
            .count();
        let x = x_of(c) + 1;
        let w = w_of(c).saturating_sub(1);
        put(
            buf,
            x,
            y + rows as u16 - 1,
            &format!("{:<w$}", format!("+{n} more"), w = usize::from(w)),
            w,
            Style::default().fg(theme.dim),
        );
    }
}

// ---------------------------------------------------------------------------
// The bar: view tabs, the date, a summary
// ---------------------------------------------------------------------------

fn bar(buf: &mut Buffer, r: Rect, app: &App, theme: &Theme) {
    let Some(cal) = &app.calendar else {
        return;
    };
    let mut x = r.x + 1;
    for (view, key, name) in [
        (CalView::Day, "d", "Day"),
        (CalView::Week, "w", "Week"),
        (CalView::Month, "m", "Month"),
    ] {
        let on = cal.view == view;
        let (key_style, name_style) = if on {
            let s = Style::default()
                .bg(theme.mode_bg)
                .fg(theme.mode_fg)
                .add_modifier(Modifier::BOLD);
            (s, s)
        } else {
            (
                Style::default().fg(theme.dim),
                Style::default().fg(theme.dim),
            )
        };
        x += put(buf, x, r.y, &format!(" {key} "), 3, key_style);
        x += put(buf, x, r.y, &format!("{name} "), 8, name_style);
        x += 1;
    }
    x += 2;
    let label = match cal.view {
        CalView::Day => cal.date.format("%A %-d %B").to_string().to_lowercase(),
        CalView::Week => {
            let from = crate::app::week_start(cal.date);
            let to = from + chrono::Days::new(6);
            format!("{} – {}", from.format("%-d %b"), to.format("%-d %b %Y")).to_lowercase()
        }
        CalView::Month => cal.date.format("%B %Y").to_string().to_lowercase(),
    };
    x += put(buf, x, r.y, "‹ ", 2, Style::default().fg(theme.dim));
    x += put(
        buf,
        x,
        r.y,
        &label,
        r.right().saturating_sub(x),
        Style::default().fg(theme.fg).add_modifier(Modifier::BOLD),
    );
    put(buf, x, r.y, " ›", 2, Style::default().fg(theme.dim));

    // Summary on the right.
    let summary = match cal.view {
        CalView::Day => {
            let items = app.cal_day_items();
            let planned: u32 = items
                .iter()
                .filter(|o| o.start.is_some())
                .map(|o| o.minutes)
                .sum();
            let n = items.len();
            let tasks = if n == 1 { "task" } else { "tasks" };
            if planned > 0 {
                format!("{n} {tasks} · {} planned", minutes_label(planned))
            } else {
                format!("{n} {tasks}")
            }
        }
        CalView::Week => {
            let from = crate::app::week_start(cal.date);
            let items = app.cal_occurrences(from, from + chrono::Days::new(6));
            let planned: u32 = items
                .iter()
                .filter(|o| o.start.is_some())
                .map(|o| o.minutes)
                .sum();
            let n = items.len();
            let tasks = if n == 1 { "task" } else { "tasks" };
            if planned > 0 {
                format!("{n} {tasks} · {} planned", minutes_label(planned))
            } else {
                format!("{n} {tasks}")
            }
        }
        CalView::Month => {
            let (from, to) = crate::app::month_bounds(cal.date);
            let n = app.cal_occurrences(from, to).len();
            format!("{n} {}", if n == 1 { "task" } else { "tasks" })
        }
    };
    let w = summary.chars().count() as u16;
    if r.right() > x + w + 4 {
        put(
            buf,
            r.right() - w - 1,
            r.y,
            &summary,
            w,
            Style::default().fg(theme.dim),
        );
    }
}

// ---------------------------------------------------------------------------
// Day: time blocks
// ---------------------------------------------------------------------------

fn day(buf: &mut Buffer, r: Rect, app: &App, theme: &Theme) {
    let Some(cal) = &app.calendar else {
        return;
    };
    let items = app.cal_day_items();
    let selected = items.get(cal.selected).cloned();
    let side = r.width >= 90;
    let grid_r = Rect {
        width: if side { r.width - SIDE_W } else { r.width },
        ..r
    };
    if side {
        let sr = Rect {
            x: r.right() - SIDE_W,
            width: SIDE_W,
            ..r
        };
        side_panel(buf, sr, app, theme, &items, selected.as_ref());
    }

    // All-day band: chips that wrap, then a dashed rule.
    let all_day: Vec<(usize, &Occurrence)> = items
        .iter()
        .enumerate()
        .filter(|(_, o)| o.start.is_none())
        .collect();
    let mut y = grid_r.y;
    if !all_day.is_empty() {
        put(
            buf,
            grid_r.x,
            y,
            &format!("{:>w$} ", "all day", w = usize::from(GUTTER_W) - 1),
            GUTTER_W,
            Style::default().fg(theme.dim),
        );
        let mut x = grid_r.x + GUTTER_W;
        for (i, o) in all_day {
            let (color, prefix) = if o.late {
                (theme.overdue, "late · ")
            } else if o.deadline {
                (theme.overdue, "◷ ")
            } else {
                (task_color(app, o.abs), if o.projected { "↻ " } else { "" })
            };
            let text = format!(" {prefix}{} ", occ_title(app, o));
            let w = (text.chars().count() as u16).min(grid_r.width.saturating_sub(GUTTER_W + 1));
            if x + w > grid_r.right() && x > grid_r.x + GUTTER_W {
                y += 1;
                x = grid_r.x + GUTTER_W;
            }
            let sel = Some(i) == Some(cal.selected);
            let amount = if sel { 0.42 } else { 0.22 };
            let mut style = Style::default().fg(color);
            if let Some(bg) = tint(color, theme.bg, amount) {
                style = style.bg(bg);
            }
            if sel {
                style = style.add_modifier(Modifier::BOLD);
            }
            if app.tasks().get(o.abs).is_some_and(|t| t.done) {
                style = Style::default()
                    .fg(theme.done)
                    .add_modifier(Modifier::CROSSED_OUT);
            }
            put(buf, x, y, &fit(&text, usize::from(w)), w, style);
            x += w + 1;
        }
        y += 1;
        for x in grid_r.x..grid_r.right() {
            if let Some(c) = buf.cell_mut((x, y)) {
                c.set_symbol("┄");
                c.set_style(Style::default().fg(theme.border));
            }
        }
        y += 1;
    }
    let grid = Rect {
        y,
        height: grid_r.bottom().saturating_sub(y),
        ..grid_r
    };
    if grid.height < 2 {
        return;
    }

    // Hours to show, and how many rows an hour gets.
    let timed: Vec<&Occurrence> = items.iter().filter(|o| o.start.is_some()).collect();
    let first = timed
        .iter()
        .filter_map(|o| o.start)
        .min()
        .map_or(DAY_FROM, |s| (s / 60).min(DAY_FROM));
    let now = (cal.date == app.today_naive())
        .then(|| app.now_minutes())
        .flatten();
    // Through the last block, and through now on today.
    let last = timed
        .iter()
        .filter_map(|o| o.end())
        .chain(now.map(|n| n + 60))
        .max()
        .map_or(DAY_TO, |e| e.div_ceil(60).clamp(DAY_TO, 24));
    let hours = last - first;
    // Rows per hour: as many as fill the height (up to 4), never under one;
    // with too little room, a window of hours around what matters.
    let scale = (f64::from(grid.height) / f64::from(hours.max(1))).clamp(1.0, 4.0);
    let visible_hours = ((f64::from(grid.height) / scale) as u32).max(1);
    // Scroll so the selected block (else now, else the first block) shows.
    let focus = selected
        .as_ref()
        .and_then(|o| o.start)
        .or(now)
        .or_else(|| timed.first().and_then(|o| o.start))
        .unwrap_or(first * 60);
    let top = if hours <= visible_hours {
        first
    } else {
        (focus / 60)
            .saturating_sub(visible_hours / 3)
            .clamp(first, last - visible_hours)
    };
    let row_of =
        |m: u32| -> i64 { ((f64::from(m) - f64::from(top * 60)) * scale / 60.0).floor() as i64 };

    // Grid lines and hour labels.
    let gx = grid.x + GUTTER_W;
    let gw = grid.width.saturating_sub(GUTTER_W);
    let col = crate::app::CalCol {
        x: gx,
        w: gw,
        date: cal.date,
        y: grid.y,
        h: grid.height,
        top: top * 60,
        scale,
    };
    app.cal_cols.borrow_mut().push(col);
    for h in top..last {
        let row = row_of(h * 60);
        if row < 0 || row >= i64::from(grid.height) {
            continue;
        }
        let y = grid.y + row as u16;
        put(
            buf,
            grid.x,
            y,
            &format!("{:>5}  ", hhmm(h * 60)),
            GUTTER_W,
            Style::default().fg(theme.dim),
        );
        for x in gx..gx + gw {
            if let Some(c) = buf.cell_mut((x, y)) {
                c.set_symbol("╌");
                c.set_style(Style::default().fg(theme.border));
            }
        }
    }

    // Blocks, side by side where they overlap.
    let lanes = calendar::lanes(&timed);
    for (o, (lane, n)) in timed.iter().zip(lanes) {
        let (Some(s), Some(e)) = (o.start, o.end()) else {
            continue;
        };
        let r0 = row_of(s);
        let r1 = gap_before_next(r0, row_of(e).max(r0 + 1), e, &timed, &row_of);
        let (r0, r1) = (r0.max(0), r1.min(i64::from(grid.height)));
        if r0 >= r1 {
            continue;
        }
        let lane_w = gw / n as u16;
        let bx = gx + lane_w * lane as u16;
        let bw = if lane + 1 == n {
            gw - lane_w * lane as u16
        } else {
            lane_w.saturating_sub(1)
        };
        let block = Rect {
            x: bx,
            y: grid.y + r0 as u16,
            width: bw,
            height: (r1 - r0) as u16,
        };
        let sel = selected.as_ref().is_some_and(|x| x == *o);
        draw_block(buf, block, app, theme, o, sel);
        block_hits(app, block, o);
    }
    draw_ghost(buf, app, theme, &col, gx, gw);

    // Now: a red line across the free part of the grid.
    if let Some(now) = now
        && now >= top * 60
    {
        let row = row_of(now);
        if row >= 0 && row < i64::from(grid.height) {
            let y = grid.y + row as u16;
            let red = Style::default()
                .fg(theme.overdue)
                .add_modifier(Modifier::BOLD);
            put(
                buf,
                grid.x,
                y,
                &format!("{:>5} ●", hhmm(now)),
                GUTTER_W,
                red,
            );
            for x in gx..gx + gw {
                if let Some(c) = buf.cell_mut((x, y))
                    && matches!(c.symbol(), " " | "╌")
                {
                    c.set_symbol("─");
                    c.set_style(red);
                }
            }
        }
    }
}

/// Where a block ending at `end` (rows `r0..r1`) stops: one row short of a
/// block that starts right when it ends (or in the same row, by rounding),
/// so two back-to-back tasks read as two. A one-row block keeps its row.
fn gap_before_next(
    r0: i64,
    r1: i64,
    end: u32,
    others: &[&Occurrence],
    row_of: &dyn Fn(u32) -> i64,
) -> i64 {
    let next = others
        .iter()
        .filter_map(|o| o.start)
        .filter(|&t| t >= end)
        .map(row_of)
        .filter(|&r| r <= r1)
        .min();
    match next {
        Some(r) => {
            let stop = r.min(r1).max(r0 + 1);
            if stop - r0 >= 2 { stop - 1 } else { stop }
        }
        None => r1,
    }
}

/// One time block: a tinted rectangle, a stronger left edge (dashed for a
/// future repeat), the title, then its hours and space.
fn draw_block(buf: &mut Buffer, r: Rect, app: &App, theme: &Theme, o: &Occurrence, sel: bool) {
    let done = app.tasks().get(o.abs).is_some_and(|t| t.done);
    let color = if o.deadline {
        theme.overdue
    } else {
        task_color(app, o.abs)
    };
    let amount = if sel { 0.40 } else { 0.20 };
    let bg = if done {
        theme.panel
    } else {
        tint(color, theme.bg, amount).unwrap_or(theme.panel)
    };
    fill(buf, r, Style::default().bg(bg));
    let edge = if o.projected {
        "┆"
    } else if sel {
        "┃"
    } else {
        "▎"
    };
    for y in r.top()..r.bottom() {
        if let Some(c) = buf.cell_mut((r.x, y)) {
            c.set_symbol(edge);
            c.set_style(
                Style::default()
                    .fg(if done { theme.done } else { color })
                    .bg(bg),
            );
        }
    }
    let w = r.width.saturating_sub(2);
    let mut head = String::new();
    if o.projected {
        head.push_str("↻ ");
    }
    if let Some(p) = app.tasks().get(o.abs).and_then(|t| t.priority) {
        head.push_str(&format!("({p}) "));
    }
    head.push_str(&occ_title(app, o));
    let title_style = if done {
        Style::default()
            .fg(theme.done)
            .bg(bg)
            .add_modifier(Modifier::CROSSED_OUT)
    } else {
        Style::default()
            .fg(theme.fg)
            .bg(bg)
            .add_modifier(Modifier::BOLD)
    };
    let (Some(s), Some(e)) = (o.start, o.end()) else {
        return;
    };
    let hours = format!("{} – {}", hhmm(s), hhmm(e));
    let space = app
        .tasks()
        .get(o.abs)
        .and_then(|t| t.projects.first())
        .map(|p| crate::core::spaces::display(p));
    if r.height == 1 {
        // One row: title and start time on the same line.
        let line = format!(
            "{} · {}",
            fit(&head, usize::from(w).saturating_sub(8)),
            hhmm(s)
        );
        put(buf, r.x + 2, r.y, &line, w, title_style);
        return;
    }
    put(
        buf,
        r.x + 2,
        r.y,
        &fit(&head, usize::from(w)),
        w,
        title_style,
    );
    let mut detail = hours;
    if let Some(sp) = space {
        detail.push_str(" · ");
        detail.push_str(&sp);
    }
    let detail_style = Style::default()
        .fg(if done { theme.done } else { color })
        .bg(bg);
    put(
        buf,
        r.x + 2,
        r.y + 1,
        &fit(&detail, usize::from(w)),
        w,
        detail_style,
    );
}

fn side_panel(
    buf: &mut Buffer,
    r: Rect,
    app: &App,
    theme: &Theme,
    items: &[Occurrence],
    selected: Option<&Occurrence>,
) {
    fill(buf, r, Style::default().bg(theme.panel));
    for y in r.top()..r.bottom() {
        if let Some(c) = buf.cell_mut((r.x, y)) {
            c.set_symbol("│");
            c.set_style(Style::default().fg(theme.border).bg(theme.panel));
        }
    }
    let x = r.x + 2;
    let w = r.width.saturating_sub(3);
    let head = Style::default()
        .fg(theme.dim)
        .bg(theme.panel)
        .add_modifier(Modifier::BOLD);
    let key = Style::default().fg(theme.dim).bg(theme.panel);
    let val = Style::default().fg(theme.fg).bg(theme.panel);
    let mut y = r.y + 1;
    let row = |buf: &mut Buffer, y: &mut u16, k: &str, v: &str, style: Style| {
        if *y >= r.bottom() {
            return;
        }
        put(buf, x, *y, &format!("{k:<9}"), 9, key);
        put(
            buf,
            x + 9,
            *y,
            &fit(v, usize::from(w.saturating_sub(9))),
            w.saturating_sub(9),
            style,
        );
        *y += 1;
    };

    put(buf, x, y, "SELECTED", w, head);
    y += 1;
    match selected.and_then(|o| Some((o, app.tasks().get(o.abs)?))) {
        Some((o, t)) => {
            let today = app.today_naive().format("%Y-%m-%d").to_string();
            row(
                buf,
                &mut y,
                "task",
                &occ_title(app, o),
                val.add_modifier(Modifier::BOLD),
            );
            let mut when = chip_date(&o.date.format("%Y-%m-%d").to_string(), &today);
            if let Some(s) = o.start {
                when.push_str(&format!(" · {}", hhmm(s)));
            }
            row(buf, &mut y, "when", &when, val);
            if o.start.is_some() {
                row(buf, &mut y, "lasts", &minutes_label(o.minutes), val);
            }
            if let Some(p) = t.projects.first() {
                let sp = crate::core::spaces::display(p);
                row(buf, &mut y, "space", &sp, val.fg(app.space_color(p)));
            }
            if let Some(d) = t.due.as_deref() {
                row(
                    buf,
                    &mut y,
                    "deadline",
                    &chip_date(d, &today),
                    val.fg(theme.overdue),
                );
            }
            if let Some(rec) = t.rec.as_deref() {
                row(buf, &mut y, "repeat", &crate::app::describe_rec(rec), val);
            }
            if let Some(rem) = t.reminders.as_deref() {
                row(
                    buf,
                    &mut y,
                    "remind",
                    &format!("{} before", rem.replace(',', ", ")),
                    val,
                );
            }
            if o.projected {
                row(buf, &mut y, "", "a future repeat", key);
            }
        }
        None => {
            put(buf, x, y, "nothing on this day", w, key);
            y += 1;
        }
    }

    y += 1;
    if y + 1 < r.bottom() {
        put(buf, x, y, "FREE", w, head);
        y += 1;
        let refs: Vec<&Occurrence> = items.iter().collect();
        let slots = calendar::free_slots(&refs, 8 * 60, 22 * 60, 30);
        if slots.is_empty() {
            put(buf, x, y, "no gaps between 08 and 22", w, key);
        }
        for (s, e) in slots {
            if y >= r.bottom() {
                break;
            }
            let text = format!("{} – {}  {}", hhmm(s), hhmm(e), minutes_label(e - s));
            put(buf, x, y, &text, w, key);
            y += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// Week
// ---------------------------------------------------------------------------

/// The seven days of the selected day's week.
fn week_days(app: &App) -> Vec<chrono::NaiveDate> {
    let Some(cal) = &app.calendar else {
        return Vec::new();
    };
    let from = crate::app::week_start(cal.date);
    (0..7).map(|i| from + chrono::Days::new(i)).collect()
}

/// `mon 5`.
fn day_head(d: chrono::NaiveDate) -> String {
    d.format("%a %-d").to_string().to_lowercase()
}

/// A chip or block colour for an occurrence.
fn occ_color(app: &App, theme: &Theme, o: &Occurrence) -> Color {
    if o.late || o.deadline {
        theme.overdue
    } else {
        task_color(app, o.abs)
    }
}

fn week_blocks(buf: &mut Buffer, r: Rect, app: &App, theme: &Theme) {
    let Some(cal) = &app.calendar else {
        return;
    };
    let days = week_days(app);
    let today = app.today_naive();
    let occs = app.cal_occurrences(days[0], days[6]);
    let selected = app.cal_selected();
    const GW: u16 = 4;
    let col_w = (r.width.saturating_sub(GW)) / 7;
    let col_x = |i: usize| r.x + GW + col_w * i as u16;
    let col_width = |i: usize| if i == 6 { r.right() - col_x(6) } else { col_w };

    // Today's column is faintly lit.
    if let Some(i) = days.iter().position(|d| *d == today)
        && let Some(bg) = tint(theme.mode_bg, theme.bg, 0.06)
    {
        fill(
            buf,
            Rect {
                x: col_x(i),
                y: r.y,
                width: col_width(i),
                height: r.height,
            },
            Style::default().bg(bg),
        );
    }

    // Day heads.
    let mut y = r.y;
    for (i, d) in days.iter().enumerate() {
        let w = col_width(i);
        let text = format!("{:^w$}", day_head(*d), w = usize::from(w));
        let style = if *d == today {
            Style::default()
                .bg(theme.mode_bg)
                .fg(theme.mode_fg)
                .add_modifier(Modifier::BOLD)
        } else if *d == cal.date {
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::default().fg(theme.dim)
        };
        put(buf, col_x(i), y, &text, w, style);
    }
    y += 1;

    // All-day: bands across the days (several-day ones run straight
    // across), up to three rows; a crowded day says how many more.
    let occ_refs: Vec<&Occurrence> = occs.iter().collect();
    let bands = all_day_bands(&occ_refs, &days);
    let used = bands.iter().map(|b| b.row + 1).max().unwrap_or(0);
    let all_rows = used.min(3);
    draw_bands(
        buf,
        app,
        theme,
        &bands,
        y,
        all_rows,
        &|i| col_x(i),
        &|i| col_width(i).saturating_sub(1),
        selected.as_ref(),
        7,
    );
    y += all_rows as u16;
    for x in r.x..r.right() {
        if let Some(c) = buf.cell_mut((x, y)) {
            c.set_symbol("┄");
            c.set_style(Style::default().fg(theme.border));
        }
    }
    y += 1;
    let grid = Rect {
        y,
        height: r.bottom().saturating_sub(y),
        ..r
    };
    if grid.height < 2 {
        return;
    }

    // Hours across the week.
    let timed: Vec<&Occurrence> = occs.iter().filter(|o| o.start.is_some()).collect();
    let first = timed
        .iter()
        .filter_map(|o| o.start)
        .min()
        .map_or(DAY_FROM, |s| (s / 60).min(DAY_FROM));
    let last = timed
        .iter()
        .filter_map(|o| o.end())
        .max()
        .map_or(DAY_TO, |e| e.div_ceil(60).clamp(DAY_TO, 24));
    let hours = last - first;
    let scale = (f64::from(grid.height) / f64::from(hours.max(1))).clamp(1.0, 4.0);
    let visible_hours = ((f64::from(grid.height) / scale) as u32).max(1);
    let focus = selected
        .as_ref()
        .and_then(|o| o.start)
        .or_else(|| timed.first().and_then(|o| o.start))
        .unwrap_or(first * 60);
    let top = if hours <= visible_hours {
        first
    } else {
        (focus / 60)
            .saturating_sub(visible_hours / 3)
            .clamp(first, last - visible_hours)
    };
    let row_of =
        |m: u32| -> i64 { ((f64::from(m) - f64::from(top * 60)) * scale / 60.0).floor() as i64 };

    for h in top..last {
        let row = row_of(h * 60);
        if row < 0 || row >= i64::from(grid.height) {
            continue;
        }
        let gy = grid.y + row as u16;
        put(
            buf,
            grid.x,
            gy,
            &format!("{h:02}  "),
            GW,
            Style::default().fg(theme.dim),
        );
        for x in grid.x + GW..grid.right() {
            if let Some(c) = buf.cell_mut((x, gy)) {
                c.set_symbol("╌");
                c.set_style(Style::default().fg(theme.border));
            }
        }
    }
    // Column rules.
    for i in 1..7 {
        for gy in grid.top()..grid.bottom() {
            if let Some(c) = buf.cell_mut((col_x(i), gy)) {
                c.set_symbol("│");
                c.set_style(Style::default().fg(theme.border));
            }
        }
    }

    for (i, d) in days.iter().enumerate() {
        let day_timed: Vec<&Occurrence> = timed.iter().copied().filter(|o| o.date == *d).collect();
        let lanes = calendar::lanes(&day_timed);
        let inner_x = col_x(i) + 1;
        let inner_w = col_width(i).saturating_sub(1);
        app.cal_cols.borrow_mut().push(crate::app::CalCol {
            x: col_x(i),
            w: col_width(i),
            date: *d,
            y: grid.y,
            h: grid.height,
            top: top * 60,
            scale,
        });
        for (o, (lane, n)) in day_timed.iter().zip(lanes) {
            let (Some(s), Some(e)) = (o.start, o.end()) else {
                continue;
            };
            let r0 = row_of(s).max(0);
            let r1 = gap_before_next(
                row_of(s),
                row_of(e).max(row_of(s) + 1),
                e,
                &day_timed,
                &row_of,
            )
            .min(i64::from(grid.height));
            if r0 >= r1 {
                continue;
            }
            let lane_w = (inner_w / n as u16).max(1);
            let bx = inner_x + lane_w * lane as u16;
            let bw = if lane + 1 == n {
                inner_w - lane_w * lane as u16
            } else {
                lane_w
            };
            let rect = Rect {
                x: bx,
                y: grid.y + r0 as u16,
                width: bw,
                height: (r1 - r0) as u16,
            };
            let sel = selected.as_ref().is_some_and(|x| x == *o);
            draw_mini_block(buf, rect, app, theme, o, sel);
            block_hits(app, rect, o);
        }
        // Now, on today's column.
        if *d == today
            && let Some(now) = app.now_minutes()
        {
            let row = row_of(now);
            if row >= 0 && row < i64::from(grid.height) {
                let ny = grid.y + row as u16;
                for x in inner_x..inner_x + inner_w {
                    if let Some(c) = buf.cell_mut((x, ny))
                        && matches!(c.symbol(), " " | "╌")
                    {
                        c.set_symbol("─");
                        c.set_style(Style::default().fg(theme.overdue));
                    }
                }
            }
        }
    }
    // The block being dragged, where it would land.
    let cols: Vec<crate::app::CalCol> = app.cal_cols.borrow().clone();
    for c in &cols {
        draw_ghost(buf, app, theme, c, c.x + 1, c.w.saturating_sub(1));
    }
}

/// Where a click on block `r` lands: the block, and its bottom row to
/// drag its length (when it has more than one).
fn block_hits(app: &App, r: Rect, o: &Occurrence) {
    if o.projected && app.tasks().get(o.abs).is_none_or(|t| t.rec.is_none()) {
        return;
    }
    app.hits
        .add(r, crate::app::Hit::CalBlock(Box::new(o.clone()), false));
    if r.height >= 2 {
        app.hits.add(
            Rect {
                y: r.bottom() - 1,
                height: 1,
                ..r
            },
            crate::app::Hit::CalBlock(Box::new(o.clone()), true),
        );
    }
}

/// The dragged block's ghost in column `col` (drawn `x`, `w`), when it
/// would land there: a dashed outline with the time it would take.
fn draw_ghost(
    buf: &mut Buffer,
    app: &App,
    theme: &Theme,
    col: &crate::app::CalCol,
    x: u16,
    w: u16,
) {
    let Some(drag) = app.cal_drag.as_ref() else {
        return;
    };
    let Some((date, start, minutes)) = drag.target else {
        return;
    };
    if date != col.date || w < 3 {
        return;
    }
    let r0 = col.row_of(start).max(0);
    let r1 = col
        .row_of(start + minutes)
        .max(r0 + 1)
        .min(i64::from(col.h));
    if r0 >= r1 {
        return;
    }
    let color = task_color(app, drag.occ.abs);
    let mut style = Style::default().fg(color).add_modifier(Modifier::BOLD);
    if let Some(bg) = tint(color, theme.bg, 0.12) {
        style = style.bg(bg);
    }
    let (top, bottom) = (col.y + r0 as u16, col.y + r1 as u16 - 1);
    for y in top..=bottom {
        for xx in x..x + w {
            let edge_y = y == top || y == bottom;
            let edge_x = xx == x || xx == x + w - 1;
            if let Some(c) = buf.cell_mut((xx, y)) {
                if edge_y {
                    c.set_symbol("╌");
                    c.set_style(style);
                } else if edge_x {
                    c.set_symbol("╎");
                    c.set_style(style);
                } else {
                    c.set_symbol(" ");
                    c.set_style(style);
                }
            }
        }
    }
    let label = format!(" {}–{} ", hhmm(start), hhmm((start + minutes).min(24 * 60)));
    put(buf, x + 1, top, &label, w.saturating_sub(2), style);
}

/// A block in a week column: title, then its hours when there's a row.
fn draw_mini_block(buf: &mut Buffer, r: Rect, app: &App, theme: &Theme, o: &Occurrence, sel: bool) {
    let done = app.tasks().get(o.abs).is_some_and(|t| t.done);
    let color = occ_color(app, theme, o);
    let bg = if done {
        theme.panel
    } else {
        tint(color, theme.bg, if sel { 0.40 } else { 0.20 }).unwrap_or(theme.panel)
    };
    fill(buf, r, Style::default().bg(bg));
    let edge = if o.projected {
        "┆"
    } else if sel {
        "┃"
    } else {
        "▎"
    };
    for y in r.top()..r.bottom() {
        if let Some(c) = buf.cell_mut((r.x, y)) {
            c.set_symbol(edge);
            c.set_style(
                Style::default()
                    .fg(if done { theme.done } else { color })
                    .bg(bg),
            );
        }
    }
    let w = r.width.saturating_sub(1);
    let mut head = String::new();
    if o.projected {
        head.push('↻');
        head.push(' ');
    }
    head.push_str(&occ_title(app, o));
    let style = if done {
        Style::default()
            .fg(theme.done)
            .bg(bg)
            .add_modifier(Modifier::CROSSED_OUT)
    } else {
        Style::default()
            .fg(theme.fg)
            .bg(bg)
            .add_modifier(Modifier::BOLD)
    };
    put(buf, r.x + 1, r.y, &fit(&head, usize::from(w)), w, style);
    if r.height >= 2
        && let (Some(s), Some(e)) = (o.start, o.end())
    {
        let hours = format!("{}–{}", hhmm(s), hhmm(e));
        put(
            buf,
            r.x + 1,
            r.y + 1,
            &fit(&hours, usize::from(w)),
            w,
            Style::default().fg(color).bg(bg),
        );
    }
}

/// The week as a list, a group per day: works at any width.
fn week_agenda(buf: &mut Buffer, r: Rect, app: &App, theme: &Theme) {
    let Some(cal) = &app.calendar else {
        return;
    };
    let days = week_days(app);
    let today = app.today_naive();
    let occs = app.cal_occurrences(days[0], days[6]);
    let selected = app.cal_selected();
    agenda(
        buf,
        r,
        app,
        theme,
        &days,
        &occs,
        selected.as_ref(),
        cal.date,
        today,
    );
}

/// Day groups with their tasks; scrolls to keep the selection in view.
#[allow(clippy::too_many_arguments)]
fn agenda(
    buf: &mut Buffer,
    r: Rect,
    app: &App,
    theme: &Theme,
    days: &[chrono::NaiveDate],
    occs: &[Occurrence],
    selected: Option<&Occurrence>,
    sel_day: chrono::NaiveDate,
    today: chrono::NaiveDate,
) {
    enum Row<'a> {
        Head(chrono::NaiveDate),
        Item(&'a Occurrence),
        Empty,
    }
    let mut rows: Vec<Row> = Vec::new();
    for d in days {
        rows.push(Row::Head(*d));
        let items: Vec<&Occurrence> = occs.iter().filter(|o| o.date == *d).collect();
        if items.is_empty() {
            rows.push(Row::Empty);
        }
        rows.extend(items.into_iter().map(Row::Item));
    }
    let sel_row = rows
        .iter()
        .position(|row| matches!(row, Row::Item(o) if selected.is_some_and(|s| s == *o)))
        .or_else(|| {
            rows.iter()
                .position(|row| matches!(row, Row::Head(d) if *d == sel_day))
        })
        .unwrap_or(0);
    let h = usize::from(r.height.saturating_sub(1)).max(1);
    let offset = sel_row
        .saturating_sub(h * 2 / 3)
        .min(rows.len().saturating_sub(h));
    let x = r.x + 2;
    let w = r.width.saturating_sub(4);
    for (i, row) in rows.iter().enumerate().skip(offset).take(h) {
        let y = r.y + 1 + (i - offset) as u16;
        match row {
            Row::Head(d) => {
                let mut label = d.format("%a %-d %b").to_string().to_uppercase();
                if *d == today {
                    label.push_str(" · TODAY");
                }
                let style = if *d == today {
                    Style::default()
                        .fg(theme.mode_bg)
                        .add_modifier(Modifier::BOLD)
                } else if *d == sel_day {
                    Style::default()
                        .fg(theme.accent)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.fg).add_modifier(Modifier::BOLD)
                };
                let used = put(buf, x, y, &label, w, style);
                for cx in x + used + 1..x + w {
                    if let Some(c) = buf.cell_mut((cx, y)) {
                        c.set_symbol("─");
                        c.set_style(Style::default().fg(theme.border));
                    }
                }
            }
            Row::Empty => {
                put(buf, x + 2, y, "—", w, Style::default().fg(theme.border));
            }
            Row::Item(o) => {
                let sel = selected.is_some_and(|s| s == *o);
                let t = app.tasks().get(o.abs);
                let done = t.is_some_and(|t| t.done);
                let dim = o.projected || done;
                let base = if sel {
                    Style::default().bg(theme.cursor)
                } else {
                    Style::default()
                };
                if sel {
                    fill(
                        buf,
                        Rect {
                            x: r.x,
                            y,
                            width: r.width,
                            height: 1,
                        },
                        base,
                    );
                }
                let (mark, mark_color) = if sel {
                    ("▸", theme.accent)
                } else if o.late {
                    ("!", theme.overdue)
                } else if o.deadline {
                    ("◷", theme.overdue)
                } else if o.projected {
                    ("↻", theme.dim)
                } else {
                    (" ", theme.dim)
                };
                put(buf, x, y, mark, 1, base.fg(mark_color));
                let time = o.start.map_or("  —  ".to_string(), hhmm);
                put(buf, x + 2, y, &time, 5, base.fg(theme.dim));
                let mut text = occ_title(app, o);
                if o.start.is_some() && o.minutes != calendar::DEFAULT_MINUTES {
                    text.push_str(&format!(" · {}", minutes_label(o.minutes)));
                }
                let space = t.and_then(|t| t.projects.first()).cloned();
                let space_w = space.as_ref().map_or(0, |p| {
                    crate::core::spaces::leaf(p).chars().count() as u16 + 3
                });
                let tw = w.saturating_sub(8 + space_w);
                let fg = if dim { theme.dim } else { theme.fg };
                let mut ts = base.fg(fg);
                if done {
                    ts = ts.add_modifier(Modifier::CROSSED_OUT);
                }
                if sel {
                    ts = ts.add_modifier(Modifier::BOLD);
                }
                put(buf, x + 8, y, &fit(&text, usize::from(tw)), tw, ts);
                if let Some(p) = space {
                    let label = format!("● {}", crate::core::spaces::leaf(&p));
                    let lw = label.chars().count() as u16;
                    let color = if dim { theme.dim } else { app.space_color(&p) };
                    put(
                        buf,
                        (x + w).saturating_sub(lw),
                        y,
                        &label,
                        lw,
                        base.fg(color),
                    );
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Month
// ---------------------------------------------------------------------------

fn month(buf: &mut Buffer, r: Rect, app: &App, theme: &Theme) {
    let Some(cal) = &app.calendar else {
        return;
    };
    let today = app.today_naive();
    let (first, last) = crate::app::month_bounds(cal.date);
    let grid_from = crate::app::week_start(first);
    let weeks = ((last - grid_from).num_days() / 7 + 1) as u16;
    let grid_to = grid_from + chrono::Days::new(u64::from(weeks) * 7 - 1);
    let occs = app.cal_occurrences(grid_from, grid_to);
    let titles = cal.month_style == crate::app::CalStyle::List;

    // Room: the grid, and in the counts look the selected day below it.
    let panel_h = if titles {
        0
    } else {
        (r.height / 3).clamp(4, 9)
    };
    let grid_h = r.height.saturating_sub(panel_h + 1);
    let cell_h = (grid_h.saturating_sub(1) / weeks).max(2);
    let cell_w = r.width / 7;
    let col_x = |i: u16| r.x + cell_w * i;
    let col_w = |i: u16| if i == 6 { r.width - cell_w * 6 } else { cell_w };

    // Weekday heads.
    for (i, name) in ["mon", "tue", "wed", "thu", "fri", "sat", "sun"]
        .iter()
        .enumerate()
    {
        let i = i as u16;
        let text = format!("{name:^w$}", w = usize::from(col_w(i)));
        put(
            buf,
            col_x(i),
            r.y,
            &text,
            col_w(i),
            Style::default().fg(theme.dim),
        );
    }
    let rule = Style::default().fg(theme.border);
    hline(buf, r.x, r.y + 1, r.width, rule);

    // Each week's title bands, drawn over the rules at the end.
    let mut week_bands: Vec<(u16, Vec<Band>)> = Vec::new();
    for week in 0..weeks {
        let y0 = r.y + 2 + week * cell_h;
        for i in 0..7u16 {
            let d = grid_from + chrono::Days::new(u64::from(week * 7 + i));
            let cell = Rect {
                x: col_x(i),
                y: y0,
                width: col_w(i),
                height: cell_h - 1,
            };
            if cell.bottom() > r.bottom() {
                continue;
            }
            let in_month = d.month() == first.month();
            let sel = d == cal.date;
            if sel && let Some(bg) = tint(theme.accent, theme.bg, 0.10) {
                fill(buf, cell, Style::default().bg(bg));
            }
            let bg = |s: Style| {
                if sel {
                    s.bg(tint(theme.accent, theme.bg, 0.10).unwrap_or(theme.bg))
                } else {
                    s
                }
            };
            // Day number.
            let num = format!("{:>2}", d.day());
            let num_style = if d == today {
                Style::default()
                    .bg(theme.mode_bg)
                    .fg(theme.mode_fg)
                    .add_modifier(Modifier::BOLD)
            } else if !in_month {
                bg(Style::default().fg(theme.border))
            } else if sel {
                bg(Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD))
            } else {
                bg(Style::default().fg(theme.fg))
            };
            put(buf, cell.x + 1, cell.y, &format!(" {num} "), 4, num_style);
            if sel {
                put(
                    buf,
                    cell.right().saturating_sub(2),
                    cell.y,
                    "◆",
                    1,
                    bg(Style::default().fg(theme.accent)),
                );
            }

            let day: Vec<&Occurrence> = occs.iter().filter(|o| o.date == d).collect();
            if day.is_empty() {
                continue;
            }
            if titles {
                // Drawn for the whole week below.
            } else if cell.height >= 2 {
                // How many, then a dot per task in its colour.
                let n = format!("{} ", day.len());
                let mut x = cell.x + 2;
                x += put(buf, x, cell.y + 1, &n, 4, bg(Style::default().fg(theme.fg)));
                let room = cell.right().saturating_sub(x + 1);
                for o in day.iter().take(usize::from(room)) {
                    put(
                        buf,
                        x,
                        cell.y + 1,
                        "●",
                        1,
                        bg(Style::default().fg(occ_color(app, theme, o))),
                    );
                    x += 1;
                }
            }
        }
        // Titles: bands across the week (several-day ones run straight
        // across the days).
        if titles && y0 + cell_h - 1 <= r.bottom() {
            let days: Vec<chrono::NaiveDate> = (0..7u16)
                .map(|i| grid_from + chrono::Days::new(u64::from(week * 7 + i)))
                .collect();
            let refs: Vec<&Occurrence> = occs.iter().collect();
            let bands = all_day_bands(&refs, &days);
            let timed: Vec<&Occurrence> =
                refs.iter().copied().filter(|o| o.start.is_some()).collect();
            // Timed ones are days' items too, after the bands.
            let mut all = bands;
            let mut next_free = [0usize; 7];
            for b in &all {
                for free in &mut next_free[b.c0..=b.c1] {
                    *free = (*free).max(b.row + 1);
                }
            }
            for o in timed {
                if let Some(c) = days.iter().position(|d| *d == o.date) {
                    all.push(Band {
                        occ: o,
                        c0: c,
                        c1: c,
                        more_before: false,
                        more_after: false,
                        row: next_free[c],
                    });
                    next_free[c] += 1;
                }
            }
            week_bands.push((y0 + 1, all));
        }
        // Rules under each week and between the days.
        let ry = y0 + cell_h - 1;
        if ry < r.bottom() {
            hline(buf, r.x, ry, r.width, rule);
        }
    }

    // Rules between the days.
    let grid_bottom = (r.y + 2 + weeks * cell_h).min(r.bottom());
    for i in 1..7u16 {
        for y in r.y + 2..grid_bottom {
            if let Some(c) = buf.cell_mut((col_x(i), y)) {
                let joint = c.symbol() == "─";
                c.set_symbol(if joint { "┼" } else { "│" });
                c.set_style(rule);
            }
        }
    }

    for (y, bands) in &week_bands {
        draw_bands(
            buf,
            app,
            theme,
            bands,
            *y,
            usize::from(cell_h.saturating_sub(2)),
            &|i| col_x(i as u16),
            &|i| col_w(i as u16).saturating_sub(2),
            None,
            7,
        );
    }

    // The selected day, below the grid.
    if panel_h > 0 {
        let py = r.y + 2 + weeks * cell_h;
        if py + 2 < r.bottom() {
            let pr = Rect {
                x: r.x,
                y: py,
                width: r.width,
                height: r.bottom() - py,
            };
            let items = app.cal_day_items();
            let selected = app.cal_selected();
            agenda(
                buf,
                pr,
                app,
                theme,
                &[cal.date],
                &items,
                selected.as_ref(),
                cal.date,
                today,
            );
        }
    }
}
