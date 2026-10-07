//! The Notes screen: the list of notes on the left with a search box, and
//! the selected note rendered on the right, with the tasks that link to it
//! at the bottom.

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Paragraph, Wrap};

use crate::app::App;

const LIST_W: u16 = 34;

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let theme = app.theme();
    super::fill_bg(frame, area, Style::default().bg(theme.bg));
    let Some(state) = app.notes_screen.as_ref() else {
        return;
    };
    if area.width < 40 || area.height < 8 {
        return;
    }
    let entries = app.note_entries();
    // Reading a note: it takes the whole screen.
    let list_w = if state.reading {
        0
    } else {
        LIST_W.min(area.width / 2)
    };
    let list = Rect {
        width: list_w,
        ..area
    };
    let doc = if state.reading {
        Rect {
            x: area.x + area.width / 8,
            width: area.width - area.width / 4,
            ..area
        }
    } else {
        Rect {
            x: area.x + list_w + 1,
            width: area.width - list_w - 1,
            ..area
        }
    };
    let buf = frame.buffer_mut();
    let bg = Style::default().bg(theme.bg);
    for y in (area.top()..area.bottom()).filter(|_| !state.reading) {
        if let Some(c) = buf.cell_mut((area.x + list_w, y)) {
            c.set_symbol("│");
            c.set_style(bg.fg(theme.border));
        }
    }

    // The search box.
    let q = Rect {
        x: list.x + 1,
        y: list.y + 1,
        width: list.width.saturating_sub(2),
        height: 3,
    };
    let border = if state.searching || state.naming.is_some() {
        theme.accent
    } else {
        theme.border
    };
    if !state.reading {
        rounded(buf, q, bg.fg(border));
        app.hits.add(q, crate::app::Hit::NotesSearch);
    }
    let text = if let Some(name) = &state.naming {
        format!("+ new note: {name}▏")
    } else if state.query.is_empty() && !state.searching {
        match &state.task {
            Some(t) => format!("⌕ notes of {}", t.title),
            None => "⌕ search notes   /".to_string(),
        }
    } else if state.searching {
        format!("⌕ {}▏", state.query)
    } else {
        format!("⌕ {}", state.query)
    };
    let color = if state.naming.is_some() {
        theme.accent
    } else if state.query.is_empty() && !state.searching {
        theme.dim
    } else {
        theme.fg
    };
    if !state.reading {
        put(
            buf,
            q.x + 2,
            q.y + 1,
            &fit(&text, q.width - 4),
            q.width - 4,
            bg.fg(color),
        );
    }

    // The notes, two rows each.
    let top = q.bottom() + 1;
    let room = if state.reading {
        0
    } else {
        usize::from(list.bottom().saturating_sub(top) / 2)
    };
    if entries.is_empty() && !state.reading {
        let msg = if !state.query.is_empty() {
            "nothing matches"
        } else if state.task.is_some() {
            "no notes yet — a adds one"
        } else {
            "no notes yet — o on a task"
        };
        put(buf, list.x + 2, top, msg, list.width - 3, bg.fg(theme.dim));
    }
    let skip = state.cursor.saturating_sub(room.saturating_sub(1));
    for (n, e) in entries.iter().enumerate().skip(skip).take(room) {
        let y = top + 2 * (n - skip) as u16;
        let here = n == state.cursor;
        let row = if here { bg.bg(theme.cursor) } else { bg };
        app.hits.add(
            Rect {
                x: list.x + 1,
                y,
                width: list.width.saturating_sub(2),
                height: 2,
            },
            crate::app::Hit::NoteRow(n),
        );
        for dy in 0..2 {
            for x in list.x + 1..list.right() - 1 {
                if let Some(c) = buf.cell_mut((x, y + dy)) {
                    c.set_symbol(" ");
                    c.set_style(row);
                }
            }
        }
        if here {
            put(buf, list.x + 1, y, "▎", 1, row.fg(theme.accent));
            put(buf, list.x + 1, y + 1, "▎", 1, row.fg(theme.accent));
        }
        let w = list.width.saturating_sub(5);
        // The task's main note wears a star.
        let main = app.is_main_note(&e.path);
        let mut tx = list.x + 3;
        if main {
            tx += put(buf, tx, y, "★ ", 2, row.fg(theme.matched));
        }
        let tw = w.saturating_sub(tx - list.x - 3);
        put(
            buf,
            tx,
            y,
            &fit(&e.title, tw),
            tw,
            row.fg(theme.fg).add_modifier(Modifier::BOLD),
        );
        let mut x = list.x + 3;
        if let Some(s) = &e.space {
            x += put(buf, x, y + 1, "● ", 2, row.fg(app.space_color(s)));
            let name = crate::core::spaces::display(s);
            x += put(
                buf,
                x,
                y + 1,
                &name,
                w.saturating_sub(x - list.x),
                row.fg(theme.dim),
            );
            x += put(buf, x, y + 1, " · ", 3, row.fg(theme.dim));
        }
        let rest = (list.x + 3 + w).saturating_sub(x);
        put(buf, x, y + 1, &e.when, rest, row.fg(theme.dim));
    }

    // The note: in the built-in editor when it's open there.
    if let Some(editor) = state.editor.as_ref() {
        let r = Rect {
            x: doc.x + 1,
            width: doc.width.saturating_sub(1),
            ..doc
        };
        super::note_editor::render_editor(frame, r, theme, editor, true);
        return;
    }
    let Some(e) = entries.get(state.cursor) else {
        return;
    };
    let inner = Rect {
        x: doc.x + 2,
        y: doc.y + 1,
        width: doc.width.saturating_sub(4),
        height: doc.height.saturating_sub(1),
    };
    put(
        buf,
        inner.x,
        inner.y,
        &e.title,
        inner.width,
        bg.fg(theme.fg).add_modifier(Modifier::BOLD),
    );
    put(
        buf,
        inner.x,
        inner.y + 1,
        &if state.reading {
            format!(
                "edited {} · e edit · Tab next note · G end · t its task · Esc back",
                e.when
            )
        } else if state.task.is_some() {
            format!(
                "edited {} · Enter read · e edit · a new · * main · t its task",
                e.when
            )
        } else {
            format!(
                "edited {} · Enter read · e edit · E $EDITOR · p pin · t its task",
                e.when
            )
        },
        inner.width,
        bg.fg(theme.dim),
    );
    // Who links here, at the bottom.
    let links: Vec<String> = app
        .note_tasks(&e.path)
        .iter()
        .map(|t| crate::todo::body_only(&t.raw))
        .collect();
    let foot = if links.is_empty() { 0 } else { 2 };
    let body = Rect {
        x: inner.x,
        y: inner.y + 3,
        width: inner.width,
        height: inner.height.saturating_sub(3 + foot),
    };
    // The heading is drawn above; the rest goes through Markdown.
    let text: String = {
        let mut lines = e.body.lines().peekable();
        if lines.peek().is_some_and(|l| l.starts_with("# ")) {
            lines.next();
        }
        lines.collect::<Vec<_>>().join("\n")
    };
    let rendered = super::markdown::render(
        text.trim_start_matches('\n'),
        usize::from(body.width),
        theme,
    );
    // The search, found in the note: n / N walk its hits.
    let query = state.query.trim().to_lowercase();
    let hit_lines: Vec<usize> = if query.is_empty() {
        Vec::new()
    } else {
        rendered
            .lines
            .iter()
            .enumerate()
            .filter(|(_, l)| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
                    .to_lowercase()
                    .contains(&query)
            })
            .map(|(i, _)| i)
            .collect()
    };
    let current = state
        .hit
        .filter(|_| !hit_lines.is_empty())
        .map(|h| hit_lines[h % hit_lines.len()]);
    // How far it can go, for the keys and the wheel.
    let max = rendered
        .lines
        .len()
        .saturating_sub(usize::from(body.height));
    state.max_scroll.set(max.min(usize::from(u16::MAX)) as u16);
    let scroll = current
        .map_or(state.scroll, |l| l.saturating_sub(2) as u16)
        .min(state.max_scroll.get());
    let para = Paragraph::new(rendered.lines)
        .style(bg.fg(theme.fg))
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    frame.render_widget(para, body);
    if !query.is_empty() {
        let n = query.chars().count();
        let buf = frame.buffer_mut();
        for y in body.top()..body.bottom() {
            let cells: Vec<String> = (body.left()..body.right())
                .map(|x| buf[(x, y)].symbol().to_lowercase())
                .collect();
            let is_current = current.is_some_and(|l| l as u16 == scroll + (y - body.y));
            let mut i = 0;
            while i + n <= cells.len() {
                if cells[i..i + n].concat() == query {
                    for k in i..i + n {
                        let c = &mut buf[(body.x + k as u16, y)];
                        let mark = if is_current {
                            theme.accent
                        } else {
                            theme.matched
                        };
                        c.set_bg(mark);
                        c.set_fg(theme.bg);
                    }
                    i += n;
                } else {
                    i += 1;
                }
            }
        }
        if !hit_lines.is_empty() {
            let at = state.hit.map_or(0, |h| h % hit_lines.len() + 1);
            let label = format!(" {at}/{} · n N ", hit_lines.len());
            let w = label.chars().count() as u16;
            put(
                buf,
                inner.right().saturating_sub(w),
                inner.y,
                &label,
                w,
                bg.fg(theme.matched),
            );
        }
    }
    if foot > 0 {
        let buf = frame.buffer_mut();
        let y = inner.bottom() - 2;
        put(
            buf,
            inner.x,
            y,
            &"╌".repeat(usize::from(inner.width)),
            inner.width,
            bg.fg(theme.border),
        );
        let mut x = inner.x;
        x += put(buf, x, y + 1, "linked from", inner.width, bg.fg(theme.dim));
        for l in links {
            let room = inner.right().saturating_sub(x);
            x += put(buf, x, y + 1, " · ", room, bg.fg(theme.dim));
            let room = inner.right().saturating_sub(x);
            x += put(buf, x, y + 1, &l, room, bg.fg(theme.status_fg));
        }
    }
}

fn fit(s: &str, w: u16) -> String {
    let w = usize::from(w);
    if s.chars().count() <= w {
        return s.to_string();
    }
    if w == 0 {
        return String::new();
    }
    s.chars().take(w - 1).collect::<String>() + "…"
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
