//! The Home screen: a greeting, a capture bar, and tiles for today, the
//! week, your routines, your spaces, recent notes and the inbox.

use chrono::Datelike;
use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use crate::app::{App, DayMark};
use crate::theme::Theme;
use crate::todo;
use crate::ui::task_row::tint;

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let theme = app.theme();
    super::fill_bg(frame, area, Style::default().bg(theme.bg));
    if area.width < 30 || area.height < 12 {
        return;
    }
    let buf = frame.buffer_mut();
    let bg = Style::default().bg(theme.bg);
    let inner = Rect {
        x: area.x + 3,
        y: area.y + 1,
        width: area.width - 6,
        height: area.height - 1,
    };

    // Greeting and date.
    put(
        buf,
        inner.x,
        inner.y,
        &app.greeting(),
        inner.width,
        bg.fg(theme.fg).add_modifier(Modifier::BOLD),
    );
    let date = app
        .today_naive()
        .format("%A %-d %B")
        .to_string()
        .to_lowercase();
    let dw = date.chars().count() as u16;
    if dw + 20 < inner.width {
        put(
            buf,
            inner.right() - dw,
            inner.y,
            &date,
            dw,
            bg.fg(theme.dim),
        );
    }

    // The capture bar.
    let cap = Rect {
        x: inner.x,
        y: inner.y + 2,
        width: inner.width,
        height: 3,
    };
    rounded(buf, cap, Style::default().bg(theme.panel).fg(theme.border));
    let pbg = Style::default().bg(theme.panel);
    let mut x = cap.x + 2;
    x += put(
        buf,
        x,
        cap.y + 1,
        "+ Add a task…  ",
        cap.width - 8,
        pbg.fg(theme.status_fg),
    );
    put(
        buf,
        x,
        cap.y + 1,
        "\"repasar tema 4 tomorrow at 10 in exams\"",
        cap.right().saturating_sub(x + 6),
        pbg.fg(theme.dim),
    );
    put(
        buf,
        cap.right() - 5,
        cap.y + 1,
        " n ",
        3,
        Style::default().bg(theme.cursor).fg(theme.fg),
    );

    // The tiles.
    let top = cap.bottom() + 1;
    let grid = Rect {
        x: inner.x,
        y: top,
        width: inner.width,
        height: inner.bottom().saturating_sub(top + 1),
    };
    let cols: u16 = if grid.width >= 96 {
        3
    } else if grid.width >= 60 {
        2
    } else {
        1
    };
    let rows = 6u16.div_ceil(cols);
    let gap = 2;
    let tw = (grid.width - gap * (cols - 1)) / cols;
    let th = (grid.height.saturating_sub(rows - 1)) / rows;
    if th < 4 {
        return;
    }
    let tiles: [fn(&mut Buffer, Rect, &App, &Theme); 6] =
        [today, week, routines, spaces_tile, notes, inbox];
    for (i, draw) in tiles.iter().enumerate() {
        let i = i as u16;
        let r = Rect {
            x: grid.x + (i % cols) * (tw + gap),
            y: grid.y + (i / cols) * (th + 1),
            width: tw,
            height: th,
        };
        rounded(buf, r, Style::default().bg(theme.panel).fg(theme.border));
        let body = Rect {
            x: r.x + 2,
            y: r.y + 1,
            width: r.width.saturating_sub(4),
            height: r.height.saturating_sub(2),
        };
        draw(buf, body, app, theme);
    }
}

/// A tile's heading, with something dim on the right.
fn heading(buf: &mut Buffer, r: Rect, theme: &Theme, title: &str, right: &str) {
    let pbg = Style::default().bg(theme.panel);
    put(
        buf,
        r.x,
        r.y,
        title,
        r.width,
        pbg.fg(theme.dim).add_modifier(Modifier::BOLD),
    );
    let w = right.chars().count() as u16;
    if w + title.chars().count() as u16 + 2 <= r.width {
        put(buf, r.right() - w, r.y, right, w, pbg.fg(theme.dim));
    }
}

fn today(buf: &mut Buffer, r: Rect, app: &App, theme: &Theme) {
    let pbg = Style::default().bg(theme.panel);
    let (done, total, open) = app.home_today();
    heading(buf, r, theme, "TODAY", "");
    // Progress on the right: "2/6 ━━━━──".
    const BAR: usize = 8;
    let filled = (done * BAR).checked_div(total).unwrap_or(0);
    let label = format!("{done}/{total} ");
    let w = label.chars().count() as u16 + BAR as u16;
    if w + 7 <= r.width {
        let x = r.right() - w;
        let lw = put(buf, x, r.y, &label, w, pbg.fg(theme.dim));
        put(
            buf,
            x + lw,
            r.y,
            &"━".repeat(filled),
            BAR as u16,
            pbg.fg(theme.ok),
        );
        put(
            buf,
            x + lw + filled as u16,
            r.y,
            &"━".repeat(BAR - filled),
            BAR as u16,
            pbg.fg(theme.border),
        );
    }
    let today = app.today();
    if open.is_empty() {
        let msg = if total > 0 {
            "all done for today ✓"
        } else {
            "nothing planned today"
        };
        put(buf, r.x, r.y + 2, msg, r.width, pbg.fg(theme.dim));
        return;
    }
    for (y, &i) in (r.y + 2..r.bottom()).zip(open.iter()) {
        let t = &app.tasks()[i];
        let mut x = r.x;
        x += put(buf, x, y, "☐ ", 2, pbg.fg(theme.dim));
        if let Some(p) = t.priority {
            x += put(buf, x, y, "⚑ ", 2, pbg.fg(theme.priority_color(p)));
        }
        let late = t.date().is_some_and(|d| d < today);
        let chip = if late {
            Some(("late".to_string(), theme.overdue))
        } else {
            todo::find_kv(&t.clean_raw, "at").map(|a| (a, theme.accent))
        };
        let cw = chip
            .as_ref()
            .map_or(0, |(c, _)| c.chars().count() as u16 + 2);
        let title = todo::body_only(&t.raw);
        let room = r.right().saturating_sub(x + cw);
        put(buf, x, y, &fit(&title, room), room, pbg.fg(theme.fg));
        if let Some((c, color)) = chip {
            put(buf, r.right() - cw + 2, y, &c, cw, pbg.fg(color));
        }
    }
}

fn week(buf: &mut Buffer, r: Rect, app: &App, theme: &Theme) {
    let pbg = Style::default().bg(theme.panel);
    let days = app.home_week();
    let Some(first) = days.first() else {
        return;
    };
    let last = days.last().map_or(first.date, |d| d.date);
    let range = if first.date.month() == last.month() {
        format!(
            "{} – {}",
            first.date.format("%b %-d").to_string().to_lowercase(),
            last.day()
        )
    } else {
        format!(
            "{} – {}",
            first.date.format("%b %-d").to_string().to_lowercase(),
            last.format("%b %-d").to_string().to_lowercase()
        )
    };
    heading(buf, r, theme, "THIS WEEK", &range);
    let today = app.today_naive();
    let max = days.iter().map(|d| d.count).max().unwrap_or(0).max(1);
    let cell = (r.width / 7).clamp(2, 5);
    let labels = ["m", "t", "w", "t", "f", "s", "s"];
    let label = |d: chrono::NaiveDate| labels[d.weekday().num_days_from_monday() as usize];
    for (k, d) in days.iter().enumerate() {
        let x = r.x + k as u16 * cell;
        let is_today = d.date == today;
        let label_style = if is_today {
            pbg.fg(theme.fg).add_modifier(Modifier::BOLD)
        } else {
            pbg.fg(theme.dim)
        };
        put(buf, x, r.y + 2, label(d.date), 1, label_style);
        let level = if d.count == 0 {
            0.0
        } else {
            0.3 + 0.7 * d.count as f32 / max as f32
        };
        let color = if level == 0.0 {
            theme.cursor
        } else {
            tint(theme.accent, theme.panel, level).unwrap_or(theme.accent)
        };
        let block = "█".repeat(usize::from(cell - 1));
        put(buf, x, r.y + 3, &block, cell - 1, pbg.fg(color));
        if is_today {
            put(
                buf,
                x,
                r.y + 4,
                &"▔".repeat(usize::from(cell - 1)),
                cell - 1,
                pbg.fg(theme.fg),
            );
        }
    }
    // The busiest days, in words.
    let mut busy: Vec<&crate::app::HeatDay> = days.iter().filter(|d| d.count > 0).collect();
    busy.sort_by_key(|d| std::cmp::Reverse(d.count));
    let top: Vec<String> = busy
        .iter()
        .take(2)
        .filter(|d| d.count == busy[0].count || d.count * 2 > busy[0].count)
        .map(|d| d.date.format("%a").to_string().to_lowercase())
        .collect();
    let line = match top.as_slice() {
        [] => "a free week ahead".to_string(),
        [one] => format!("{one} is your busiest day"),
        [a, b, ..] => format!("{a} and {b} are your busiest days"),
    };
    put(buf, r.x, r.y + 5, &line, r.width, pbg.fg(theme.dim));
    if r.height > 8 {
        put(
            buf,
            r.x,
            r.y + 7,
            "NEXT UP",
            r.width,
            pbg.fg(theme.dim).add_modifier(Modifier::BOLD),
        );
        let next = app.home_next_up().map_or_else(
            || "nothing else with a time today".to_string(),
            |(at, title, till)| format!("{at} {title} · {till}"),
        );
        put(
            buf,
            r.x,
            r.y + 8,
            &fit(&next, r.width),
            r.width,
            pbg.fg(theme.status_fg),
        );
    }
}

fn routines(buf: &mut Buffer, r: Rect, app: &App, theme: &Theme) {
    let pbg = Style::default().bg(theme.panel);
    heading(buf, r, theme, "ROUTINES", "");
    let list = app.routines();
    if list.is_empty() {
        put(
            buf,
            r.x,
            r.y + 2,
            "repeat a task to track it here",
            r.width,
            pbg.fg(theme.dim),
        );
        return;
    }
    let name_w = (r.width / 3).clamp(6, 16);
    for (n, rt) in list.iter().enumerate() {
        let y = r.y + 2 + n as u16;
        if y >= r.bottom() {
            break;
        }
        put(
            buf,
            r.x,
            y,
            &fit(&rt.title, name_w - 1),
            name_w - 1,
            pbg.fg(theme.fg),
        );
        let mut x = r.x + name_w;
        for m in rt.week {
            let (g, c) = match m {
                DayMark::Done => ("▮", theme.ok),
                DayMark::Missed => ("▯", theme.overdue),
                DayMark::Pending => ("▯", theme.dim),
                DayMark::Off => ("·", theme.border),
            };
            x += put(buf, x, y, g, 1, pbg.fg(c));
            x += put(buf, x, y, " ", 1, pbg);
        }
        let streak = if rt.daily {
            format!("{}d", rt.streak)
        } else {
            format!("{}×", rt.streak)
        };
        let sw = streak.chars().count() as u16;
        let color = if rt.streak > 0 { theme.ok } else { theme.dim };
        if x + sw < r.right() {
            put(
                buf,
                r.right() - sw,
                y,
                &streak,
                sw,
                pbg.fg(color).add_modifier(Modifier::BOLD),
            );
        }
    }
}

fn spaces_tile(buf: &mut Buffer, r: Rect, app: &App, theme: &Theme) {
    let pbg = Style::default().bg(theme.panel);
    heading(buf, r, theme, "SPACES", "");
    let list = app.home_spaces();
    if list.is_empty() {
        put(
            buf,
            r.x,
            r.y + 2,
            "no spaces yet",
            r.width,
            pbg.fg(theme.dim),
        );
        return;
    }
    let max = list.iter().map(|(_, n)| *n).max().unwrap_or(1).max(1);
    let name_w = (r.width / 3).clamp(6, 14);
    let bar_w = r.width.saturating_sub(name_w + 4);
    for (n, (path, count)) in list.iter().enumerate() {
        let y = r.y + 2 + n as u16;
        if y >= r.bottom() {
            break;
        }
        let color = app.space_color(path);
        put(buf, r.x, y, "●", 1, pbg.fg(color));
        put(
            buf,
            r.x + 2,
            y,
            &fit(&crate::core::spaces::display(path), name_w - 3),
            name_w - 3,
            pbg.fg(theme.status_fg),
        );
        let filled = (usize::from(bar_w) * count).div_ceil(max);
        let x = r.x + name_w;
        put(buf, x, y, &"━".repeat(filled), bar_w, pbg.fg(color));
        put(
            buf,
            x + filled as u16,
            y,
            &"━".repeat(usize::from(bar_w) - filled),
            bar_w,
            pbg.fg(theme.cursor),
        );
        let c = count.to_string();
        put(buf, r.right() - c.len() as u16, y, &c, 3, pbg.fg(theme.dim));
    }
}

fn notes(buf: &mut Buffer, r: Rect, app: &App, theme: &Theme) {
    let pbg = Style::default().bg(theme.panel);
    heading(buf, r, theme, "RECENT NOTES", "");
    let list = app.home_recent_notes(usize::from(r.height.saturating_sub(2)));
    if list.is_empty() {
        put(
            buf,
            r.x,
            r.y + 2,
            "no notes yet — o on a task",
            r.width,
            pbg.fg(theme.dim),
        );
        return;
    }
    for (n, note) in list.iter().enumerate() {
        let y = r.y + 2 + n as u16;
        if y >= r.bottom() {
            break;
        }
        let mut x = r.x;
        x += put(buf, x, y, "≡ ", 2, pbg.fg(theme.accent));
        let ww = note.when.chars().count() as u16 + 3;
        let room = r.right().saturating_sub(x + ww);
        x += put(
            buf,
            x,
            y,
            &fit(&note.title, room),
            room,
            pbg.fg(theme.status_fg),
        );
        put(
            buf,
            x,
            y,
            &format!(" · {}", note.when),
            ww,
            pbg.fg(theme.dim),
        );
    }
}

fn inbox(buf: &mut Buffer, r: Rect, app: &App, theme: &Theme) {
    let pbg = Style::default().bg(theme.panel);
    let items = app.inbox();
    heading(buf, r, theme, "INBOX", &items.len().to_string());
    if items.is_empty() {
        put(
            buf,
            r.x,
            r.y + 2,
            "all sorted ✓",
            r.width,
            pbg.fg(theme.dim),
        );
        return;
    }
    let room = r.height.saturating_sub(3);
    for (n, &i) in items.iter().take(usize::from(room)).enumerate() {
        let title = todo::body_only(&app.tasks()[i].raw);
        put(
            buf,
            r.x,
            r.y + 2 + n as u16,
            &fit(&format!("· {title}"), r.width),
            r.width,
            pbg.fg(theme.status_fg),
        );
    }
    put(
        buf,
        r.x,
        r.bottom() - 1,
        "i sort them in",
        r.width,
        pbg.fg(theme.dim),
    );
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use crate::app::App;
    use crate::config::Config;

    #[test]
    fn home_draws_at_any_size() {
        let raw = "Gym plan:2026-05-06 rec:1d\nx 2026-05-05 Gym plan:2026-05-05 rec:1d\n\
                   (A) Teoría +Uni plan:2026-05-06 at:09:00\nllamar al dentista\n";
        let path = std::env::temp_dir().join(format!("tasq-home-{}.txt", std::process::id()));
        std::fs::write(&path, raw).unwrap();
        let mut app = App::new(
            path.clone(),
            raw.into(),
            "2026-05-06".into(),
            Config::default(),
        );
        app.open_home();
        for (w, h) in [(40, 14), (70, 24), (100, 30), (160, 45), (20, 5)] {
            let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
            t.draw(|f| super::render(f, f.area(), &app)).unwrap();
            if w >= 100 {
                let text: String = t
                    .backend()
                    .buffer()
                    .content()
                    .iter()
                    .map(|c| c.symbol())
                    .collect();
                for tile in ["TODAY", "THIS WEEK", "ROUTINES", "SPACES", "INBOX"] {
                    assert!(text.contains(tile), "{tile} at {w}x{h}");
                }
                assert!(text.contains("1d"), "Gym's streak");
            }
        }
        let _ = std::fs::remove_file(&path);
    }
}
