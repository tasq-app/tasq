use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::search::subseq_match_ci;
use crate::theme::Theme;
use crate::todo::{Task, body_after_priority, is_star_token};

#[derive(Clone, Copy)]
pub struct RowOpts<'a> {
    pub idx_label: usize,
    pub cursor: bool,
    pub multi_mode: bool,
    pub multi_checked: bool,
    pub selected: bool,
    pub show_line_num: bool,
    pub match_term: Option<&'a str>,
    pub today: &'a str,
    /// `key:value` tokens whose key is in this list are omitted from the
    /// rendered body. Empty (the common case) means render everything,
    /// byte-for-byte as before.
    pub hidden_keys: &'a [String],
    /// The colour a space (a `+project` path) is painted in.
    pub space_color: &'a dyn Fn(&str) -> Color,
}

impl Default for RowOpts<'_> {
    fn default() -> Self {
        Self {
            idx_label: 0,
            cursor: false,
            multi_mode: false,
            multi_checked: false,
            selected: false,
            show_line_num: false,
            match_term: None,
            today: "",
            hidden_keys: &[],
            space_color: &|_| Color::Reset,
        }
    }
}

pub fn build_line<'a>(task: &'a Task, opts: RowOpts<'a>, theme: &Theme) -> Line<'a> {
    let mut spans: Vec<Span<'a>> = Vec::new();

    if opts.show_line_num {
        // fixes in line color selection
        let num_color = if opts.cursor { theme.fg } else { theme.dim };
        spans.push(Span::styled(
            format!("{:>3} ", opts.idx_label + 1),
            Style::default().fg(num_color),
        ));
    }
    if opts.multi_mode {
        let mark = if opts.multi_checked { "[x] " } else { "[ ] " };
        let c = if opts.multi_checked {
            theme.accent
        } else {
            theme.dim
        };
        spans.push(Span::styled(mark, Style::default().fg(c)));
    }

    // status glyph + priority box
    let glyph = if task.done {
        "✓ "
    } else if opts.cursor {
        "▸ "
    } else {
        "  "
    };
    // makes glyph visible on the cursor row
    let glyph_color = if task.done && !opts.cursor {
        theme.done
    } else {
        theme.accent
    };
    let mut glyph_style = Style::default().fg(glyph_color);
    if opts.cursor {
        glyph_style = glyph_style.add_modifier(Modifier::BOLD);
    }
    spans.push(Span::styled(glyph, glyph_style));

    // Priority as a flag in its colour (A red, B orange, C yellow…).
    match task.priority {
        Some(p) if !task.done => spans.push(Span::styled(
            "⚑ ",
            Style::default()
                .fg(theme.priority_color(p))
                .add_modifier(Modifier::BOLD),
        )),
        _ => spans.push(Span::raw("  ")),
    }
    // The `star:1` tag itself is hidden from the body below; this glyph
    // stands in for it.
    if task.starred && !task.done {
        spans.push(Span::styled(
            "★ ",
            Style::default()
                .fg(theme.pri_b)
                .add_modifier(Modifier::BOLD),
        ));
    }

    // body — walk &str slices instead of collecting Vec<char>. Spans borrow
    // straight from `task.raw`, so most rows allocate only for the format!()
    // calls above.
    let body = body_after_priority(&task.clean_raw);
    let body_match_positions: Option<Vec<usize>> =
        opts.match_term.and_then(|n| subseq_match_ci(body, n));
    let body_start = body.as_ptr() as usize;
    let mut rest = body;
    // Whether any visible body token has been emitted yet. Drives the
    // hidden-token branch's whitespace fix-up so a skipped token never
    // leaves a leading, trailing, or doubled space. When `hidden_keys`
    // is empty the branch is never entered and output is byte-identical
    // to before.
    let mut emitted_body_token = false;
    while !rest.is_empty() {
        let ws_end = rest
            .find(|c: char| !c.is_whitespace())
            .unwrap_or(rest.len());
        let pushed_ws = ws_end > 0;
        if pushed_ws {
            spans.push(Span::raw(&rest[..ws_end]));
            rest = &rest[ws_end..];
        }
        if rest.is_empty() {
            break;
        }
        let tok_end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let token = &rest[..tok_end];
        // Tags drawn as chips after the title, and the star (a glyph above),
        // leave the title itself plain words.
        if is_hidden_kv(token, opts.hidden_keys) || is_star_token(token) || is_chip_token(token) {
            // Drop the separator we just emitted for this token...
            if pushed_ws {
                spans.pop();
            }
            rest = &rest[tok_end..];
            // ...and if nothing visible precedes it, also swallow the
            // following whitespace run so the next token doesn't inherit
            // an orphan leading space.
            if !emitted_body_token {
                let n = rest
                    .find(|c: char| !c.is_whitespace())
                    .unwrap_or(rest.len());
                rest = &rest[n..];
            }
            continue;
        }
        let token_offset = token.as_ptr() as usize - body_start;
        push_token_spans(
            &mut spans,
            token,
            token_offset,
            body_match_positions.as_deref(),
            task,
            opts,
            theme,
        );
        emitted_body_token = true;
        rest = &rest[tok_end..];
    }
    push_chips(&mut spans, task, opts, theme);
    let line_style = if opts.cursor {
        // handles the background highligh and serves as a fallback.
        Style::default().bg(theme.cursor).fg(theme.fg)
    } else if opts.selected {
        Style::default().bg(theme.selected).fg(theme.fg)
    } else {
        Style::default()
    };
    Line::from(spans).style(line_style)
}

fn push_token_spans<'a>(
    spans: &mut Vec<Span<'a>>,
    token: &'a str,
    token_offset_in_body: usize,
    body_match_positions: Option<&[usize]>,
    task: &Task,
    opts: RowOpts<'a>,
    theme: &Theme,
) {
    if let Some(c) = sigil_token_color(token, task, theme) {
        spans.push(Span::styled(token, Style::default().fg(c)));
        return;
    }
    if let Some(rest) = token.strip_prefix("plan:") {
        spans.push(Span::styled(
            token,
            planned_token_style(task.done, rest, opts.today, theme),
        ));
        return;
    }
    if let Some(rest) = token.strip_prefix("due:") {
        spans.push(Span::styled(
            token,
            due_token_style(task.done, rest, opts.today, theme),
        ));
        return;
    }
    // URLs are picked off before the generic key:value branch — `http:` would
    // otherwise classify as a lowercase key and steal the underline + accent
    // styling that doubles as the OSC 8 hyperlink marker (see `ui::hyperlinks`).
    if is_url_token(token) {
        spans.push(Span::styled(token, url_token_style(task.done, theme)));
        return;
    }
    // generic key:value (lowercase key)
    if let Some((k, _v)) = token.split_once(':')
        && !k.is_empty()
        && k.chars()
            .next()
            .expect("invariant: !k.is_empty() guarded above")
            .is_ascii_lowercase()
        && k.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        spans.push(Span::styled(token, Style::default().fg(theme.dim)));
        return;
    }

    // plain word — highlight each matched subsequence char inside this token.
    // makes the task body text visible on the cursor row
    let base_color = if task.done && !opts.cursor {
        theme.done
    } else {
        theme.fg
    };
    let base_style = apply_dim(Style::default().fg(base_color), task.done);
    let hl_style = Style::default()
        .fg(theme.bg)
        .bg(theme.matched)
        .add_modifier(Modifier::BOLD);

    let token_end = token_offset_in_body + token.len();
    let mut local_positions = body_match_positions
        .into_iter()
        .flatten()
        .copied()
        .filter(|&p| p >= token_offset_in_body && p < token_end)
        .map(|p| p - token_offset_in_body)
        .peekable();

    if local_positions.peek().is_none() {
        spans.push(Span::styled(token, base_style));
        return;
    }

    let mut cursor = 0usize;
    for p in local_positions {
        if cursor < p {
            spans.push(Span::styled(&token[cursor..p], base_style));
        }
        let ch = token[p..]
            .chars()
            .next()
            .expect("match offset lands on a char boundary");
        let next = p + ch.len_utf8();
        spans.push(Span::styled(&token[p..next], hl_style));
        cursor = next;
    }
    if cursor < token.len() {
        spans.push(Span::styled(&token[cursor..], base_style));
    }
}

/// Tag keys shown as chips after the title instead of inside it.
const CHIP_KEYS: &[&str] = &[
    "due", "plan", "at", "dur", "remind", "rec", "until", "times", "t", "notes",
];

/// Whether `token` is drawn as a chip: a `+space`, an `@tag`, or one of
/// [`CHIP_KEYS`] with a value.
fn is_chip_token(token: &str) -> bool {
    if (token.starts_with('+') || token.starts_with('@')) && token.len() > 1 {
        return true;
    }
    token
        .split_once(':')
        .is_some_and(|(k, v)| !v.is_empty() && CHIP_KEYS.contains(&k))
}

/// Mix `color` into `bg` at `amount` (0–1), for a chip's tinted ground.
/// `None` when either isn't an RGB colour (a terminal-palette theme).
fn tint(color: Color, bg: Color, amount: f32) -> Option<Color> {
    match (color, bg) {
        (Color::Rgb(r, g, b), Color::Rgb(br, bgc, bb)) => {
            let mix = |c: u8, base: u8| {
                (f32::from(base) + (f32::from(c) - f32::from(base)) * amount).round() as u8
            };
            Some(Color::Rgb(mix(r, br), mix(g, bgc), mix(b, bb)))
        }
        _ => None,
    }
}

/// One chip: ` text ` in `color` on a tint of it. A done task's chips are
/// plain and dim.
fn push_chip<'a>(spans: &mut Vec<Span<'a>>, text: String, color: Color, done: bool, theme: &Theme) {
    spans.push(Span::raw(" "));
    if done {
        spans.push(Span::styled(text, Style::default().fg(theme.done)));
        return;
    }
    let mut style = Style::default().fg(color);
    if let Some(bg) = tint(color, theme.bg, 0.2) {
        style = style.bg(bg);
    }
    spans.push(Span::styled(format!(" {text} "), style));
}

/// A date as a chip reads it: `today`, `tomorrow`, `yesterday`, else
/// `fri 9 oct` (with the year when it isn't this one).
pub(crate) fn chip_date(date: &str, today: &str) -> String {
    use chrono::Datelike;
    let parse = |s: &str| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok();
    let (Some(d), Some(t)) = (parse(date), parse(today)) else {
        return date.to_string();
    };
    match (d - t).num_days() {
        0 => "today".into(),
        1 => "tomorrow".into(),
        -1 => "yesterday".into(),
        _ if d.year() == t.year() => d.format("%a %-d %b").to_string().to_lowercase(),
        _ => d.format("%-d %b %Y").to_string().to_lowercase(),
    }
}

/// The chips after the title: when (date · time · duration), the
/// deadline, the repeat, the space, the tags and a note marker.
fn push_chips<'a>(spans: &mut Vec<Span<'a>>, task: &Task, opts: RowOpts<'a>, theme: &Theme) {
    let shown = |key: &str| !opts.hidden_keys.iter().any(|h| h.eq_ignore_ascii_case(key));
    let done = task.done;
    let time = shown("at")
        .then(|| crate::todo::find_kv(&task.clean_raw, "at"))
        .flatten();

    // When: the planned day, else (with no deadline) just the time.
    let mut when: Vec<String> = Vec::new();
    if let Some(p) = task.planned.as_deref().filter(|_| shown("plan")) {
        when.push(chip_date(p, opts.today));
    }
    if let Some(t) = time {
        when.push(t);
    }
    if let Some(d) = task.duration.as_deref().filter(|_| shown("dur")) {
        when.push(d.to_string());
    }
    if !when.is_empty() {
        let color = match task.planned.as_deref() {
            Some(p) if p <= opts.today => theme.today,
            _ => theme.accent,
        };
        push_chip(spans, when.join(" · "), color, done, theme);
    }
    if let Some(d) = task.due.as_deref().filter(|_| shown("due")) {
        let date = chip_date(d, opts.today);
        let (text, color) = match due_status(d, opts.today) {
            DueStatus::Overdue => (format!("◷ overdue · {date}"), theme.overdue),
            DueStatus::Today => ("◷ due today".to_string(), theme.overdue),
            DueStatus::Soon => (format!("◷ by {date}"), theme.due),
            DueStatus::Later | DueStatus::None => (format!("◷ by {date}"), theme.dim),
        };
        push_chip(spans, text, color, done, theme);
    }
    if let Some(t) = task.threshold.as_deref().filter(|_| shown("t")) {
        let from = crate::threshold::parse_threshold(t)
            .and_then(|spec| {
                crate::threshold::resolve(&spec, task.due.as_deref(), task.created_date.as_deref())
            })
            .map_or_else(
                || t.to_string(),
                |d| chip_date(&d.format("%Y-%m-%d").to_string(), opts.today),
            );
        push_chip(spans, format!("shows from {from}"), theme.dim, done, theme);
    }
    if let Some(r) = task.rec.as_deref().filter(|_| shown("rec")) {
        let mut text = format!("↻ {}", crate::app::describe_rec(r));
        if let Some(u) = task.until.as_deref() {
            text.push_str(&format!(" until {}", chip_date(u, opts.today)));
        }
        if let Some(n) = task.times.as_deref() {
            text.push_str(&format!(" · {n} left"));
        }
        push_chip(spans, text, theme.pri_other, done, theme);
    }
    for p in &task.projects {
        let color = (opts.space_color)(p);
        push_chip(
            spans,
            format!("● {}", crate::core::spaces::display(p)),
            color,
            done,
            theme,
        );
    }
    for c in &task.contexts {
        spans.push(Span::styled(
            format!(" @{c}"),
            Style::default().fg(if done { theme.done } else { theme.context }),
        ));
    }
    if crate::todo::find_kv(&task.clean_raw, "notes").is_some() && shown("notes") {
        spans.push(Span::styled(" ≡", Style::default().fg(theme.dim)));
    }
}

/// True when `token` is a `key:value` pair whose key (case-insensitively)
/// appears in `hidden_keys`. Empty list short-circuits so the common path
/// stays allocation- and comparison-free.
fn is_hidden_kv(token: &str, hidden_keys: &[String]) -> bool {
    if hidden_keys.is_empty() {
        return false;
    }
    match token.split_once(':') {
        Some((k, v)) if !k.is_empty() && !v.is_empty() => {
            hidden_keys.iter().any(|h| h.eq_ignore_ascii_case(k))
        }
        _ => false,
    }
}

pub(crate) fn is_url_token(token: &str) -> bool {
    token.starts_with("http://") || token.starts_with("https://")
}

pub(crate) fn url_token_style(task_done: bool, theme: &Theme) -> Style {
    let color = if task_done { theme.done } else { theme.accent };
    let mut style = Style::default()
        .fg(color)
        .add_modifier(Modifier::UNDERLINED);
    if task_done {
        style = style.add_modifier(Modifier::DIM);
    }
    style
}

fn sigil_token_color(token: &str, task: &Task, theme: &Theme) -> Option<Color> {
    if !token.starts_with('+') && !token.starts_with('@') {
        return None;
    }
    if task.done {
        return Some(theme.done);
    }
    if token.starts_with('+') {
        Some(theme.project)
    } else {
        Some(theme.context)
    }
}

fn apply_dim(style: Style, dim: bool) -> Style {
    if dim {
        style.add_modifier(Modifier::DIM)
    } else {
        style
    }
}

#[derive(Copy, Clone)]
enum DueStatus {
    Overdue,
    Today,
    Soon,
    Later,
    None,
}

fn due_status(due: &str, today: &str) -> DueStatus {
    if due.len() != 10 || today.len() != 10 {
        return DueStatus::None;
    }
    match due.cmp(today) {
        std::cmp::Ordering::Less => DueStatus::Overdue,
        std::cmp::Ordering::Equal => DueStatus::Today,
        std::cmp::Ordering::Greater => {
            // within 2 days?
            let d = day_diff(due, today).unwrap_or(99);
            if d <= 2 {
                DueStatus::Soon
            } else {
                DueStatus::Later
            }
        }
    }
}

fn day_diff(a: &str, b: &str) -> Option<i64> {
    let to_ymd = |s: &str| -> Option<(i32, u32, u32)> {
        let y = s.get(0..4)?.parse().ok()?;
        let mo = s.get(5..7)?.parse().ok()?;
        let d = s.get(8..10)?.parse().ok()?;
        Some((y, mo, d))
    };
    let (ay, am, ad) = to_ymd(a)?;
    let (by, bm, bd) = to_ymd(b)?;
    let da = chrono::NaiveDate::from_ymd_opt(ay, am, ad)?;
    let db = chrono::NaiveDate::from_ymd_opt(by, bm, bd)?;
    Some(da.signed_duration_since(db).num_days())
}

pub(crate) fn due_token_style(task_done: bool, due: &str, today: &str, theme: &Theme) -> Style {
    let status = due_status(due, today);
    let c = if task_done {
        theme.done
    } else {
        match status {
            DueStatus::Overdue => theme.overdue,
            DueStatus::Today => theme.today,
            DueStatus::Soon => theme.due,
            DueStatus::Later | DueStatus::None => theme.dim,
        }
    };
    let mut style = Style::default().fg(c);
    if matches!(status, DueStatus::Overdue | DueStatus::Today) {
        style = style.add_modifier(Modifier::BOLD);
    }
    style
}

/// A planned date is never "overdue" — only a deadline is. Today's stands
/// out; past ones keep the date colour, so a late plan is still visible.
pub(crate) fn planned_token_style(
    task_done: bool,
    planned: &str,
    today: &str,
    theme: &Theme,
) -> Style {
    if task_done {
        return Style::default().fg(theme.dim);
    }
    let style = Style::default().fg(theme.due);
    if planned <= today {
        style.add_modifier(Modifier::BOLD)
    } else {
        style
    }
}

pub fn due_label(due: &str, today: &str) -> String {
    if let Some(d) = day_diff(due, today) {
        if d < 0 {
            return if d == -1 {
                "overdue 1d".into()
            } else {
                format!("overdue {}d", -d)
            };
        }
        if d == 0 {
            return "today".into();
        }
        if d == 1 {
            return "tomorrow".into();
        }
        if d < 7 {
            return format!("in {}d", d);
        }
    }
    due.to_string()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::theme::MUTED;
    use crate::todo::parse_line;

    #[test]
    fn starred_task_shows_a_star_glyph_instead_of_its_tag() {
        let task = parse_line("(A) Book hotel star:1").unwrap();
        let line = build_line(&task, RowOpts::default(), &MUTED);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("★ Book hotel"), "{text}");
        assert!(!text.contains("star:1"), "{text}");
    }

    #[test]
    fn build_line_does_not_panic_on_unicode_with_match_term() {
        // Regression: the previous lowercase-find-then-byte-slice approach
        // panics here. "İ".to_lowercase() = "i" + combining dot (3 bytes vs
        // 2 in the original), so the match offset derived from the
        // lowercased string lands off a char boundary in the source token.
        let task = parse_line("İa").unwrap();
        let opts = RowOpts {
            idx_label: 0,
            cursor: false,
            multi_mode: false,
            multi_checked: false,
            selected: false,
            show_line_num: false,
            match_term: Some("a"),
            today: "2026-05-06",
            hidden_keys: &[],
            space_color: &|_| Color::Reset,
        };
        // Build must not panic; we don't assert on the rendered spans.
        let _ = build_line(&task, opts, &MUTED);
    }

    #[test]
    fn build_line_highlights_subsequence_chars() {
        // "cade" is a subsequence of "Call dentist": C(0), a(1), D(5), e(6).
        // The renderer should emit highlighted single-char spans for those
        // positions, with the unmatched chars rendered in the base style.
        let task = parse_line("Call dentist").unwrap();
        let opts = RowOpts {
            idx_label: 0,
            cursor: false,
            multi_mode: false,
            multi_checked: false,
            selected: false,
            show_line_num: false,
            match_term: Some("cade"),
            today: "2026-05-06",
            hidden_keys: &[],
            space_color: &|_| Color::Reset,
        };
        let line = build_line(&task, opts, &MUTED);
        let highlight_bg = MUTED.matched;
        let highlighted: String = line
            .spans
            .iter()
            .filter(|s| s.style.bg == Some(highlight_bg))
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(highlighted, "Cade");
    }

    /// Render `raw` and return the body text (all span content joined,
    /// fixed glyph/priority prefix trimmed). Tasks here carry no priority
    /// and aren't done, so the prefix is pure leading whitespace and the
    /// "no leading body space" invariant makes `trim_start` exact.
    fn body_text(raw: &str, hidden: &[String]) -> String {
        let task = parse_line(raw).unwrap();
        let opts = RowOpts {
            idx_label: 0,
            cursor: false,
            multi_mode: false,
            multi_checked: false,
            selected: false,
            show_line_num: false,
            match_term: None,
            today: "2026-05-06",
            hidden_keys: hidden,
            space_color: &|_| Color::Reset,
        };
        let line = build_line(&task, opts, &MUTED);
        line.spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect::<String>()
            .trim_start()
            .to_string()
    }

    #[test]
    fn hidden_key_in_middle_omitted() {
        let h = vec!["uid".to_string()];
        assert_eq!(
            body_text("Call dentist uid:abc-123 @phone +health", &h),
            // Spaces and tags come after the title, as chips.
            "Call dentist  ● health  @phone",
        );
    }

    #[test]
    fn hidden_key_at_start_omitted() {
        let h = vec!["uid".to_string()];
        assert_eq!(body_text("uid:abc-123 Call dentist", &h), "Call dentist");
    }

    #[test]
    fn hidden_key_at_end_omitted() {
        let h = vec!["uid".to_string()];
        assert_eq!(body_text("Call dentist uid:abc-123", &h), "Call dentist");
    }

    #[test]
    fn adjacent_hidden_keys_collapse_to_single_space() {
        let h = vec!["uid".to_string(), "sync".to_string()];
        assert_eq!(body_text("Call uid:a sync:b dentist", &h), "Call dentist",);
    }

    #[test]
    fn hidden_key_match_is_case_insensitive() {
        let h = vec!["uid".to_string()];
        assert_eq!(body_text("Call UID:abc done", &h), "Call done");
    }

    #[test]
    fn empty_hidden_list_renders_everything_unchanged() {
        assert_eq!(
            body_text("Call dentist uid:abc @phone +health", &[]),
            "Call dentist uid:abc  ● health  @phone",
        );
    }

    #[test]
    fn url_token_is_underlined_and_accented() {
        // The underline modifier is the sentinel `ui::hyperlinks::linkify`
        // looks for. If this test fails, OSC 8 hyperlinks silently stop being
        // emitted — break it intentionally only when changing the marker.
        let task = parse_line("See https://example.com for details").unwrap();
        let opts = RowOpts {
            idx_label: 0,
            cursor: false,
            multi_mode: false,
            multi_checked: false,
            selected: false,
            show_line_num: false,
            match_term: None,
            today: "2026-05-06",
            hidden_keys: &[],
            space_color: &|_| Color::Reset,
        };
        let line = build_line(&task, opts, &MUTED);
        let url_span = line
            .spans
            .iter()
            .find(|s| s.content.as_ref() == "https://example.com")
            .expect("URL token rendered as its own span");
        assert!(
            url_span.style.add_modifier.contains(Modifier::UNDERLINED),
            "URL span must carry Modifier::UNDERLINED; got {:?}",
            url_span.style,
        );
        assert_eq!(url_span.style.fg, Some(MUTED.accent));
    }

    #[test]
    fn url_token_not_classified_as_key_value() {
        // Without the URL branch in front of the generic key:value branch,
        // `http:` would split into ("http", "//example.com") and render with
        // the dim key-value style instead of the accent + underline.
        let task = parse_line("note http://example.com").unwrap();
        let opts = RowOpts {
            idx_label: 0,
            cursor: false,
            multi_mode: false,
            multi_checked: false,
            selected: false,
            show_line_num: false,
            match_term: None,
            today: "2026-05-06",
            hidden_keys: &[],
            space_color: &|_| Color::Reset,
        };
        let line = build_line(&task, opts, &MUTED);
        let url_span = line
            .spans
            .iter()
            .find(|s| s.content.as_ref() == "http://example.com")
            .expect("URL span");
        assert_ne!(
            url_span.style.fg,
            Some(MUTED.dim),
            "URL must not pick up the dim key-value color",
        );
    }

    #[test]
    fn non_listed_key_not_hidden() {
        let h = vec!["uid".to_string()];
        // The deadline stays (as a chip); only configured keys are dropped.
        assert_eq!(
            body_text("Pay rent due:2026-05-15 uid:x", &h),
            "Pay rent  ◷ by fri 15 may ",
        );
    }

    #[test]
    fn a_task_reads_as_its_title_and_chips() {
        let raw = "(A) Trabajo TIS +Uni/Exams @laptop plan:2026-05-06 at:16:00 dur:2h due:2026-05-08 rec:+1w until:2026-06-30 star:1";
        let task = parse_line(raw).unwrap();
        let opts = RowOpts {
            today: "2026-05-06",
            ..RowOpts::default()
        };
        let text: String = build_line(&task, opts, &MUTED)
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(
            text.trim_end(),
            "  ⚑ ★ Trabajo TIS  today · 16:00 · 2h   ◷ by fri 8 may   ↻ every week until tue 30 jun   ● Uni › Exams  @laptop"
        );
        assert_eq!(chip_date("2026-05-07", "2026-05-06"), "tomorrow");
        assert_eq!(chip_date("2027-01-02", "2026-05-06"), "2 jan 2027");
    }
}
