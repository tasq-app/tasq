//! The colour picker for a space: the palette as a small grid, the one
//! under the cursor bracketed, and a hex field when `c` is pressed again.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};

use crate::app::{App, Hit, PICK_COLS};
use crate::core::spaces::{PALETTE_SLOTS, SpaceColor};

/// Columns one swatch takes: ` ███ ` with room for the brackets.
const CELL_W: u16 = 5;

pub fn render(frame: &mut Frame, screen: Rect, app: &App) {
    let Some(pick) = app.color_pick.as_ref() else {
        return;
    };
    let theme = app.theme();
    let rows = PALETTE_SLOTS.div_ceil(PICK_COLS) as u16;
    let grid_w = CELL_W * PICK_COLS as u16;
    let w = (grid_w + 4).min(screen.width);
    let h = (rows * 2 + 6).min(screen.height);
    let r = Rect {
        x: screen.x + screen.width.saturating_sub(w) / 2,
        y: screen.y + screen.height.saturating_sub(h) / 2,
        width: w,
        height: h,
    };
    frame.render_widget(Clear, r);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.accent))
        .title(Span::styled(
            format!(" {} ", crate::core::spaces::display(&pick.path)),
            Style::default().fg(theme.fg).add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(theme.panel));
    let inner = block.inner(r);
    frame.render_widget(block, r);

    let palette = theme.palette();
    let current = app.store.space_color(&pick.path);
    let gx = inner.x + inner.width.saturating_sub(grid_w) / 2;
    let mut lines: Vec<Line> = vec![Line::raw("")];
    for row in 0..rows as usize {
        if row > 0 {
            lines.push(Line::raw(""));
        }
        let mut spans = Vec::new();
        for col in 0..PICK_COLS {
            let i = row * PICK_COLS + col;
            if i >= PALETTE_SLOTS {
                break;
            }
            let on = pick.hex.is_none() && i == pick.cursor;
            let mark = |c: &'static str| {
                Span::styled(
                    if on { c } else { " " },
                    Style::default().fg(theme.fg).add_modifier(Modifier::BOLD),
                )
            };
            spans.push(mark("["));
            // The space's colour now has a dot in it.
            spans.push(if current == SpaceColor::Slot(i) {
                Span::styled(" ● ", Style::default().fg(theme.panel).bg(palette[i]))
            } else {
                Span::styled("███", Style::default().fg(palette[i]))
            });
            spans.push(mark("]"));
            let y = inner.y + 1 + 2 * row as u16;
            let x = gx + CELL_W * col as u16;
            app.hits.add(
                Rect {
                    x,
                    y,
                    width: CELL_W,
                    height: 1,
                }
                .intersection(inner),
                Hit::Swatch(i),
            );
        }
        lines.push(Line::from(spans).centered());
    }
    lines.push(Line::raw(""));
    match pick.hex.as_ref() {
        Some(hex) => {
            let typed = app.color_pick_typed();
            let mut spans = vec![
                Span::styled("custom  #", Style::default().fg(theme.dim)),
                Span::styled(format!("{hex}▏"), Style::default().fg(theme.fg)),
                Span::raw("  "),
            ];
            if let Some(c) = typed {
                spans.push(Span::styled(
                    "███",
                    Style::default().fg(theme.space_color(c)),
                ));
            }
            lines.push(Line::from(spans).centered());
            lines.push(Line::raw(""));
            lines.push(
                Line::styled(
                    "Enter use it · Esc back to the grid",
                    Style::default().fg(theme.dim),
                )
                .centered(),
            );
        }
        None => {
            if let SpaceColor::Rgb(..) = current {
                lines.push(
                    Line::from(vec![
                        Span::styled("now  ", Style::default().fg(theme.dim)),
                        Span::styled(
                            format!("{} ███", current.to_value()),
                            Style::default().fg(theme.space_color(current)),
                        ),
                    ])
                    .centered(),
                );
            } else {
                lines.push(Line::raw(""));
            }
            lines.push(Line::raw(""));
            lines.push(
                Line::styled(
                    "Enter pick · c hex · C automatic · Esc",
                    Style::default().fg(theme.dim),
                )
                .centered(),
            );
        }
    }
    frame.render_widget(
        Paragraph::new(lines).style(Style::default().bg(theme.panel)),
        inner,
    );
}
