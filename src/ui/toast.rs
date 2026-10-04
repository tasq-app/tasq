//! Toasts at the top right: a rounded card in the message's colour that
//! slides in from the right edge, stays, and slides back out.

use std::time::Instant;

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use crate::app::{App, ToastKind};
use crate::theme::Theme;

/// Rows a toast takes: border, text, border.
const H: u16 = 3;
const MAX_W: u16 = 56;

fn colors(kind: ToastKind, theme: &Theme) -> (Color, &'static str) {
    match kind {
        ToastKind::Done => (theme.ok, "✓"),
        ToastKind::Info => (theme.accent, "●"),
        ToastKind::Error => (theme.overdue, "!"),
    }
}

/// Draw the toasts into the top right of `area`, newest on top. `top` is
/// the first row they may use (below whatever pins itself there).
pub fn render(frame: &mut Frame, area: Rect, app: &App, top: u16) {
    let theme = app.theme();
    let now = Instant::now();
    let buf = frame.buffer_mut();
    let mut y = area.y + top;
    for (toast, offset) in app.toasts.visible(now) {
        if y + H > area.bottom() {
            break;
        }
        let (color, icon) = colors(toast.kind, theme);
        let detail = toast.detail.as_deref().unwrap_or("");
        let text_w = toast.title.chars().count()
            + if detail.is_empty() {
                0
            } else {
                detail.chars().count() + 3
            };
        let w = (text_w as u16 + 6)
            .min(MAX_W)
            .min(area.width.saturating_sub(2));
        // Fully in: one column from the right edge. Out: past the edge.
        let rest_x = area.right().saturating_sub(w + 1);
        let x = rest_x + ((f32::from(w + 1) * offset).round() as u16);
        // A raised card: a soft border, the icon in plain text, the title
        // in the notice's colour.
        card(buf, area, x, y, w, theme.selection, theme);
        let mut cx = x + 2;
        let bg = Style::default().bg(theme.panel);
        cx += put(buf, area, cx, y + 1, icon, bg.fg(theme.fg));
        cx += put(buf, area, cx, y + 1, " ", bg);
        let room = usize::from((x + w).saturating_sub(cx + 1));
        let title: String = toast.title.chars().take(room).collect();
        cx += put(
            buf,
            area,
            cx,
            y + 1,
            &title,
            bg.fg(color).add_modifier(Modifier::BOLD),
        );
        if !detail.is_empty() {
            let room = usize::from((x + w).saturating_sub(cx + 1));
            let mut d: String = format!(" · {detail}");
            if d.chars().count() > room {
                d = d.chars().take(room.saturating_sub(1)).collect::<String>() + "…";
            }
            put(buf, area, cx, y + 1, &d, bg.fg(theme.fg));
        }
        y += H;
    }
}

/// A rounded box in `color` on the panel colour, clipped to `area`.
fn card(buf: &mut Buffer, area: Rect, x: u16, y: u16, w: u16, color: Color, theme: &Theme) {
    let border = Style::default().fg(color).bg(theme.panel);
    for dy in 0..H {
        for dx in 0..w {
            let (cx, cy) = (x + dx, y + dy);
            if cx >= area.right() || cy >= area.bottom() {
                continue;
            }
            let sym = match (dy, dx) {
                (0, 0) => "╭",
                (0, d) if d == w - 1 => "╮",
                (r, 0) if r == H - 1 => "╰",
                (r, d) if r == H - 1 && d == w - 1 => "╯",
                (0, _) | (2, _) => "─",
                (_, 0) => "│",
                (_, d) if d == w - 1 => "│",
                _ => " ",
            };
            if let Some(c) = buf.cell_mut((cx, cy)) {
                c.set_symbol(sym);
                c.set_style(border);
            }
        }
    }
}

/// Write `s` from `(x, y)`, clipped to `area`; returns the columns it took.
fn put(buf: &mut Buffer, area: Rect, x: u16, y: u16, s: &str, style: Style) -> u16 {
    if x >= area.right() {
        return 0;
    }
    let max = usize::from(area.right() - x);
    let (end, _) = buf.set_stringn(x, y, s, max, style);
    end.saturating_sub(x)
}
