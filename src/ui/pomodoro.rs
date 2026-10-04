//! The focus timer, pinned at the top right: a rounded card with the time
//! left, a bar, and what you're on. Toasts stack under it.

use std::time::Instant;

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use crate::app::{App, Phase};

/// Rows the card takes.
pub const H: u16 = 4;
const W: u16 = 40;

/// Draw the timer if it runs; returns the rows it took from the top.
pub fn render(frame: &mut Frame, area: Rect, app: &App, top: u16) -> u16 {
    let Some(p) = app.pomodoro.as_ref() else {
        return 0;
    };
    let theme = app.theme();
    let w = W.min(area.width.saturating_sub(2));
    if w < 20 || area.height < top + H {
        return 0;
    }
    let now = Instant::now();
    let x = area.right() - w - 1;
    let y = area.y + top;
    let color = if p.paused() {
        theme.dim
    } else {
        match p.phase {
            Phase::Focus => theme.pri_a,
            Phase::Break => theme.ok,
        }
    };
    let buf = frame.buffer_mut();
    let bg = Style::default().bg(theme.panel);
    card(
        buf,
        Rect {
            x,
            y,
            width: w,
            height: H,
        },
        bg.fg(color),
    );

    // ◔ focus  18:42  ━━━━━━────
    let pie = ["○", "◔", "◑", "◕", "●"][(p.progress(now) * 4.0).floor() as usize];
    let mut cx = x + 2;
    cx += put(buf, cx, y + 1, pie, 2, bg.fg(color));
    cx += put(buf, cx, y + 1, " ", 1, bg);
    let label = match (p.paused(), p.phase) {
        (true, _) => "paused",
        (false, Phase::Focus) => "focus",
        (false, Phase::Break) => "break",
    };
    cx += put(
        buf,
        cx,
        y + 1,
        label,
        7,
        bg.fg(color).add_modifier(Modifier::BOLD),
    );
    cx += put(buf, cx, y + 1, "  ", 2, bg);
    cx += put(
        buf,
        cx,
        y + 1,
        &p.clock(now),
        5,
        bg.fg(theme.fg).add_modifier(Modifier::BOLD),
    );
    cx += put(buf, cx, y + 1, "  ", 2, bg);
    let bar_w = (x + w).saturating_sub(cx + 2);
    let filled = (f32::from(bar_w) * p.progress(now)).round() as u16;
    put(
        buf,
        cx,
        y + 1,
        &"━".repeat(usize::from(filled)),
        bar_w,
        bg.fg(color),
    );
    put(
        buf,
        cx + filled,
        y + 1,
        &"━".repeat(usize::from(bar_w - filled)),
        bar_w,
        bg.fg(theme.border),
    );

    // What you're on, and the round.
    let mut line = p.task.clone().unwrap_or_else(|| "focus".to_string());
    if p.rounds > 0 {
        line.push_str(&format!(" · round {}", p.rounds + 1));
    }
    let room = w.saturating_sub(6);
    let text: String = if line.chars().count() > usize::from(room) {
        line.chars().take(usize::from(room) - 1).collect::<String>() + "…"
    } else {
        line
    };
    put(buf, x + 4, y + 2, &text, room, bg.fg(theme.status_fg));
    H
}

fn card(buf: &mut Buffer, r: Rect, style: Style) {
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
