//! First start on a computer with nothing in it yet: sign in to bring your
//! tasks from your other computers, make an account, or just carry on
//! here. Key handling lives in `handle_onboard` (main.rs).

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};

use crate::app::App;

pub const WIDTH: u16 = 62;
pub const HEIGHT: u16 = 19;

const CHOICES: &[(&str, &str, &str)] = &[
    (
        "s",
        "Sign in or create an account",
        "your tasks on every computer, end-to-end encrypted",
    ),
    (
        "l",
        "Continue on this computer",
        "everything stays here · sign in later from Settings (,)",
    ),
];

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let theme = app.theme();
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border).bg(theme.panel))
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                "tasq",
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" · welcome ", Style::default().fg(theme.dim)),
        ]))
        .style(Style::default().bg(theme.panel));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<Line> = Vec::new();
    if inner.width >= super::logo::WIDTH {
        lines.extend(super::logo::centered_lines(theme, inner.width));
        lines.push(Line::raw(""));
    }
    lines.push(Line::from(Span::styled(
        "  Already use tasq on another computer?",
        Style::default().fg(theme.fg).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::raw(""));
    for (k, label, hint) in CHOICES {
        lines.push(Line::from(vec![
            Span::raw("   "),
            Span::styled(
                format!(" {k} "),
                Style::default()
                    .bg(theme.mode_bg)
                    .fg(theme.mode_fg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(label.to_string(), Style::default().fg(theme.fg)),
        ]));
        lines.push(Line::from(Span::styled(
            format!("        {hint}"),
            Style::default().fg(theme.dim),
        )));
        lines.push(Line::raw(""));
    }
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}
