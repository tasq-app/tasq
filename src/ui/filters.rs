use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::{App, Filter, Scope, ordered_unique};
use crate::core::filter;
use crate::theme::Theme;

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let theme = app.theme();
    super::fill_bg(frame, area, Style::default().bg(theme.panel));

    let spaces = filter::spaces_tree(app.tasks());
    let contexts = ordered_unique(app.tasks(), |t| &t.contexts);

    let mut lines: Vec<Line> = Vec::new();
    lines.push(line_pad(
        theme,
        vec![Span::styled(
            " VIEWS",
            Style::default().fg(theme.dim).add_modifier(Modifier::BOLD),
        )],
    ));
    // Open tasks in each scope, before any space/tag/search filter.
    let today = app.today();
    let in_list = app.view == crate::app::View::List;
    for (scope, label, key) in [
        (Scope::Today, "Today", "1"),
        (Scope::Upcoming, "Upcoming", "2"),
        (Scope::All, "All", "3"),
    ] {
        let count = app
            .tasks()
            .iter()
            .filter(|t| {
                filter::in_scope(t, scope, today)
                    && filter::list_predicate(
                        t,
                        false,
                        app.prefs.show_future,
                        today,
                        &Filter::default(),
                        None,
                    )
            })
            .count();
        let active = in_list && app.prefs.scope == scope;
        let mut row = filter_row(theme, "", label, count, active, theme.fg);
        row.spans.push(Span::styled(
            format!(" {key}"),
            Style::default().fg(theme.dim),
        ));
        lines.push(row);
    }
    lines.push(line_pad(theme, vec![Span::raw(" ")]));
    lines.push(line_pad(
        theme,
        vec![Span::styled(
            " SPACES",
            Style::default()
                .fg(theme.project)
                .add_modifier(Modifier::BOLD),
        )],
    ));
    if spaces.is_empty() {
        lines.push(hint_row(theme, "+space", theme.project));
    } else {
        // Sub-spaces sit indented under their parent; a space's count
        // includes them.
        for row in &spaces {
            let active = app.filter.project.as_deref() == Some(row.path.as_str());
            let indent = "  ".repeat(row.depth);
            let label = format!("{indent}{}", filter::space_leaf(&row.path));
            lines.push(filter_row(
                theme,
                "",
                &label,
                row.count,
                active,
                theme.project,
            ));
        }
    }
    lines.push(line_pad(theme, vec![Span::raw(" ")]));
    lines.push(line_pad(
        theme,
        vec![Span::styled(
            " TAGS",
            Style::default()
                .fg(theme.context)
                .add_modifier(Modifier::BOLD),
        )],
    ));
    if contexts.is_empty() {
        lines.push(hint_row(theme, "@context", theme.context));
    } else {
        for (name, count) in &contexts {
            let active = app.filter.context.as_deref() == Some(name.as_str());
            lines.push(filter_row(theme, "@", name, *count, active, theme.context));
        }
    }

    let saved = app.saved_filters();
    if !saved.is_empty() {
        lines.push(line_pad(theme, vec![Span::raw(" ")]));
        lines.push(line_pad(
            theme,
            vec![Span::styled(
                " SAVED",
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            )],
        ));
        let today = app.today();
        let default_filter = Filter::default();
        for f in saved {
            let active = app.filter().search == f.query;
            // Same predicate the list view uses, so a saved `due:` query
            // counts by date range instead of its own literal text.
            let needle = filter::resolve_needle(&f.query, today);
            let count = app
                .tasks()
                .iter()
                .filter(|t| {
                    !t.done && filter::passes_user_filter(t, &default_filter, Some(&needle))
                })
                .count();
            lines.push(filter_row(theme, "", &f.name, count, active, theme.accent));
        }
    }

    let para = Paragraph::new(lines).style(Style::default().bg(theme.panel).fg(theme.fg));
    frame.render_widget(para, area);
}

fn filter_row<'a>(
    theme: &Theme,
    sigil: &str,
    name: &str,
    count: usize,
    active: bool,
    sigil_color: ratatui::style::Color,
) -> Line<'a> {
    let bg = if active { theme.selected } else { theme.panel };
    let prefix = if active { "▸ " } else { "  " };
    let mut padded = format!("{}{}", sigil, name);
    if padded.chars().count() < 16 {
        let pad = 16 - padded.chars().count();
        padded.push_str(&" ".repeat(pad));
    }
    Line::from(vec![
        Span::raw(" "),
        Span::styled(prefix.to_string(), Style::default().fg(theme.accent)),
        Span::styled(padded, Style::default().fg(sigil_color)),
        Span::styled(format!("{:>3}", count), Style::default().fg(theme.dim)),
    ])
    .style(Style::default().bg(bg))
}

fn hint_row<'a>(theme: &Theme, token: &'a str, token_color: ratatui::style::Color) -> Line<'a> {
    Line::from(vec![
        Span::raw("   "),
        Span::styled("tag with ", Style::default().fg(theme.dim)),
        Span::styled(token, Style::default().fg(token_color)),
    ])
    .style(Style::default().bg(theme.panel))
}

fn line_pad<'a>(theme: &Theme, spans: Vec<Span<'a>>) -> Line<'a> {
    Line::from(spans).style(Style::default().bg(theme.panel))
}
