//! The Search window: a box on top, and what matches below it in
//! sections — tasks, notes (with the line that matched) and spaces — the
//! words you typed lit up.

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::Clear;

use crate::app::{App, Found};
use crate::theme::Theme;

pub fn render(frame: &mut Frame, screen: Rect, app: &App) {
    let theme = app.theme();
    let rows = app.search_all_rows();
    let mut lines: Vec<(Option<usize>, &str)> = Vec::new();
    let mut last = "";
    for (i, r) in rows.iter().enumerate() {
        if r.section != last {
            lines.push((None, r.section));
            last = r.section;
        }
        lines.push((Some(i), r.section));
    }
    let w = 76.min(screen.width.saturating_sub(4));
    let body = (lines.len().max(1) as u16).min(screen.height.saturating_sub(10));
    let h = 5 + body + 1;
    let r = Rect {
        x: screen.x + (screen.width.saturating_sub(w)) / 2,
        y: screen.y + 2,
        width: w,
        height: h.min(screen.height.saturating_sub(2)),
    };
    frame.render_widget(Clear, r);
    let buf = frame.buffer_mut();
    let bg = Style::default().bg(theme.panel);
    rounded(buf, r, bg.fg(theme.border));
    // The box.
    let q = Rect {
        x: r.x + 2,
        y: r.y + 1,
        width: r.width.saturating_sub(4),
        height: 3,
    };
    rounded(buf, q, bg.fg(theme.accent));
    let typed = &app.search_all.query;
    let mut x = q.x + 2;
    x += put(buf, x, q.y + 1, "⌕ ", 2, bg.fg(theme.dim));
    if typed.is_empty() {
        // The hint sits after the caret, so the caret doesn't cover the `s`.
        put(
            buf,
            x + 1,
            q.y + 1,
            "search tasks, notes and spaces…",
            q.right().saturating_sub(x + 2),
            bg.fg(theme.dim),
        );
    } else {
        x += put(
            buf,
            x,
            q.y + 1,
            typed,
            q.right().saturating_sub(x + 2),
            bg.fg(theme.fg),
        );
    }
    put(buf, x, q.y + 1, "▏", 1, bg.fg(theme.accent));

    let top = q.bottom();
    let room = usize::from(r.bottom().saturating_sub(top + 1));
    if typed.trim().is_empty() {
        put(
            buf,
            r.x + 3,
            top,
            "type a word: it looks in task titles, note titles and text, and spaces",
            r.width.saturating_sub(6),
            bg.fg(theme.dim),
        );
        return;
    }
    if rows.is_empty() {
        put(
            buf,
            r.x + 3,
            top,
            "nothing found",
            r.width - 6,
            bg.fg(theme.dim),
        );
        return;
    }
    let needle = typed.trim().to_lowercase();
    let cur = lines
        .iter()
        .position(|(i, _)| *i == Some(app.search_all.cursor))
        .unwrap_or(0);
    let skip = cur.saturating_sub(room.saturating_sub(1));
    for (n, (i, section)) in lines.iter().skip(skip).take(room).enumerate() {
        let y = top + n as u16;
        let Some(i) = i else {
            put(
                buf,
                r.x + 3,
                y,
                section,
                r.width - 6,
                bg.fg(theme.dim).add_modifier(Modifier::BOLD),
            );
            continue;
        };
        let row = &rows[*i];
        let here = *i == app.search_all.cursor;
        let rbg = if here { theme.cursor } else { theme.panel };
        let base = Style::default().bg(rbg);
        for x in r.x + 2..r.right() - 2 {
            if let Some(c) = buf.cell_mut((x, y)) {
                c.set_symbol(" ");
                c.set_style(base);
            }
        }
        let (icon, color): (&str, Color) = match &row.found {
            Found::Task(_) if row.done => ("■", theme.ok),
            Found::Task(_) => ("☐", theme.dim),
            Found::Note(_) => ("≡", theme.accent),
            Found::Space(p) => ("●", app.space_color(p)),
        };
        put(buf, r.x + 3, y, icon, 2, base.fg(color));
        let label_style = if row.done {
            base.fg(theme.done).add_modifier(Modifier::CROSSED_OUT)
        } else if here {
            base.fg(theme.fg).add_modifier(Modifier::BOLD)
        } else {
            base.fg(theme.fg)
        };
        let lx = r.x + 5;
        let lw = (r.width / 2).saturating_sub(4);
        let used = put(buf, lx, y, &fit(&row.label, lw), lw, label_style);
        light(buf, lx, y, used, &needle, theme);
        if !row.detail.is_empty() {
            let dx = lx + used + 2;
            let dw = r.right().saturating_sub(dx + 3);
            let d = put(buf, dx, y, &fit(&row.detail, dw), dw, base.fg(theme.dim));
            light(buf, dx, y, d, &needle, theme);
        }
    }
}

/// Light up `needle` where it shows in the `w` cells from `(x, y)`.
fn light(buf: &mut Buffer, x: u16, y: u16, w: u16, needle: &str, theme: &Theme) {
    let n = needle.chars().count();
    if n == 0 {
        return;
    }
    let cells: Vec<String> = (x..x + w)
        .map(|cx| buf[(cx, y)].symbol().to_lowercase())
        .collect();
    let mut i = 0;
    while i + n <= cells.len() {
        if cells[i..i + n].concat() == needle {
            for k in i..i + n {
                buf[(x + k as u16, y)].set_fg(theme.matched);
                buf[(x + k as u16, y)].modifier.insert(Modifier::BOLD);
            }
            i += n;
        } else {
            i += 1;
        }
    }
}

fn fit(s: &str, w: u16) -> String {
    let w = usize::from(w);
    if s.chars().count() <= w {
        return s.to_string();
    }
    if w == 0 {
        return String::new();
    }
    s.chars().take(w - 1).collect::<String>() + "…"
}

fn rounded(buf: &mut Buffer, r: Rect, style: Style) {
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
                c.set_style(style);
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
