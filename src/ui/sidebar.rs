//! The sidebar, drawn Notion-style: the brand, the views, the spaces as a
//! tree in their colours, the filters, and a card for you at the bottom.
//!
//! It is drawn at full width into a buffer of its own and then copied in as
//! wide as it currently is, so opening and closing slides like an
//! accordion instead of reflowing.

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use crate::app::{App, NavItem, NavRow, Preset, SIDEBAR_W};
use crate::theme::Theme;
use crate::ui::task_row::tint;

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    if area.width == 0 {
        return;
    }
    let full = Rect {
        x: 0,
        y: 0,
        width: SIDEBAR_W,
        height: area.height,
    };
    let mut own = Buffer::empty(full);
    draw(&mut own, full, app);
    // Copy in the part that's open, its right edge sliding.
    let buf = frame.buffer_mut();
    for y in 0..area.height {
        for x in 0..area.width.min(SIDEBAR_W) {
            if let (Some(src), Some(dst)) =
                (own.cell((x, y)), buf.cell_mut((area.x + x, area.y + y)))
            {
                *dst = src.clone();
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

fn fill_row(buf: &mut Buffer, x: u16, y: u16, w: u16, style: Style) {
    for dx in 0..w {
        if let Some(c) = buf.cell_mut((x + dx, y)) {
            c.set_symbol(" ");
            c.set_style(style);
        }
    }
}

fn icon(item: &NavItem, app: &App, theme: &Theme) -> (String, Color) {
    match item {
        NavItem::Today => ("◉".into(), theme.accent),
        NavItem::Upcoming => ("◷".into(), theme.dim),
        NavItem::All => ("≡".into(), theme.dim),
        NavItem::Calendar => ("▦".into(), theme.dim),
        NavItem::Search => ("⌕".into(), theme.dim),
        NavItem::Space(p) => ("●".into(), app.space_color(p)),
        NavItem::Preset(Preset::HighPriority) => ("⚑".into(), theme.pri_a),
        NavItem::Preset(Preset::Starred) => ("★".into(), theme.matched),
        NavItem::Preset(Preset::Overdue) => ("◷".into(), theme.overdue),
        NavItem::Preset(Preset::Inbox) | NavItem::Inbox => ("▤".into(), theme.dim),
        NavItem::Home => ("⌂".into(), theme.dim),
        NavItem::Notes => ("✎".into(), theme.dim),
        NavItem::Saved(_) => ("☰".into(), theme.dim),
    }
}

fn draw(buf: &mut Buffer, r: Rect, app: &App) {
    let theme = app.theme();
    let bg = Style::default().bg(theme.panel).fg(theme.fg);
    for y in r.top()..r.bottom() {
        fill_row(buf, r.x, y, r.width, bg);
    }
    // A hairline on the right edge.
    for y in r.top()..r.bottom() {
        if let Some(c) = buf.cell_mut((r.right() - 1, y)) {
            c.set_symbol("│");
            c.set_style(Style::default().fg(theme.border).bg(theme.panel));
        }
    }
    let inner_w = r.width - 2;
    let mut y = r.y + 1;

    // Brand.
    put(buf, r.x + 2, y, "▣", 1, bg.fg(theme.accent));
    put(
        buf,
        r.x + 4,
        y,
        "tasq",
        8,
        bg.fg(theme.fg).add_modifier(Modifier::BOLD),
    );
    y += 2;

    let rows = app.sidebar_rows();
    let active = app.sidebar_active();
    let card_h = 4;
    let bottom = r.bottom().saturating_sub(card_h + 1);
    let mut section = 0u8;
    for (i, row) in rows.iter().enumerate() {
        // Section headings between the views, the spaces and the filters.
        let this = match row.item {
            NavItem::Space(_) => 1,
            NavItem::Preset(_) | NavItem::Saved(_) => 2,
            _ => 0,
        };
        if this != section {
            section = this;
            y += 1;
            if y >= bottom {
                break;
            }
            let label = if this == 1 { "SPACES" } else { "FILTERS" };
            put(
                buf,
                r.x + 2,
                y,
                label,
                inner_w,
                bg.fg(theme.dim).add_modifier(Modifier::BOLD),
            );
            y += 1;
        }
        if y >= bottom {
            // No room: say how many more.
            let more = rows.len() - i;
            put(
                buf,
                r.x + 2,
                y.min(bottom),
                &format!("+{more} more"),
                inner_w,
                bg.fg(theme.dim),
            );
            break;
        }
        draw_row(buf, r, y, row, app, theme, active.as_ref(), i);
        y += 1;
    }

    profile_card(buf, r, app, theme);
}

#[allow(clippy::too_many_arguments)]
fn draw_row(
    buf: &mut Buffer,
    r: Rect,
    y: u16,
    row: &NavRow,
    app: &App,
    theme: &Theme,
    active: Option<&NavItem>,
    i: usize,
) {
    let is_active = active == Some(&row.item);
    let focused = app.sidebar_focus && app.sidebar_cursor == i;
    let row_bg = if focused {
        tint(theme.accent, theme.panel, 0.22).unwrap_or(theme.selected)
    } else if is_active {
        theme.cursor
    } else {
        theme.panel
    };
    let base = Style::default().bg(row_bg);
    // The row is a rounded pill one column in from each edge.
    fill_row(buf, r.x + 1, y, r.width - 3, base);
    if focused {
        put(buf, r.x + 1, y, "▎", 1, base.fg(theme.accent));
    }
    let indent = 2 + 2 * row.depth as u16;
    let (ic, ic_color) = icon(&row.item, app, theme);
    let ic_color = if row.dimmed { theme.dim } else { ic_color };
    let x = r.x + 1 + indent;
    put(buf, x, y, &ic, 1, base.fg(ic_color));
    let text_style = if row.dimmed {
        base.fg(theme.dim)
    } else if is_active || focused {
        base.fg(theme.fg).add_modifier(Modifier::BOLD)
    } else {
        base.fg(theme.status_fg)
    };
    let count = row.count.map(|n| n.to_string());
    let count_w = count.as_ref().map_or(0, |c| c.len() as u16 + 1);
    let label_w = (r.width - 3).saturating_sub(indent + 2 + count_w + 1);
    let label = fit(&row.label, usize::from(label_w));
    put(buf, x + 2, y, &label, label_w, text_style);
    let hint = match row.item {
        NavItem::Search => Some("/"),
        _ => None,
    };
    if let Some(c) = count.as_deref().or(hint) {
        let cx = r.right() - 3 - c.len() as u16;
        put(buf, cx, y, c, c.len() as u16, base.fg(theme.dim));
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

/// You, at the bottom: a round avatar with your initial, your name, and how
/// your tasks are kept (on this machine, until sync lands).
fn profile_card(buf: &mut Buffer, r: Rect, app: &App, theme: &Theme) {
    if r.height < 10 {
        return;
    }
    let w = r.width - 3;
    let x = r.x + 1;
    let y = r.bottom() - 4;
    let border = Style::default().fg(theme.border).bg(theme.panel);
    let line = "─".repeat(usize::from(w - 2));
    put(buf, x, y, &format!("╭{line}╮"), w, border);
    put(buf, x, y + 2, &format!("╰{line}╯"), w, border);
    put(buf, x, y + 1, "│", 1, border);
    put(buf, x + w - 1, y + 1, "│", 1, border);
    let name = &app.user_name;
    let initial: String = name
        .chars()
        .next()
        .map_or("?".into(), |c| c.to_uppercase().collect());
    let avatar = Style::default()
        .bg(theme.pri_b)
        .fg(theme.bg)
        .add_modifier(Modifier::BOLD);
    put(buf, x + 2, y + 1, &format!(" {initial} "), 3, avatar);
    let bg = Style::default().bg(theme.panel);
    let used = put(
        buf,
        x + 6,
        y + 1,
        &fit(name, usize::from(w.saturating_sub(16))),
        w.saturating_sub(16),
        bg.fg(theme.fg).add_modifier(Modifier::BOLD),
    );
    let status = if app.is_db() { "● local" } else { "● file" };
    put(
        buf,
        x + 7 + used,
        y + 1,
        status,
        w.saturating_sub(8 + used),
        bg.fg(theme.pri_c),
    );
}
