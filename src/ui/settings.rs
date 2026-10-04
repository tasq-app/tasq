//! Settings and profile: you and the sections on the left, the section's
//! settings as rounded cards on the right.

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use crate::app::{App, SETTINGS_SECTIONS, SetRow};
use crate::theme::Theme;
use crate::ui::task_row::tint;

const NAV_W: u16 = 30;

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let theme = app.theme();
    super::fill_bg(frame, area, Style::default().bg(theme.bg));
    if area.width < 50 || area.height < 12 {
        return;
    }
    let buf = frame.buffer_mut();
    let st = &app.settings;
    let nav = Rect {
        width: NAV_W,
        ..area
    };
    let pbg = Style::default().bg(theme.panel);
    for y in nav.top()..nav.bottom() {
        for x in nav.left()..nav.right() {
            if let Some(c) = buf.cell_mut((x, y)) {
                c.set_symbol(if x == nav.right() - 1 { "│" } else { " " });
                c.set_style(pbg.fg(theme.border));
            }
        }
    }

    // You.
    let name = &app.user_name;
    let initial: String = name
        .chars()
        .next()
        .map_or("?".into(), |c| c.to_uppercase().collect());
    put(
        buf,
        nav.x + 2,
        nav.y + 1,
        &format!(" {initial} "),
        3,
        Style::default()
            .bg(theme.pri_b)
            .fg(theme.bg)
            .add_modifier(Modifier::BOLD),
    );
    put(
        buf,
        nav.x + 6,
        nav.y + 1,
        name,
        NAV_W - 8,
        pbg.fg(theme.fg).add_modifier(Modifier::BOLD),
    );
    let place = if app.is_db() { "● local" } else { "● file" };
    put(
        buf,
        nav.x + 6,
        nav.y + 2,
        place,
        NAV_W - 8,
        pbg.fg(theme.pri_c),
    );

    // The sections.
    for (i, s) in SETTINGS_SECTIONS.iter().enumerate() {
        let y = nav.y + 4 + i as u16;
        if y >= nav.bottom() {
            break;
        }
        let here = i == st.section;
        let row = if here && !st.in_rows {
            tint(theme.accent, theme.panel, 0.22).unwrap_or(theme.selected)
        } else if here {
            theme.cursor
        } else {
            theme.panel
        };
        for x in nav.x + 1..nav.right() - 2 {
            if let Some(c) = buf.cell_mut((x, y)) {
                c.set_symbol(" ");
                c.set_bg(row);
            }
        }
        if here && !st.in_rows {
            put(
                buf,
                nav.x + 1,
                y,
                "▎",
                1,
                Style::default().bg(row).fg(theme.accent),
            );
        }
        let style = if here {
            Style::default()
                .bg(row)
                .fg(theme.fg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().bg(row).fg(theme.status_fg)
        };
        put(buf, nav.x + 3, y, s, NAV_W - 5, style);
    }

    // The section's cards.
    let body = Rect {
        x: area.x + NAV_W + 2,
        y: area.y + 1,
        width: area.width.saturating_sub(NAV_W + 4),
        height: area.height.saturating_sub(1),
    };
    put(
        buf,
        body.x,
        body.y,
        SETTINGS_SECTIONS[st.section],
        body.width,
        Style::default()
            .bg(theme.bg)
            .fg(theme.fg)
            .add_modifier(Modifier::BOLD),
    );
    let rows = app.settings_rows(st.section);
    let mut y = body.y + 2;
    let mut i = 0;
    while i < rows.len() {
        let card = rows[i].card;
        let n = rows[i..].iter().take_while(|r| r.card == card).count();
        let h = n as u16 + 3;
        if y + h > body.bottom() {
            break;
        }
        let r = Rect {
            x: body.x,
            y,
            width: body.width.min(76),
            height: h,
        };
        rounded(buf, r, pbg.fg(theme.border));
        put(
            buf,
            r.x + 2,
            r.y + 1,
            &card.to_uppercase(),
            r.width - 4,
            pbg.fg(theme.dim).add_modifier(Modifier::BOLD),
        );
        for (k, row) in rows[i..i + n].iter().enumerate() {
            let here = st.in_rows && st.row == i + k;
            draw_row(buf, r, r.y + 2 + k as u16, row, here, theme);
        }
        y += h + 1;
        i += n;
    }
    let hint = if st.in_rows {
        "↑↓ move · Enter change · ← sections · Esc close"
    } else {
        "↑↓ sections · → or Enter open · Esc close"
    };
    put(
        buf,
        body.x,
        body.bottom().saturating_sub(1),
        hint,
        body.width,
        Style::default().bg(theme.bg).fg(theme.dim),
    );
}

fn draw_row(buf: &mut Buffer, card: Rect, y: u16, row: &SetRow, here: bool, theme: &Theme) {
    let bg = if here { theme.cursor } else { theme.panel };
    let base = Style::default().bg(bg);
    for x in card.x + 1..card.right() - 1 {
        if let Some(c) = buf.cell_mut((x, y)) {
            c.set_symbol(" ");
            c.set_style(base);
        }
    }
    if here {
        put(buf, card.x + 1, y, "▎", 1, base.fg(theme.accent));
    }
    let label_style = if row.soon {
        base.fg(theme.dim)
    } else {
        base.fg(theme.fg)
    };
    put(buf, card.x + 3, y, row.label, 18, label_style);
    let x = card.x + 22;
    let room = card.right().saturating_sub(x + 2);
    let (text, style) = match row.value.as_str() {
        "on" => ("● on".to_string(), base.fg(theme.pri_c)),
        "off" => ("○ off".to_string(), base.fg(theme.dim)),
        v if row.soon => (v.to_string(), base.fg(theme.dim)),
        v if row.key.is_some() => (format!("{v} ›"), base.fg(theme.accent)),
        v => (v.to_string(), base.fg(theme.status_fg)),
    };
    put(buf, x, y, &fit(&text, room), room, style);
}

fn fit(s: &str, w: u16) -> String {
    let w = usize::from(w);
    if s.chars().count() <= w {
        return s.to_string();
    }
    if w == 0 {
        return String::new();
    }
    // Paths keep their end.
    let tail: String = s
        .chars()
        .rev()
        .take(w - 1)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("…{tail}")
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
