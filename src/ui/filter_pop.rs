//! The "+ filter" popover: a rounded card hanging under the filter chips,
//! with a search box on top and, below it, everything you can filter by in
//! sections — each row an icon, a name and a count.

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use crate::app::{App, PopPick, PopRow, Preset};
use crate::theme::Theme;

const W: u16 = 46;

/// Draw the popover into `list` (the list's column), under its chips row.
pub fn render(frame: &mut Frame, list: Rect, app: &App) {
    let theme = app.theme();
    let rows = app.filter_rows();
    // Rows with their headings, as lines: None is a heading.
    let mut lines: Vec<(Option<usize>, &str)> = Vec::new();
    let mut last = "";
    for (i, r) in rows.iter().enumerate() {
        if r.section != last {
            lines.push((None, r.section));
            last = r.section;
        }
        lines.push((Some(i), r.section));
    }

    let x = list.x + 2;
    let y = list.y + 3;
    let w = W.min(list.width.saturating_sub(3));
    let max_h = list.bottom().saturating_sub(y + 1);
    // Border, search box (3), a gap, the lines, border.
    let want = 2 + 3 + lines.len().max(1) as u16;
    let h = want.min(max_h);
    if w < 20 || h < 6 {
        return;
    }
    let r = Rect {
        x,
        y,
        width: w,
        height: h,
    };
    let buf = frame.buffer_mut();
    let bg = Style::default().bg(theme.panel).fg(theme.fg);
    rounded(buf, r, bg.fg(theme.border));

    // The search box.
    let q = Rect {
        x: r.x + 2,
        y: r.y + 1,
        width: r.width - 4,
        height: 3,
    };
    rounded(buf, q, bg.fg(theme.accent));
    let typed = &app.filter_pop.query;
    let mut cx = q.x + 2;
    cx += put(buf, cx, q.y + 1, "⌕ ", q.right() - cx, bg.fg(theme.dim));
    if typed.is_empty() {
        // The caret first, the hint after it, so it doesn't cover the `s`.
        put(buf, cx, q.y + 1, "▏", 1, bg.fg(theme.accent));
        put(
            buf,
            cx + 1,
            q.y + 1,
            "search spaces, tags, dates…",
            q.right().saturating_sub(cx + 2),
            bg.fg(theme.dim),
        );
    } else {
        cx += put(
            buf,
            cx,
            q.y + 1,
            typed,
            q.right().saturating_sub(cx + 2),
            bg.fg(theme.fg),
        );
        put(buf, cx, q.y + 1, "▏", 1, bg.fg(theme.accent));
    }

    // The options, scrolled to keep the cursor in view.
    let top = q.bottom();
    let room = usize::from(r.bottom().saturating_sub(top + 1));
    if lines.is_empty() {
        put(
            buf,
            r.x + 3,
            top,
            "nothing matches",
            r.width - 6,
            bg.fg(theme.dim),
        );
        return;
    }
    let cur_line = lines
        .iter()
        .position(|(i, _)| *i == Some(app.filter_pop.cursor))
        .unwrap_or(0);
    let skip = cur_line.saturating_sub(room.saturating_sub(1));
    for (n, (row, section)) in lines.iter().skip(skip).take(room).enumerate() {
        let ly = top + n as u16;
        match row {
            None => {
                put(
                    buf,
                    r.x + 3,
                    ly,
                    section,
                    r.width - 6,
                    bg.fg(theme.dim).add_modifier(Modifier::BOLD),
                );
            }
            Some(i) => {
                let here = *i == app.filter_pop.cursor;
                option(buf, r, ly, &rows[*i], here, app, theme);
            }
        }
    }
    if skip + room < lines.len() {
        put(buf, r.right() - 4, r.bottom() - 1, "↓", 1, bg.fg(theme.dim));
    }
}

fn option(buf: &mut Buffer, r: Rect, y: u16, row: &PopRow, here: bool, app: &App, theme: &Theme) {
    let row_bg = if here { theme.cursor } else { theme.panel };
    let base = Style::default().bg(row_bg);
    for x in r.x + 2..r.right() - 2 {
        if let Some(c) = buf.cell_mut((x, y)) {
            c.set_symbol(" ");
            c.set_style(base);
        }
    }
    let (icon, color) = icon(&row.pick, app, theme);
    put(buf, r.x + 3, y, &icon, 2, base.fg(color));
    let label_style = if row.on {
        base.fg(theme.accent).add_modifier(Modifier::BOLD)
    } else if here {
        base.fg(theme.fg).add_modifier(Modifier::BOLD)
    } else {
        base.fg(theme.status_fg)
    };
    let right = match (row.on, row.count) {
        (true, _) => "✓".to_string(),
        (false, Some(n)) => n.to_string(),
        (false, None) => String::new(),
    };
    let rw = right.chars().count() as u16;
    let label_w = r.width.saturating_sub(10 + rw);
    put(buf, r.x + 5, y, &row.label, label_w, label_style);
    if rw > 0 {
        let color = if row.on { theme.accent } else { theme.dim };
        put(buf, r.right() - 3 - rw, y, &right, rw, base.fg(color));
    }
}

fn icon(pick: &PopPick, app: &App, theme: &Theme) -> (String, Color) {
    match pick {
        PopPick::Space(p) => ("●".into(), app.space_color(p)),
        PopPick::Tag(_) => ("@".into(), theme.context),
        PopPick::Due(_) => ("◷".into(), theme.due),
        PopPick::Preset(Preset::Overdue) => ("◷".into(), theme.overdue),
        PopPick::Preset(Preset::HighPriority) => ("⚑".into(), theme.pri_a),
        PopPick::Preset(Preset::Starred) => ("★".into(), theme.matched),
        PopPick::Preset(Preset::Inbox) => ("▤".into(), theme.accent),
        PopPick::Saved(_) => ("☰".into(), theme.dim),
        PopPick::Save => ("+".into(), theme.accent),
        PopPick::Clear => ("×".into(), theme.dim),
    }
}

fn rounded(buf: &mut Buffer, r: Rect, border: Style) {
    for dy in 0..r.height {
        for dx in 0..r.width {
            let last_x = dx == r.width - 1;
            let last_y = dy == r.height - 1;
            let sym = match (dy, dx) {
                (0, 0) => "╭",
                (0, _) if last_x => "╮",
                (_, 0) if last_y => "╰",
                _ if last_x && last_y => "╯",
                (0, _) => "─",
                _ if last_y => "─",
                (_, 0) => "│",
                _ if last_x => "│",
                _ => " ",
            };
            if let Some(c) = buf.cell_mut((r.x + dx, r.y + dy)) {
                c.set_symbol(sym);
                c.set_style(border);
            }
        }
    }
}

fn put(buf: &mut Buffer, x: u16, y: u16, s: &str, max: u16, style: Style) -> u16 {
    if max == 0 || !buf.area.contains((x, y).into()) {
        return 0;
    }
    let (end, _) = buf.set_stringn(x, y, s, usize::from(max), style);
    end.saturating_sub(x)
}
