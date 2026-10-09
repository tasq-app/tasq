//! A task as a Google Calendar event: the JSON body the Calendar API takes.
//!
//! Only tasks with a day go: open, planned or due. One event per task; a
//! repeat is one recurring event (`RRULE`), its skipped days `EXDATE`s.
//! With a time it's a block (30 minutes without a duration), without one
//! an all-day event; one lasting several days spans them. A deadline with
//! no plan shows as an all-day "◷ title" on its day.

use chrono::NaiveDate;
use serde_json::{Value, json};

use crate::core::series;
use crate::recurrence::{self, RecUnit};
use crate::todo::{self, Task};

/// How long a timed event is without a `dur:`.
const DEFAULT_MINUTES: u32 = 30;

/// Google keeps at most five reminders an event.
const MAX_REMINDERS: usize = 5;

fn date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

/// `HH:MM` → minutes after midnight.
fn clock(s: &str) -> Option<u32> {
    crate::core::calendar::parse_time(s)
}

/// The day the task is on, and whether that's only its deadline.
fn day_of(t: &Task) -> Option<(NaiveDate, bool)> {
    if let Some(p) = t.planned.as_deref().and_then(date) {
        return Some((p, false));
    }
    t.due.as_deref().and_then(date).map(|d| (d, true))
}

/// The rule of a repeat, in RFC 5545 (`FREQ=WEEKLY;INTERVAL=2;BYDAY=MO`),
/// without its end.
fn rrule(rec: &str) -> Option<String> {
    let spec = recurrence::parse_rec_spec(rec)?;
    let n = spec.n.max(1);
    let mut rule = match spec.unit {
        RecUnit::Day => format!("FREQ=DAILY;INTERVAL={n}"),
        RecUnit::Week => format!("FREQ=WEEKLY;INTERVAL={n}"),
        RecUnit::Month => format!("FREQ=MONTHLY;INTERVAL={n}"),
        RecUnit::Year => format!("FREQ=YEARLY;INTERVAL={n}"),
        // Every working day; "every 3 working days" has no exact rule, so
        // it reads as every working day.
        RecUnit::BusinessDay => "FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR".to_string(),
    };
    if spec.unit == RecUnit::Week && spec.days != 0 {
        const CODES: [&str; 7] = ["MO", "TU", "WE", "TH", "FR", "SA", "SU"];
        let days: Vec<&str> = (0..7)
            .filter(|i| spec.days & (1 << i) != 0)
            .map(|i| CODES[i])
            .collect();
        rule.push_str(&format!(";BYDAY={}", days.join(",")));
    }
    Some(rule)
}

/// Everything the event needs that isn't on the task.
pub struct Context<'a> {
    /// The IANA time zone timed events are in, e.g. `Europe/Madrid`.
    pub time_zone: &'a str,
    /// The Google `colorId` of the task's space, if it has one.
    pub color_id: Option<&'a str>,
}

/// The event body for task `t`, or `None` when it has no place in the
/// calendar (done, or no day).
pub fn body(t: &Task, cx: &Context) -> Option<Value> {
    if t.done {
        return None;
    }
    let (day, deadline_only) = day_of(t)?;
    let title = todo::body_only(&t.raw);
    let summary = if deadline_only {
        format!("◷ {title}")
    } else {
        title
    };
    let at = todo::find_kv(todo::body_after_priority(&t.clean_raw), "at")
        .and_then(|v| clock(&v))
        .filter(|_| !deadline_only);
    let span = series::span_days(t).max(0);
    let fmt_day = |d: NaiveDate| d.format("%Y-%m-%d").to_string();

    let (start, end) = match at.filter(|_| span == 0) {
        Some(m) => {
            let minutes = t
                .duration
                .as_deref()
                .and_then(crate::duration::parse_minutes)
                .filter(|m| *m > 0)
                .unwrap_or(DEFAULT_MINUTES);
            let starts = day.and_hms_opt(m / 60, m % 60, 0)?;
            let ends = starts + chrono::Duration::minutes(i64::from(minutes));
            let dt = |d: chrono::NaiveDateTime| {
                json!({
                    "dateTime": d.format("%Y-%m-%dT%H:%M:%S").to_string(),
                    "timeZone": cx.time_zone,
                })
            };
            (dt(starts), dt(ends))
        }
        None => {
            // All day; the end is the day after the last (Google's end is
            // exclusive).
            let last = day + chrono::Duration::days(span);
            (
                json!({ "date": fmt_day(day) }),
                json!({ "date": fmt_day(last.succ_opt()?) }),
            )
        }
    };

    let mut event = json!({
        "summary": summary,
        "start": start,
        "end": end,
        "extendedProperties": { "private": { "tasq": t.id } },
    });
    if !t.projects.is_empty() {
        event["description"] = json!(
            t.projects
                .iter()
                .map(|p| crate::core::spaces::display(p))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    if let Some(c) = cx.color_id {
        event["colorId"] = json!(c);
    }

    // Repeats: the rule, its end, and the days it leaves out.
    if !deadline_only && let Some(mut rule) = t.rec.as_deref().and_then(rrule) {
        if let Some(n) = t.times.as_deref().and_then(|v| v.parse::<u32>().ok()) {
            rule.push_str(&format!(";COUNT={n}"));
        } else if let Some(u) = t.until.as_deref().and_then(date) {
            rule.push_str(&match at {
                // A timed repeat ends in UTC: the end of its last day, so
                // that day's occurrence is in whatever the zone.
                Some(_) => format!(";UNTIL={}T235959Z", u.format("%Y%m%d")),
                None => format!(";UNTIL={}", u.format("%Y%m%d")),
            });
        }
        let mut recurrence = vec![format!("RRULE:{rule}")];
        let skips = series::skips(&t.raw);
        if !skips.is_empty() {
            let list: Vec<String> = skips
                .iter()
                .map(|d| match at {
                    Some(m) => format!("{}T{:02}{:02}00", d.format("%Y%m%d"), m / 60, m % 60),
                    None => d.format("%Y%m%d").to_string(),
                })
                .collect();
            recurrence.push(match at {
                Some(_) => format!("EXDATE;TZID={}:{}", cx.time_zone, list.join(",")),
                None => format!("EXDATE;VALUE=DATE:{}", list.join(",")),
            });
        }
        event["recurrence"] = json!(recurrence);
    }

    // Reminders before its time (a timed event only).
    if at.is_some()
        && let Some(r) = t.reminders.as_deref()
    {
        let overrides: Vec<Value> = r
            .split(',')
            .filter_map(crate::duration::parse_minutes)
            .take(MAX_REMINDERS)
            .map(|m| json!({ "method": "popup", "minutes": m }))
            .collect();
        if !overrides.is_empty() {
            event["reminders"] = json!({ "useDefault": false, "overrides": overrides });
        }
    }
    Some(event)
}

/// The IANA time zone of this machine: `$TZ`, else where `/etc/localtime`
/// points (macOS and Linux both link it into a zoneinfo directory), else
/// UTC.
pub fn local_time_zone() -> String {
    if let Ok(tz) = std::env::var("TZ") {
        let tz = tz.trim_start_matches(':');
        if tz.contains('/') {
            return tz.to_string();
        }
    }
    std::fs::read_link("/etc/localtime")
        .ok()
        .and_then(|p| {
            let s = p.to_string_lossy().into_owned();
            s.split_once("zoneinfo/").map(|(_, z)| z.to_string())
        })
        .unwrap_or_else(|| "UTC".to_string())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::todo::parse_line;

    fn ev(line: &str) -> Value {
        let mut t = parse_line(line).unwrap();
        t.id = "T1".into();
        body(
            &t,
            &Context {
                time_zone: "Europe/Madrid",
                color_id: Some("9"),
            },
        )
        .unwrap()
    }

    #[test]
    fn a_timed_task_is_a_block_in_its_zone() {
        let e = ev("(A) Teoría AII +Uni/AII plan:2026-10-12 at:09:00 dur:1h30m remind:15m");
        assert_eq!(e["summary"], "Teoría AII");
        assert_eq!(e["start"]["dateTime"], "2026-10-12T09:00:00");
        assert_eq!(e["end"]["dateTime"], "2026-10-12T10:30:00");
        assert_eq!(e["start"]["timeZone"], "Europe/Madrid");
        assert_eq!(e["colorId"], "9");
        assert_eq!(e["description"], "Uni › AII");
        assert_eq!(e["reminders"]["overrides"][0]["minutes"], 15);
        assert_eq!(e["extendedProperties"]["private"]["tasq"], "T1");
    }

    #[test]
    fn without_a_time_its_all_day_and_several_days_span() {
        let e = ev("Pay rent plan:2026-10-12");
        assert_eq!(e["start"]["date"], "2026-10-12");
        assert_eq!(e["end"]["date"], "2026-10-13");
        let e = ev("Trip event:1 plan:2026-11-16 end:2026-11-19");
        assert_eq!(e["start"]["date"], "2026-11-16");
        assert_eq!(e["end"]["date"], "2026-11-20");
        let e = ev("Essay due:2026-10-20");
        assert_eq!(e["summary"], "◷ Essay");
        assert_eq!(e["start"]["date"], "2026-10-20");
    }

    #[test]
    fn a_repeat_is_one_recurring_event() {
        let e =
            ev("Class plan:2026-10-12 at:09:00 rec:+1w:mon,wed until:2026-12-20 skip:2026-10-14");
        assert_eq!(
            e["recurrence"],
            json!([
                "RRULE:FREQ=WEEKLY;INTERVAL=1;BYDAY=MO,WE;UNTIL=20261220T235959Z",
                "EXDATE;TZID=Europe/Madrid:20261014T090000"
            ])
        );
        let e = ev("Rent plan:2026-11-01 rec:+1m times:6");
        assert_eq!(
            e["recurrence"],
            json!(["RRULE:FREQ=MONTHLY;INTERVAL=1;COUNT=6"])
        );
        let e = ev("Gym plan:2026-10-12 rec:2w skip:2026-10-26");
        assert_eq!(
            e["recurrence"],
            json!(["RRULE:FREQ=WEEKLY;INTERVAL=2", "EXDATE;VALUE=DATE:20261026"])
        );
    }

    #[test]
    fn done_or_undated_tasks_stay_out() {
        let mut t = parse_line("x 2026-10-08 Done plan:2026-10-08").unwrap();
        t.id = "T".into();
        let cx = Context {
            time_zone: "UTC",
            color_id: None,
        };
        assert!(body(&t, &cx).is_none());
        let t = parse_line("Someday maybe").unwrap();
        assert!(body(&t, &cx).is_none());
    }
}
