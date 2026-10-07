//! Repeating tasks and events, one occurrence at a time: skipping a date,
//! taking one occurrence out of its series, splitting a series in two, and
//! rolling an event that's over on to its next date.
//!
//! All of it works on the task's todo.txt line, so a series stays one line:
//! `skip:` lists the dates left out, `until:` ends it.

use chrono::NaiveDate;

use crate::recurrence;
use crate::todo::{self, Task};

/// Marks an event (`event:1`): a class, a holiday, an exam. It isn't ticked
/// off and never goes overdue; once it's over it moves on to its next date,
/// or out of the way.
pub const EVENT_KEY: &str = "event";
/// The last day of an event that lasts several days (`end:YYYY-MM-DD`).
pub const END_KEY: &str = "end";
/// Dates a repeat leaves out (`skip:2026-05-07,2026-05-14`).
pub const SKIP_KEY: &str = "skip";

const FMT: &str = "%Y-%m-%d";

fn date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, FMT).ok()
}

fn fmt(d: NaiveDate) -> String {
    d.format(FMT).to_string()
}

/// `raw` with every `key:` token gone and, when `value` is given, one
/// `key:value` in place of the first (or at the end).
pub fn set_kv(raw: &str, key: &str, value: Option<&str>) -> String {
    let prefix = format!("{key}:");
    let mut placed = false;
    let mut out: Vec<String> = Vec::new();
    for tok in raw.split_whitespace() {
        if tok.starts_with(&prefix) && tok.len() > prefix.len() {
            if !placed && let Some(v) = value {
                out.push(format!("{prefix}{v}"));
            }
            placed = true;
            continue;
        }
        out.push(tok.to_string());
    }
    if !placed && let Some(v) = value {
        out.push(format!("{prefix}{v}"));
    }
    out.join(" ")
}

/// The dates `raw` skips.
pub fn skips(raw: &str) -> Vec<NaiveDate> {
    todo::find_kv(todo::body_after_priority(raw), SKIP_KEY)
        .map(|v| v.split(',').filter_map(date).collect())
        .unwrap_or_default()
}

/// `raw` skipping `d` too.
pub fn add_skip(raw: &str, d: NaiveDate) -> String {
    let mut all = skips(raw);
    if !all.contains(&d) {
        all.push(d);
    }
    all.sort_unstable();
    let list: Vec<String> = all.into_iter().map(fmt).collect();
    set_kv(raw, SKIP_KEY, Some(&list.join(",")))
}

/// Whether the task is an event.
pub fn is_event(raw: &str) -> bool {
    todo::find_kv(todo::body_after_priority(raw), EVENT_KEY).is_some_and(|v| v != "0")
}

/// The date the task's occurrences are counted from: planned, else due.
fn main_date(t: &Task) -> Option<NaiveDate> {
    t.planned
        .as_deref()
        .and_then(date)
        .or_else(|| t.due.as_deref().and_then(date))
}

/// How many days after it starts the task ends (0 for one day).
pub fn span_days(t: &Task) -> i64 {
    match (main_date(t), t.end.as_deref().and_then(date)) {
        (Some(s), Some(e)) if e > s => (e - s).num_days(),
        _ => 0,
    }
}

/// `raw` with every date on it (`plan:`, `due:`, `end:`) moved by `days`.
pub fn shift_dates(raw: &str, days: i64) -> String {
    if days == 0 {
        return raw.to_string();
    }
    raw.split_whitespace()
        .map(|tok| {
            for key in [todo::PLAN_KEY, "due", END_KEY] {
                if let Some(v) = tok.strip_prefix(key).and_then(|r| r.strip_prefix(':'))
                    && let Some(d) = date(v)
                    && let Some(n) = d.checked_add_signed(chrono::Duration::days(days))
                {
                    return format!("{key}:{}", fmt(n));
                }
            }
            tok.to_string()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The occurrence of `t` on `on`, as a task of its own that doesn't repeat:
/// its dates moved to that day, the repeat gone.
pub fn one_off(t: &Task, on: NaiveDate) -> String {
    let gap = main_date(t).map_or(0, |m| (on - m).num_days());
    let mut raw = shift_dates(&t.raw, gap);
    for key in ["rec", todo::UNTIL_KEY, todo::TIMES_KEY, SKIP_KEY] {
        raw = set_kv(&raw, key, None);
    }
    raw
}

/// The series of `t` cut at `on`: what's left of it (ending the day
/// before) and a new series starting that day.
pub fn split_at(t: &Task, on: NaiveDate) -> (String, String) {
    let before = on.pred_opt().unwrap_or(on);
    let old = set_kv(&t.raw, todo::UNTIL_KEY, Some(&fmt(before)));
    let old = set_kv(&old, todo::TIMES_KEY, None);
    let gap = main_date(t).map_or(0, |m| (on - m).num_days());
    let mut new = shift_dates(&t.raw, gap);
    new = set_kv(&new, todo::TIMES_KEY, None);
    let later: Vec<String> = skips(&t.raw)
        .into_iter()
        .filter(|d| *d > on)
        .map(fmt)
        .collect();
    new = set_kv(
        &new,
        SKIP_KEY,
        (!later.is_empty()).then(|| later.join(",")).as_deref(),
    );
    (old, new)
}

/// The dates `t` falls on after its own, in order, up to `limit` of them:
/// its repeats, without the skipped ones, within `until:` and `times:`.
pub fn repeats(t: &Task, limit: usize) -> Vec<NaiveDate> {
    match main_date(t) {
        Some(start) => repeats_from(t, start, limit),
        None => Vec::new(),
    }
}

/// Like [`repeats`], counting from `start` (for a repeat with no date yet,
/// which starts today).
pub fn repeats_from(t: &Task, start: NaiveDate, limit: usize) -> Vec<NaiveDate> {
    let Some(spec) = t.rec.as_deref().and_then(recurrence::parse_rec_spec) else {
        return Vec::new();
    };
    let until = t.until.as_deref().and_then(date);
    let mut left = t.times.as_deref().and_then(|v| v.parse::<u32>().ok());
    let skipped = skips(&t.raw);
    let mut out = Vec::new();
    let mut cur = start;
    while out.len() < limit {
        if let Some(n) = left.as_mut() {
            if *n <= 1 {
                break;
            }
            *n -= 1;
        }
        let Some(next) = recurrence::advance(cur, &spec) else {
            break;
        };
        if until.is_some_and(|u| next > u) {
            break;
        }
        if !skipped.contains(&next) {
            out.push(next);
        }
        cur = next;
    }
    out
}

/// The series of `t` moved on past its own date to the next one (one
/// fewer `times:`), or `None` when that was the last.
pub fn advance_one(t: &Task) -> Option<String> {
    let start = main_date(t)?;
    let next = *repeats(t, 1).first()?;
    let mut raw = shift_dates(&t.raw, (next - start).num_days());
    if let Some(n) = t.times.as_deref().and_then(|v| v.parse::<u32>().ok()) {
        raw = set_kv(
            &raw,
            todo::TIMES_KEY,
            Some(&n.saturating_sub(1).max(1).to_string()),
        );
    }
    let kept: Vec<String> = skips(&raw)
        .into_iter()
        .filter(|d| *d > next)
        .map(fmt)
        .collect();
    Some(set_kv(
        &raw,
        SKIP_KEY,
        (!kept.is_empty()).then(|| kept.join(",")).as_deref(),
    ))
}

/// An event that's over by `today`, moved on: to its next date that hasn't
/// ended yet (`Some(line)`), or `None` when there's none left.
pub fn roll_event(t: &Task, today: NaiveDate) -> Option<String> {
    let start = main_date(t)?;
    let span = span_days(t);
    let next = repeats(t, 2000)
        .into_iter()
        .find(|d| *d + chrono::Duration::days(span) >= today)?;
    let mut raw = shift_dates(&t.raw, (next - start).num_days());
    // `times:` counts what's left: one fewer per date passed.
    if let Some(n) = t.times.as_deref().and_then(|v| v.parse::<u32>().ok()) {
        let passed = repeats(t, 2000).iter().take_while(|d| **d < next).count() as u32 + 1;
        raw = set_kv(
            &raw,
            todo::TIMES_KEY,
            Some(&n.saturating_sub(passed).max(1).to_string()),
        );
    }
    let kept: Vec<String> = skips(&raw)
        .into_iter()
        .filter(|d| *d > next)
        .map(fmt)
        .collect();
    Some(set_kv(
        &raw,
        SKIP_KEY,
        (!kept.is_empty()).then(|| kept.join(",")).as_deref(),
    ))
}

/// Whether the event `t` is over by `today` (its last day before it).
pub fn event_over(t: &Task, today: NaiveDate) -> bool {
    main_date(t).is_some_and(|s| s + chrono::Duration::days(span_days(t)) < today)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::todo::parse_line;

    fn d(s: &str) -> NaiveDate {
        date(s).unwrap()
    }

    #[test]
    fn keys_are_set_replaced_and_removed() {
        assert_eq!(set_kv("a until:x b", "until", Some("y")), "a until:y b");
        assert_eq!(set_kv("a b", "until", Some("y")), "a b until:y");
        assert_eq!(set_kv("a skip:1 b skip:2", "skip", None), "a b");
        let r = add_skip("class rec:+1w", d("2026-05-14"));
        let r = add_skip(&r, d("2026-05-07"));
        assert_eq!(r, "class rec:+1w skip:2026-05-07,2026-05-14");
        assert_eq!(skips(&r), [d("2026-05-07"), d("2026-05-14")]);
    }

    #[test]
    fn one_occurrence_comes_out_of_its_series() {
        let t =
            parse_line("Class plan:2026-05-04 at:09:00 rec:+1w until:2026-06-30 skip:2026-05-11")
                .unwrap();
        assert_eq!(
            one_off(&t, d("2026-05-18")),
            "Class plan:2026-05-18 at:09:00"
        );
        let (old, new) = split_at(&t, d("2026-05-18"));
        assert_eq!(
            old,
            "Class plan:2026-05-04 at:09:00 rec:+1w until:2026-05-17 skip:2026-05-11"
        );
        assert_eq!(
            new,
            "Class plan:2026-05-18 at:09:00 rec:+1w until:2026-06-30"
        );
    }

    #[test]
    fn a_series_moves_on_one_date() {
        let t = parse_line("Class plan:2026-05-04 rec:+1w times:3 skip:2026-05-11").unwrap();
        assert_eq!(
            advance_one(&t).unwrap(),
            "Class plan:2026-05-18 rec:+1w times:2"
        );
        let last = parse_line("Class plan:2026-05-04 rec:+1w times:1").unwrap();
        assert_eq!(advance_one(&last), None);
    }

    #[test]
    fn repeats_leave_out_skipped_days_and_stop_at_the_end() {
        let t =
            parse_line("Class plan:2026-05-04 rec:+1w until:2026-05-25 skip:2026-05-11").unwrap();
        assert_eq!(repeats(&t, 10), [d("2026-05-18"), d("2026-05-25")]);
        let t = parse_line("Class plan:2026-05-04 rec:+1w times:3").unwrap();
        assert_eq!(repeats(&t, 10), [d("2026-05-11"), d("2026-05-18")]);
    }

    #[test]
    fn an_event_thats_over_moves_on_or_ends() {
        let today = d("2026-05-20");
        let class = parse_line("Algebra event:1 plan:2026-05-04 at:09:00 rec:+1w").unwrap();
        assert!(event_over(&class, today));
        assert_eq!(
            roll_event(&class, today).unwrap(),
            "Algebra event:1 plan:2026-05-25 at:09:00 rec:+1w"
        );
        // Several days: over only once its last day has gone.
        let trip = parse_line("Trip event:1 plan:2026-05-18 end:2026-05-22").unwrap();
        assert!(!event_over(&trip, today));
        assert_eq!(span_days(&trip), 4);
        let done = parse_line("Fair event:1 plan:2026-05-01").unwrap();
        assert!(event_over(&done, today));
        assert_eq!(roll_event(&done, today), None);
        let last = parse_line("Lab event:1 plan:2026-05-04 rec:+1w until:2026-05-15").unwrap();
        assert_eq!(roll_event(&last, today), None);
        let counted = parse_line("Lab event:1 plan:2026-05-04 rec:+1w times:5").unwrap();
        assert_eq!(
            roll_event(&counted, today).unwrap(),
            "Lab event:1 plan:2026-05-25 rec:+1w times:2"
        );
    }
}
