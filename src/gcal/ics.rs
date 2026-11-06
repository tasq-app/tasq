//! An event as iCalendar (RFC 5545), the way CalDAV servers (iCloud) take
//! it: built from the same event body Google gets, so both calendars show
//! a task the same way.
//!
//! Times are floating — "09:00", in whatever zone the device is in — which
//! is what a task's time means, and keeps a weekly 09:00 at 09:00 across a
//! daylight-saving change without a `VTIMEZONE`.

use serde_json::Value;

/// Text escaped for a property value.
fn text(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            c => out.push(c),
        }
    }
    out
}

/// A content line folded at 75 octets (continuations start with a space),
/// ended with CRLF.
fn line(out: &mut String, s: &str) {
    let mut width = 0;
    for c in s.chars() {
        let n = c.len_utf8();
        if width + n > 75 {
            out.push_str("\r\n ");
            width = 1;
        }
        out.push(c);
        width += n;
    }
    out.push_str("\r\n");
}

/// `2026-10-12` → `20261012`; `2026-10-12T09:00:00` → `20261012T090000`.
fn compact(s: &str) -> String {
    s.chars().filter(|c| *c != '-' && *c != ':').collect()
}

/// `DTSTART` / `DTEND` from Google's `{date}` or `{dateTime}`.
fn when(name: &str, v: &Value) -> Option<String> {
    if let Some(d) = v["date"].as_str() {
        return Some(format!("{name};VALUE=DATE:{}", compact(d)));
    }
    let dt = v["dateTime"].as_str()?;
    Some(format!("{name}:{}", compact(dt)))
}

/// The iCalendar object for the event `body` (Google's shape, see
/// [`super::event::body`]), its `UID` from `uid`.
pub fn from_body(body: &Value, uid: &str, stamp: &str) -> Option<String> {
    let mut out = String::new();
    for l in [
        "BEGIN:VCALENDAR",
        "VERSION:2.0",
        "PRODID:-//tasq//tasq//EN",
        "CALSCALE:GREGORIAN",
        "BEGIN:VEVENT",
    ] {
        line(&mut out, l);
    }
    line(&mut out, &format!("UID:{uid}@tasq"));
    line(&mut out, &format!("DTSTAMP:{stamp}"));
    line(&mut out, &when("DTSTART", &body["start"])?);
    line(&mut out, &when("DTEND", &body["end"])?);
    let summary = body["summary"].as_str().unwrap_or("");
    line(&mut out, &format!("SUMMARY:{}", text(summary)));
    if let Some(d) = body["description"].as_str() {
        line(&mut out, &format!("DESCRIPTION:{}", text(d)));
    }
    for r in body["recurrence"].as_array().into_iter().flatten() {
        let Some(r) = r.as_str() else { continue };
        // Floating times: no zone on the skipped days, and the end of a
        // repeat as a local time too.
        let r = if let Some(rest) = r.strip_prefix("EXDATE;TZID=") {
            match rest.split_once(':') {
                Some((_, days)) => format!("EXDATE:{days}"),
                None => continue,
            }
        } else if r.starts_with("RRULE:") && r.contains("T235959Z") {
            r.replace("T235959Z", "T235959")
        } else {
            r.to_string()
        };
        line(&mut out, &r);
    }
    for o in body["reminders"]["overrides"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let Some(m) = o["minutes"].as_u64() else {
            continue;
        };
        for l in [
            "BEGIN:VALARM".to_string(),
            "ACTION:DISPLAY".to_string(),
            format!("DESCRIPTION:{}", text(summary)),
            format!("TRIGGER:-PT{m}M"),
            "END:VALARM".to_string(),
        ] {
            line(&mut out, &l);
        }
    }
    line(&mut out, "END:VEVENT");
    line(&mut out, "END:VCALENDAR");
    Some(out)
}

/// Now, as an iCalendar UTC stamp.
pub fn stamp_now() -> String {
    chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::gcal::event::{Context, body};
    use crate::todo::parse_line;

    fn ics(l: &str) -> String {
        let mut t = parse_line(l).unwrap();
        t.id = "T1".into();
        let b = body(
            &t,
            &Context {
                time_zone: "Europe/Madrid",
                color_id: None,
            },
        )
        .unwrap();
        from_body(&b, &t.id, "20261012T000000Z").unwrap()
    }

    #[test]
    fn a_task_as_icalendar() {
        let s = ics("Exam, room 3 +Uni plan:2026-10-12 at:09:00 dur:2h remind:30m");
        assert!(s.contains("UID:T1@tasq\r\n"));
        assert!(s.contains("DTSTART:20261012T090000\r\n"));
        assert!(s.contains("DTEND:20261012T110000\r\n"));
        assert!(s.contains("SUMMARY:Exam\\, room 3\r\n"));
        assert!(s.contains("TRIGGER:-PT30M\r\n"));
        assert!(s.ends_with("END:VCALENDAR\r\n"));

        let s = ics("Trip plan:2026-10-12 end:2026-10-14");
        assert!(s.contains("DTSTART;VALUE=DATE:20261012\r\n"));
        assert!(s.contains("DTEND;VALUE=DATE:20261015\r\n"));

        let s = ics("Gym plan:2026-10-12 at:07:00 rec:1w until:2026-12-31 skip:2026-10-19");
        assert!(s.contains("RRULE:FREQ=WEEKLY;INTERVAL=1;UNTIL=20261231T235959\r\n"));
        assert!(s.contains("EXDATE:20261019T070000\r\n"));
    }

    #[test]
    fn long_lines_fold() {
        let mut out = String::new();
        line(&mut out, &"x".repeat(160));
        let lines: Vec<&str> = out.split("\r\n").collect();
        assert_eq!(lines[0].len(), 75);
        assert!(lines[1].starts_with(' '));
        assert!(lines.iter().all(|l| l.len() <= 75));
    }
}
