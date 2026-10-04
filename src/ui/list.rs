use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::{App, GroupKey, ListDueBucket, Mode, View};
use crate::core::filter;
use crate::theme::Theme;
use crate::ui::{keep_cursor_visible, task_row};

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let theme = app.theme();
    super::fill_bg(frame, area, Style::default().bg(theme.bg));

    let [title_area, body_area] =
        Layout::vertical([Constraint::Length(4), Constraint::Min(1)]).areas(area);
    title_block(frame, title_area, app, theme);

    if app.tasks().is_empty() {
        // The welcome overlay covers the same ground on first run; drawing the
        // empty-state card underneath stacks two boxes on top of each other.
        if app.mode != Mode::Welcome {
            crate::ui::empty::render(frame, body_area, app);
        }
        return;
    }

    // Highlight only the free-text part — a `due:` term filters by date and
    // never appears verbatim in the body.
    let resolved_needle = (!app.filter.search.is_empty())
        .then(|| filter::resolve_needle(&app.filter.search, app.today()));
    let match_term: Option<&str> = resolved_needle
        .as_ref()
        .and_then(|n| (!n.text.is_empty()).then_some(n.text.as_str()));

    let space_color = |p: &str| app.space_color(p);
    let visible = app.visible_indices();
    let groups = app.visible_groups();
    let mut lines: Vec<Line> = Vec::new();
    let mut cursor_line: Option<usize> = None;

    if visible.is_empty() {
        lines.push(Line::from(Span::styled(
            "   no tasks match".to_string(),
            Style::default().fg(theme.dim),
        )));
    } else {
        let blank = super::density_blank_lines(app.prefs.density);
        let counts = group_counts(groups);
        let last = visible.len().saturating_sub(1);
        let mut last_group: Option<&GroupKey> = None;

        for (i, (&abs, gk)) in visible.iter().zip(groups.iter()).enumerate() {
            // Emit a section header on group transitions. `GroupKey::None`
            // means the active sort is `Sort::File`; we never render a header
            // for it, so the layout is identical to the pre-grouping version.
            if !matches!(gk, GroupKey::None) && last_group != Some(gk) {
                if !lines.is_empty() {
                    push_blanks(&mut lines, blank);
                }
                lines.push(group_header(theme, gk, counts.lookup(gk), app.today()));
                last_group = Some(gk);
            }

            let task = &app.tasks()[abs];
            let opts = task_row::RowOpts {
                idx_label: i,
                cursor: i == app.cursor && app.mode != Mode::Help && app.mode != Mode::Settings,
                multi_mode: app.effective_mode() == Mode::Visual,
                multi_checked: app.selection.is_selected(abs),
                selected: app.selection.is_selected(abs),
                show_line_num: app.prefs.layout.line_num,
                match_term,
                today: app.today(),
                hidden_keys: &app.prefs.hidden_keys,
                space_color: &space_color,
                checklist: app.task_notes(task).progress(),
            };
            if i == app.cursor {
                cursor_line = Some(lines.len());
            }
            let mut line = task_row::build_line(task, opts, theme);
            // Upcoming keeps far-off tasks in view under Later, but quiet:
            // dimmed, except the row the cursor is on.
            if matches!(gk, GroupKey::Day(None)) && i != app.cursor {
                dim_line(&mut line, theme);
            }
            lines.push(line);
            if matches!(gk, GroupKey::None) && i != last {
                for _ in 0..blank {
                    lines.push(Line::raw(""));
                }
            }
        }
    }

    let scroll_cell = &app.view_scroll[View::List.idx()];
    let scroll = keep_cursor_visible(
        scroll_cell.get(),
        cursor_line,
        body_area.height,
        lines.len(),
    );
    scroll_cell.set(scroll);

    let para = Paragraph::new(lines)
        .style(Style::default().bg(theme.bg).fg(theme.fg))
        .scroll((scroll, 0));
    frame.render_widget(para, body_area);
}

/// Paint every span of a row in the dim colour, keeping its background.
fn dim_line(line: &mut Line, theme: &Theme) {
    for span in &mut line.spans {
        span.style = span.style.fg(theme.dim).remove_modifier(Modifier::BOLD);
    }
}

fn display_path(p: &std::path::Path) -> String {
    if let Some(home) = std::env::var_os("HOME")
        && let Ok(rel) = p.strip_prefix(&home)
    {
        return format!("~/{}", rel.display());
    }
    p.display().to_string()
}

/// Tally rows per `GroupKey` so each header can show its count without an
/// extra rescan during the render loop. Keyed by a stable string form so
/// `GroupKey::ListPriority(None)` and `ListPriority(Some('A'))` don't collide.
struct GroupCounts {
    inner: std::collections::HashMap<String, usize>,
}

impl GroupCounts {
    fn lookup(&self, gk: &GroupKey) -> usize {
        self.inner.get(&group_count_key(gk)).copied().unwrap_or(0)
    }
}

fn group_counts(groups: &[GroupKey]) -> GroupCounts {
    let mut inner: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for g in groups {
        if matches!(g, GroupKey::None) {
            continue;
        }
        *inner.entry(group_count_key(g)).or_insert(0) += 1;
    }
    GroupCounts { inner }
}

fn group_count_key(gk: &GroupKey) -> String {
    match gk {
        GroupKey::ListPriority(Some(c)) => format!("p:{c}"),
        GroupKey::ListPriority(None) => "p:_".to_string(),
        GroupKey::ListDue(b) => format!("d:{}", b.label()),
        GroupKey::Day(d) => format!("day:{}", d.as_deref().unwrap_or("later")),
        GroupKey::Slot(s) => format!("slot:{}", s.label()),
        // Not produced for List view; encode defensively.
        GroupKey::ArchiveDate(d) => format!("a:{d}"),
        GroupKey::None => String::new(),
    }
}

fn group_header<'a>(theme: &Theme, gk: &GroupKey, count: usize, today: &str) -> Line<'a> {
    let (label, color) = match gk {
        GroupKey::Slot(crate::app::TodaySlot::Late) => ("OVERDUE".to_string(), theme.overdue),
        GroupKey::Slot(s) => (s.label().to_string(), theme.dim),
        GroupKey::Day(Some(d)) => (day_label(d, today), theme.accent),
        GroupKey::Day(None) => ("LATER".to_string(), theme.dim),
        GroupKey::ListPriority(Some(c)) => (format!("PRIORITY {c}"), theme.priority_color(*c)),
        GroupKey::ListPriority(None) => ("NO PRIORITY".to_string(), theme.dim),
        GroupKey::ListDue(b) => (b.label().to_string(), due_bucket_color(theme, *b)),
        // Defensive fallthrough — not produced under List view.
        GroupKey::ArchiveDate(d) => (d.clone(), theme.accent),
        GroupKey::None => (String::new(), theme.fg),
    };

    // A quiet heading: the label, how many, and a hairline to the edge.
    let used = label.chars().count() + count.to_string().len() + 6;
    Line::from(vec![
        Span::raw("  "),
        Span::styled(
            label,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("  {count}  "), Style::default().fg(theme.dim)),
        Span::styled(
            "─".repeat(160usize.saturating_sub(used)),
            Style::default().fg(theme.border),
        ),
    ])
}

/// What the list is showing: its name in bold with the date, a progress bar
/// (Today) or how many tasks there are, and the active filters as chips.
fn title_block(frame: &mut Frame, area: Rect, app: &App, theme: &Theme) {
    use crate::app::{Preset, Scope};
    let today = app.today_naive();
    let bold = Style::default().fg(theme.fg).add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(theme.dim);

    // Row 1: the name.
    let mut name: Vec<Span> = vec![Span::raw("  ")];
    if let Some(p) = app.filter.preset {
        let color = match p {
            Preset::HighPriority => theme.pri_a,
            Preset::Starred => theme.matched,
            Preset::Overdue => theme.overdue,
            Preset::Inbox => theme.accent,
        };
        name.push(Span::styled(
            format!("{} ", p.icon()),
            Style::default().fg(color),
        ));
        name.push(Span::styled(p.label().to_string(), bold));
    } else if let Some(p) = &app.filter.project {
        name.push(Span::styled("● ", Style::default().fg(app.space_color(p))));
        name.push(Span::styled(crate::core::spaces::display(p), bold));
    } else if !app.store.is_db() {
        name.push(Span::styled(display_path(&app.file_path), bold));
    } else {
        let label = match app.prefs.scope {
            Scope::Today => "Today",
            Scope::Upcoming => "Upcoming",
            Scope::All => "All tasks",
        };
        name.push(Span::styled(label.to_string(), bold));
    }
    if app.prefs.scope == Scope::Today || app.filter.project.is_none() {
        // The date, only when it fits whole.
        let date = format!(
            "   {}",
            today.format("%A %-d %B").to_string().to_lowercase()
        );
        let used: usize = name.iter().map(|s| s.content.chars().count()).sum();
        if used + date.chars().count() <= usize::from(area.width) {
            name.push(Span::styled(date, dim));
        }
    }

    // Row 2: progress for Today, else a count and the sort.
    let visible = app.visible_indices();
    let mut info: Vec<Span> = vec![Span::raw("  ")];
    if app.prefs.scope == Scope::Today && app.filter.preset.is_none() {
        let done = visible.iter().filter(|&&i| app.tasks()[i].done).count();
        let total = visible.len();
        const BAR: usize = 16;
        let filled = (done * BAR).checked_div(total).unwrap_or(0);
        info.push(Span::styled(
            "━".repeat(filled),
            Style::default().fg(theme.pri_c),
        ));
        info.push(Span::styled(
            "━".repeat(BAR - filled),
            Style::default().fg(theme.border),
        ));
        let planned: u32 = visible
            .iter()
            .filter_map(|&i| {
                let t = &app.tasks()[i];
                crate::todo::find_kv(&t.clean_raw, "at")?;
                t.duration
                    .as_deref()
                    .and_then(crate::duration::parse_minutes)
            })
            .sum();
        let mut text = format!("  {done} of {total} done");
        if planned > 0 {
            text.push_str(&format!(
                " · {} planned",
                crate::duration::describe(planned)
            ));
        }
        info.push(Span::styled(text, dim));
    } else {
        let n = visible.len();
        info.push(Span::styled(
            format!(
                "{n} {}  ·  sort: {}",
                if n == 1 { "task" } else { "tasks" },
                app.sort_label()
            ),
            dim,
        ));
    }

    // Row 3: the active filters as chips you can clear, then the way to add one.
    let mut chips: Vec<Span> = vec![Span::raw("  ")];
    let chip = |text: String, color: Color| -> Span<'static> {
        let mut style = Style::default().fg(color);
        if let Some(bg) = crate::ui::task_row::tint(color, theme.bg, 0.2) {
            style = style.bg(bg);
        }
        Span::styled(format!(" {text} × "), style)
    };
    if let Some(p) = &app.filter.project {
        chips.push(chip(
            format!("● {}", crate::core::spaces::display(p)),
            app.space_color(p),
        ));
        chips.push(Span::raw(" "));
    }
    if let Some(c) = &app.filter.context {
        chips.push(chip(format!("@{c}"), theme.context));
        chips.push(Span::raw(" "));
    }
    if !app.filter.search.is_empty() {
        chips.push(chip(format!("⌕ {}", app.filter.search), theme.accent));
        chips.push(Span::raw(" "));
    }
    if let Some(p) = app.filter.preset {
        chips.push(chip(
            format!("{} {}", p.icon(), p.label().to_lowercase()),
            theme.accent,
        ));
        chips.push(Span::raw(" "));
    }
    // The way to add one, lit while its popover is open.
    if app.mode == Mode::Filters {
        chips.push(Span::styled(
            " + filter ",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ));
    } else {
        chips.push(Span::styled("+ filter", dim));
    }

    let lines = vec![Line::from(name), Line::from(info), Line::from(chips)];
    frame.render_widget(
        Paragraph::new(lines).style(Style::default().bg(theme.bg)),
        Rect {
            height: area.height.min(3),
            ..area
        },
    );
}

/// An Upcoming day header: `TOMORROW`, else `FRI 9 OCT`.
fn day_label(date: &str, today: &str) -> String {
    let parse = |s: &str| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok();
    match (parse(date), parse(today)) {
        (Some(d), Some(t)) if (d - t).num_days() == 1 => "TOMORROW".to_string(),
        (Some(d), _) => d.format("%a %-d %b").to_string().to_uppercase(),
        _ => date.to_string(),
    }
}

fn due_bucket_color(theme: &Theme, b: ListDueBucket) -> Color {
    match b {
        ListDueBucket::Overdue => theme.overdue,
        ListDueBucket::Today => theme.today,
        ListDueBucket::ThisWeek => theme.accent,
        ListDueBucket::NextWeek => theme.accent,
        ListDueBucket::Later => theme.accent,
        ListDueBucket::NoDue => theme.dim,
    }
}

fn push_blanks(lines: &mut Vec<Line>, n: usize) {
    for _ in 0..n {
        lines.push(Line::raw(" "));
    }
}
