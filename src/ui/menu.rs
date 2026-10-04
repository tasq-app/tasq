//! The shortcut menu: a rounded card above the status line with what you
//! can do from here, three columns of `key  label`; entries that open a
//! page of their own are in the accent colour and end in `…`.

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use crate::app::{App, MenuDo, MenuPage, menu_entries};

const COLS: u16 = 3;
const COL_W: u16 = 20;

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let theme = app.theme();
    let entries = menu_entries(app.menu_page);
    let rows = (entries.len() as u16).div_ceil(COLS);
    let w = (COL_W * COLS + 4).min(area.width.saturating_sub(2));
    let h = rows + 4;
    if area.height < h + 1 {
        return;
    }
    let x = area.x + (area.width - w) / 2;
    let y = area.bottom() - h - 1;
    let r = Rect {
        x,
        y,
        width: w,
        height: h,
    };
    let buf = frame.buffer_mut();
    let bg = Style::default().bg(theme.panel).fg(theme.fg);
    card(buf, r, bg.fg(theme.accent));

    let title = match app.menu_page {
        MenuPage::Root => "␣ menu",
        MenuPage::Go => "␣ menu › go to",
    };
    put(
        buf,
        x + 2,
        y + 1,
        title,
        w - 4,
        bg.fg(theme.accent).add_modifier(Modifier::BOLD),
    );
    let close = if app.menu_page == MenuPage::Root {
        "esc close"
    } else {
        "⌫ back · esc close"
    };
    let cw = close.chars().count() as u16;
    put(buf, x + w - 2 - cw, y + 1, close, cw, bg.fg(theme.dim));

    let key_style = Style::default()
        .bg(theme.cursor)
        .fg(theme.fg)
        .add_modifier(Modifier::BOLD);
    for (i, e) in entries.iter().enumerate() {
        let i = i as u16;
        let ex = x + 2 + (i % COLS) * COL_W;
        let ey = y + 3 + i / COLS;
        put(buf, ex, ey, &format!(" {} ", e.key), 3, key_style);
        let page = matches!(e.does, MenuDo::Page(_));
        let label_style = if page {
            bg.fg(theme.pri_other).add_modifier(Modifier::BOLD)
        } else {
            bg.fg(theme.status_fg)
        };
        put(buf, ex + 4, ey, e.label, COL_W - 5, label_style);
    }
}

fn card(buf: &mut Buffer, r: Rect, border: Style) {
    for dy in 0..r.height {
        for dx in 0..r.width {
            let sym = match (dy, dx) {
                (0, 0) => "╭",
                (0, d) if d == r.width - 1 => "╮",
                (h, 0) if h == r.height - 1 => "╰",
                (h, d) if h == r.height - 1 && d == r.width - 1 => "╯",
                (0, _) => "─",
                (h, _) if h == r.height - 1 => "─",
                (_, 0) => "│",
                (_, d) if d == r.width - 1 => "│",
                _ => " ",
            };
            if let Some(c) = buf.cell_mut((r.x + dx, r.y + dy)) {
                c.set_symbol(sym);
                c.set_style(border);
            }
        }
    }
}

fn put(buf: &mut Buffer, x: u16, y: u16, s: &str, max: u16, style: Style) {
    if max > 0 && buf.area.contains((x, y).into()) {
        buf.set_stringn(x, y, s, usize::from(max), style);
    }
}
