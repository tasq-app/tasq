//! The calendar the web app draws, worked out here so the browser only
//! paints: which task or event falls on which day and hour (the same
//! [`calendar::occurrences`] the terminal uses, repeats and deadlines and
//! late tasks included), side-by-side lanes, free gaps, each space's
//! colour, notes as HTML, and the other calendars' events in the browser's
//! time zone.
//!
//! Colours go out as tasq keeps them — a palette slot (`"3"`) or `#hex` —
//! and the page paints a slot with the theme you picked there, the way
//! the terminal does with its own ([`themes_json`]).

use std::collections::BTreeMap;

use chrono::{Datelike, NaiveDate};
use serde_json::{Value, json};

use super::external::ExtCalendar;
use super::tools::{Account, note_title};
use crate::core::calendar::{self, Occurrence};
use crate::core::db::title_of;
use crate::core::spaces;
use crate::extcal::cache::Cached;
use crate::extcal::event::{Response, When};
use crate::todo::{self, Task};

/// The widest range one answer covers: a month grid, six weeks.
pub const MAX_DAYS: i64 = 42;
/// The hours free gaps are looked for in, and the shortest worth showing,
/// as in the terminal's day view.
const FREE_FROM: u32 = 8 * 60;
const FREE_TO: u32 = 22 * 60;
const FREE_MIN: u32 = 30;

/// Markdown as HTML for the page: raw HTML in a note shows as text, and
/// links go only to the web or mail.
pub fn markdown_html(md: &str) -> String {
    use pulldown_cmark::{CowStr, Event, Options, Parser, Tag};
    let safe = |url: CowStr<'_>| -> CowStr<'static> {
        let u = url.trim().to_string();
        let lower = u.to_ascii_lowercase();
        if lower.starts_with("https://")
            || lower.starts_with("http://")
            || lower.starts_with("mailto:")
        {
            CowStr::from(u)
        } else {
            CowStr::from("#".to_string())
        }
    };
    let opts = Options::ENABLE_TASKLISTS | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES;
    let events = Parser::new_ext(md, opts).map(|e| match e {
        Event::Html(h) | Event::InlineHtml(h) => Event::Text(h),
        Event::Start(Tag::Link {
            link_type,
            dest_url,
            title,
            id,
        }) => Event::Start(Tag::Link {
            link_type,
            dest_url: safe(dest_url),
            title,
            id,
        }),
        Event::Start(Tag::Image {
            dest_url, title, ..
        }) => Event::Start(Tag::Link {
            link_type: pulldown_cmark::LinkType::Inline,
            dest_url: safe(dest_url),
            title,
            id: CowStr::from(""),
        }),
        Event::End(pulldown_cmark::TagEnd::Image) => Event::End(pulldown_cmark::TagEnd::Link),
        other => other,
    });
    let mut out = String::new();
    pulldown_cmark::html::push_html(&mut out, events);
    out
}

/// An invitation's description as plain text: Google sends some as HTML.
pub fn plain_text(s: &str) -> String {
    let mut out = String::new();
    let mut tag = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match (in_tag, c) {
            (false, '<') => {
                in_tag = true;
                tag.clear();
            }
            (true, '>') => {
                in_tag = false;
                let name = tag
                    .trim_start_matches('/')
                    .split(|c: char| c.is_whitespace() || c == '/')
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if matches!(name.as_str(), "br" | "p" | "div" | "li" | "tr") && !out.ends_with('\n')
                {
                    out.push('\n');
                }
                // A link's address, when its text doesn't show it.
                if let Some(href) = tag
                    .split("href=\"")
                    .nth(1)
                    .and_then(|h| h.split('"').next())
                    && (href.starts_with("http://") || href.starts_with("https://"))
                {
                    out.push_str(href);
                    out.push(' ');
                }
            }
            (true, c) => tag.push(c),
            (false, c) => out.push(c),
        }
    }
    let out = out
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&");
    let lines: Vec<&str> = out.lines().map(str::trim_end).collect();
    let mut text = lines.join("\n");
    while text.contains("\n\n\n") {
        text = text.replace("\n\n\n", "\n\n");
    }
    text.trim().to_string()
}

fn task_key(t: &Task) -> String {
    format!("t:{}", t.id)
}

fn ext_key(c: &Cached) -> String {
    format!("e:{}:{}:{}", c.account, c.calendar, c.event.id)
}

/// A task's colour: its space's, else none (the page uses the accent).
fn task_color(a: &Account, t: &Task) -> Option<String> {
    t.projects
        .first()
        .map(|p| spaces::color_of(p, a.store.known_spaces()).to_value())
}

fn task_detail(a: &Account, t: &Task) -> Value {
    let body = todo::body_after_priority(&t.clean_raw);
    let notes: Vec<Value> = a
        .task_notes(t)
        .unwrap_or_default()
        .iter()
        .map(|(name, body)| json!({ "title": note_title(name, body), "html": markdown_html(body) }))
        .collect();
    let (mut done, mut total) = (0, 0);
    for (_, b) in a.task_notes(t).unwrap_or_default() {
        for line in b.lines() {
            if let Some((d, _)) = crate::app::checklist::parse_item(line) {
                total += 1;
                done += usize::from(d);
            }
        }
    }
    json!({
        "kind": if t.event { "event" } else { "task" },
        "id": t.id,
        "title": title_of(&t.raw),
        "space": t.projects.first().map(|p| spaces::display(p)),
        "color": task_color(a, t),
        "tags": t.contexts,
        "priority": t.priority.map(|p| p.to_string()),
        "date": t.planned,
        "due": t.due,
        "time": todo::find_kv(body, "at"),
        "duration": t.duration,
        "repeat": t.rec,
        "repeat_until": t.until,
        "starred": t.starred,
        "done": t.done,
        "done_on": t.done_date,
        "notes": notes,
        "subtasks": if total > 0 { json!({ "done": done, "total": total }) } else { Value::Null },
    })
}

fn ext_detail(c: &Cached, o: &Occurrence) -> Value {
    let e = &c.event;
    let link = |u: &str| crate::extcal::as_account(u, &c.account);
    let mark = o.ext.as_deref();
    let (starts, ends, all_day) = match &e.when {
        When::Timed { start, end } => (start.to_rfc3339(), end.to_rfc3339(), false),
        When::AllDay { start, end } => (start.to_string(), end.to_string(), true),
    };
    json!({
        "kind": "external",
        "title": e.title,
        "calendar": mark.map(|m| m.calendar.clone()),
        "account": c.account,
        "color": mark.map(|m| m.color.to_value()),
        "starts": starts,
        "ends": ends,
        "all_day": all_day,
        "location": e.location,
        "description": e.description.as_deref().map(plain_text),
        "tentative": e.response != Response::Accepted,
        "response": match e.response {
            Response::Accepted => "accepted",
            Response::Tentative => "tentative",
            Response::Pending => "pending",
        },
        "video_call": e.meet_link.as_deref().map(link),
        "link": e.html_link.as_deref().map(link),
        "attachments": e.attachments.iter().map(|l| json!({ "title": l.title, "url": link(&l.url) })).collect::<Vec<_>>(),
        "attendees": e.attendees.iter().map(|p| json!({
            "name": p.name, "email": p.email, "response": p.response, "organizer": p.organizer,
        })).collect::<Vec<_>>(),
    })
}

/// One block or line on a day.
fn item(
    a: &Account,
    ext: &ExtCalendar,
    o: &Occurrence,
    details: &mut BTreeMap<String, Value>,
) -> Option<Value> {
    let (key, title, color, label, done, kind, priority) = match (&o.ext, ext.event_of(o)) {
        (Some(m), Some(c)) => {
            let key = ext_key(c);
            details
                .entry(key.clone())
                .or_insert_with(|| ext_detail(c, o));
            (
                key,
                m.title.clone(),
                Some(m.color.to_value()),
                Some(m.calendar.clone()),
                false,
                "external",
                None,
            )
        }
        (None, _) => {
            let t = a.tasks().get(o.abs)?;
            let key = task_key(t);
            details
                .entry(key.clone())
                .or_insert_with(|| task_detail(a, t));
            (
                key,
                title_of(&t.raw),
                task_color(a, t),
                t.projects.first().map(|p| spaces::display(p)),
                t.done,
                if t.event { "event" } else { "task" },
                t.priority.map(|p| p.to_string()),
            )
        }
        _ => return None,
    };
    Some(json!({
        "key": key,
        "kind": kind,
        "title": title,
        "color": color,
        "label": label,
        "priority": priority,
        "start": o.start,
        "end": o.end(),
        "minutes": o.minutes,
        "all_day": o.start.is_none(),
        "done": done,
        "late": o.late,
        "deadline": o.deadline,
        "projected": o.projected,
        "tentative": o.ext.as_ref().is_some_and(|m| m.tentative),
        "origin": o.origin.to_string(),
        "span": o.span,
        "day": o.day,
    }))
}

/// The calendar from `from` to `to` (inclusive) as the page draws it.
#[allow(clippy::too_many_arguments)]
pub(super) fn calendar_json(
    a: &Account,
    occs: &[Occurrence],
    ext: &ExtCalendar,
    from: NaiveDate,
    to: NaiveDate,
    today: NaiveDate,
    minute: u32,
    tz: &str,
) -> Value {
    let mut details = BTreeMap::new();
    let mut days = Vec::new();
    let mut d = from;
    while d <= to {
        let on: Vec<&Occurrence> = occs.iter().filter(|o| o.date == d).collect();
        let timed: Vec<&Occurrence> = on.iter().copied().filter(|o| o.start.is_some()).collect();
        let lanes = calendar::lanes(&timed);
        let mut all_day = Vec::new();
        for o in on.iter().filter(|o| o.start.is_none()) {
            if let Some(v) = item(a, ext, o, &mut details) {
                all_day.push(v);
            }
        }
        let mut blocks = Vec::new();
        for (o, (lane, n)) in timed.iter().zip(lanes) {
            if let Some(mut v) = item(a, ext, o, &mut details) {
                v["lane"] = json!(lane);
                v["lanes"] = json!(n);
                blocks.push(v);
            }
        }
        let busy: Vec<&Occurrence> = timed
            .iter()
            .copied()
            .filter(|o| o.ext.is_some() || a.tasks().get(o.abs).is_some_and(|t| !t.done))
            .collect();
        let free: Vec<Value> = calendar::free_slots(&busy, FREE_FROM, FREE_TO, FREE_MIN)
            .into_iter()
            .map(|(s, e)| json!([s, e]))
            .collect();
        days.push(json!({
            "date": d.to_string(),
            "weekday": d.weekday().num_days_from_monday(),
            "all_day": all_day,
            "timed": blocks,
            "free": free,
        }));
        let Some(next) = d.succ_opt() else { break };
        d = next;
    }
    json!({
        "today": today.to_string(),
        "minute": minute,
        "timezone": tz,
        "from": from.to_string(),
        "to": to.to_string(),
        "days": days,
        "details": details,
        "calendars_unavailable": ext.status,
        "themes": account_themes(&a.db_shared_themes()),
    })
}

/// The themes tasq comes with, as the page paints them: each colour as
/// `#rrggbb`, the palette with all its slots.
pub fn themes_json() -> Value {
    json!(
        crate::theme::BUILT_IN
            .iter()
            .map(|t| theme_json(t))
            .collect::<Vec<_>>()
    )
}

/// Your own themes, shared from your computers (see `Db::shared_themes`).
/// Each theme's text is read once per server run: a theme's name is kept
/// for good once read, so the same file isn't read again on every visit.
pub fn account_themes(rows: &[(String, String)]) -> Vec<Value> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static SEEN: OnceLock<Mutex<HashMap<String, Option<Value>>>> = OnceLock::new();
    let seen = SEEN.get_or_init(Default::default);
    let Ok(mut seen) = seen.lock() else {
        return Vec::new();
    };
    rows.iter()
        .filter_map(|(_, value)| {
            let text = serde_json::from_str::<Value>(value).ok()?["toml"]
                .as_str()?
                .to_string();
            if seen.len() > 1000 && !seen.contains_key(&text) {
                return None;
            }
            seen.entry(text.clone())
                .or_insert_with(|| {
                    crate::theme::parse_theme(&text)
                        .ok()
                        .map(|t| theme_json(&t))
                })
                .clone()
        })
        .collect()
}

fn theme_json(t: &crate::theme::Theme) -> Value {
    use ratatui::style::Color;
    let hex = |c: Color| {
        let (r, g, b) = crate::ui::mode_colors::rgb(c);
        format!("#{r:02x}{g:02x}{b:02x}")
    };
    // "Terminal" has no colours of its own: paint it as a dark terminal
    // would.
    let or = |c: Color, dark: &str| {
        if c == Color::Reset {
            dark.to_string()
        } else {
            hex(c)
        }
    };
    json!({
        "name": t.name,
        "bg": or(t.bg, "#14161b"),
        "panel": or(t.panel, "#1b1e24"),
        "border": hex(t.border),
        "fg": hex(t.fg),
        "dim": hex(t.dim),
        "accent": hex(t.accent),
        "selected": hex(t.selected),
        "today": hex(t.today),
        "overdue": hex(t.overdue),
        "due": hex(t.due),
        "ok": hex(t.ok),
        "done": hex(t.done),
        "mode_fg": hex(t.mode_fg),
        "priority": { "A": hex(t.pri_a), "B": hex(t.pri_b), "C": hex(t.pri_c), "D": hex(t.pri_d) },
        "palette": t.palette().iter().map(|c| hex(*c)).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn notes_render_without_raw_html_or_odd_links() {
        insta::assert_snapshot!(markdown_html(
            "# Plan\n\n- [x] tema 1\n- [ ] tema 2\n\n**ojo** <script>alert(1)</script> \
             [doc](https://example.com) [x](javascript:alert(1))"
        ));
    }

    #[test]
    fn invitations_read_as_text() {
        assert_eq!(
            plain_text(
                "Hola<br>Únete: <a href=\"https://meet.google.com/abc\">Meet</a><p>R&amp;D</p>"
            ),
            "Hola\nÚnete: https://meet.google.com/abc Meet\nR&D"
        );
    }

    #[test]
    fn every_theme_has_its_palette() {
        let v = themes_json();
        let list = v.as_array().unwrap();
        assert_eq!(list.len(), crate::theme::BUILT_IN.len());
        assert_eq!(list[0]["palette"].as_array().unwrap().len(), 16);
        assert_eq!(list[0]["palette"][0], "#8aadf4");
        assert!(
            list.iter()
                .all(|t| t["bg"].as_str().unwrap().starts_with('#'))
        );
    }
}
