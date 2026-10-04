//! The Trash screen: deleted tasks as rows, with when they were deleted.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::App;
use crate::core::KEEP_DAYS;
use crate::ui::task_row::{self, chip_date};

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let theme = app.theme();
    super::fill_bg(frame, area, Style::default().bg(theme.bg));
    let Some(state) = app.trash_screen.as_ref() else {
        return;
    };
    let items = app.trash_items();
    let today = app.today();
    let bold = Style::default().fg(theme.fg).add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(theme.dim);
    let mut lines: Vec<Line> = vec![
        Line::from(vec![
            Span::raw("  "),
            Span::styled("⌫ ", dim),
            Span::styled("Trash", bold),
        ]),
        Line::from(Span::styled(
            format!(
                "  {} {} · kept {KEEP_DAYS} days",
                items.len(),
                if items.len() == 1 { "task" } else { "tasks" }
            ),
            dim,
        )),
        Line::from(Span::styled(
            "  r restore · D delete for good · E empty the trash",
            dim,
        )),
        Line::raw(""),
    ];
    if items.is_empty() {
        lines.push(Line::from(Span::styled("  the trash is empty", dim)));
    }
    let space_color = |p: &str| app.space_color(p);
    let room = usize::from(area.height.saturating_sub(5));
    let skip = state.cursor.saturating_sub(room.saturating_sub(1));
    let parsed: Vec<(usize, crate::todo::Task, &str)> = items
        .iter()
        .enumerate()
        .skip(skip)
        .take(room)
        .filter_map(|(i, it)| {
            crate::todo::parse_line(&it.raw)
                .ok()
                .map(|t| (i, t, it.deleted_on.as_str()))
        })
        .collect();
    for (i, t, on) in &parsed {
        let opts = task_row::RowOpts {
            idx_label: *i,
            cursor: *i == state.cursor,
            today,
            space_color: &space_color,
            ..Default::default()
        };
        let mut line = task_row::build_line(t, opts, theme);
        line.spans.push(Span::styled(
            format!("   deleted {}", chip_date(on, today)),
            dim,
        ));
        lines.push(line);
    }
    frame.render_widget(
        Paragraph::new(lines).style(Style::default().bg(theme.bg).fg(theme.fg)),
        Rect {
            y: area.y + 1,
            height: area.height.saturating_sub(1),
            ..area
        },
    );
}
