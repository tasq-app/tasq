//! An event from another calendar, as tasq keeps it.
//!
//! Built from Google's own event resource, so what you see is what Google
//! has: the title as it is (a flight Gmail added reads "Flight to Madrid
//! (IB 3456)", and stays that way), the times with their real zone, who's
//! invited and whether you said yes, and the links — the event, Meet, the
//! files attached. Repeats come already expanded by Google, one event per
//! instance, so a moved or cancelled meeting in a series is just that
//! instance changed.

use chrono::{DateTime, FixedOffset, NaiveDate};
use serde_json::Value;

/// When an event happens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum When {
    /// At a moment, with the offset it was given in (a flight leaves at
    /// 10:00 +01:00 and lands at 12:30 -05:00).
    Timed {
        start: DateTime<FixedOffset>,
        end: DateTime<FixedOffset>,
    },
    /// Whole days, `end` not included (a one-day event ends the next day),
    /// the way Google and iCalendar count them.
    AllDay { start: NaiveDate, end: NaiveDate },
}

/// Your answer to an invitation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Response {
    /// Yours, or you said yes.
    Accepted,
    Tentative,
    /// Not answered yet.
    Pending,
}

/// Someone invited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attendee {
    pub email: String,
    pub name: Option<String>,
    /// "accepted", "tentative", "declined", "needsAction".
    pub response: String,
    pub organizer: bool,
}

/// A link with a name: a file attached to the event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub title: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalEvent {
    pub id: String,
    pub title: String,
    pub when: When,
    pub location: Option<String>,
    pub description: Option<String>,
    pub response: Response,
    pub attendees: Vec<Attendee>,
    /// The event in Google Calendar.
    pub html_link: Option<String>,
    /// The video call (Meet, or another conference's video entry).
    pub meet_link: Option<String>,
    pub attachments: Vec<Link>,
    /// Set on an instance of a repeating event: the series it belongs to.
    pub recurring_id: Option<String>,
    /// Google's `eventType`: "default", "fromGmail", "outOfOffice", …
    pub kind: String,
}

/// What an event from Google means for the cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Parsed {
    /// Show it.
    Keep(Box<ExternalEvent>),
    /// Don't show it (cancelled, declined, one of tasq's own): drop it
    /// from the cache if it's there.
    Drop(String),
    /// Nothing to go on (no id).
    Skip,
}

fn text(v: &Value) -> Option<String> {
    v.as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn when(v: &Value) -> Option<When> {
    let (s, e) = (&v["start"], &v["end"]);
    if let (Some(s), Some(e)) = (s["dateTime"].as_str(), e["dateTime"].as_str()) {
        let start = DateTime::parse_from_rfc3339(s).ok()?;
        let end = DateTime::parse_from_rfc3339(e).ok()?;
        return Some(When::Timed {
            start,
            end: end.max(start),
        });
    }
    let start = NaiveDate::parse_from_str(s["date"].as_str()?, "%Y-%m-%d").ok()?;
    let end = e["date"]
        .as_str()
        .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
        .filter(|d| *d > start)
        .unwrap_or_else(|| start.succ_opt().unwrap_or(start));
    Some(When::AllDay { start, end })
}

/// Hosts of video calls, for links that come only in the text of an
/// invitation (a Teams or Zoom invite sent from Outlook).
const CALL_HOSTS: &[&str] = &[
    "teams.microsoft.com",
    "teams.live.com",
    "zoom.us",
    "zoom.com",
    "zoomgov.com",
    "webex.com",
    "whereby.com",
    "meet.jit.si",
    "meet.google.com",
    "chime.aws",
    "meet.goto.com",
    "gotomeeting.com",
    "bluejeans.com",
    "around.co",
];

/// Whether `host` is `base` or inside it (`us02web.zoom.us`).
fn host_is(host: &str, base: &str) -> bool {
    host == base || host.ends_with(&format!(".{base}"))
}

/// The host of an `https://` URL.
pub fn host_of(url: &str) -> Option<&str> {
    let rest = url.strip_prefix("https://")?;
    let end = rest.find(['/', '?', '#', ':']).unwrap_or(rest.len());
    Some(&rest[..end])
}

/// The first video-call link in `text` (plain or HTML).
pub fn call_in(text: &str) -> Option<String> {
    text.split(|c: char| {
        c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\'' | '(' | ')' | '[' | ']')
    })
    .filter_map(|w| {
        let start = w.find("https://")?;
        let url = w[start..].trim_end_matches(['.', ',', ';', '!', '?']);
        let url = url.replace("&amp;", "&");
        let host = host_of(&url)?.to_ascii_lowercase();
        CALL_HOSTS.iter().any(|b| host_is(&host, b)).then_some(url)
    })
    .next()
}

/// The video call: Meet's own link, else a conference's video entry
/// (Zoom or Teams added from Google Calendar), else one written in the
/// place or the description (an invite from Outlook).
fn meet(v: &Value) -> Option<String> {
    text(&v["hangoutLink"])
        .or_else(|| {
            v["conferenceData"]["entryPoints"]
                .as_array()?
                .iter()
                .find(|p| p["entryPointType"] == "video")
                .and_then(|p| text(&p["uri"]))
        })
        .or_else(|| v["location"].as_str().and_then(call_in))
        .or_else(|| v["description"].as_str().and_then(call_in))
}

/// Read one item of Google's `events.list`.
pub fn from_google(v: &Value) -> Parsed {
    let Some(id) = text(&v["id"]) else {
        return Parsed::Skip;
    };
    let drop = || Parsed::Drop(id.clone());
    if v["status"] == "cancelled" {
        return drop();
    }
    // One of the events tasq writes for your tasks: it's already in tasq.
    if !v["extendedProperties"]["private"]["tasq"].is_null() {
        return drop();
    }
    let kind = text(&v["eventType"]).unwrap_or_else(|| "default".to_string());
    // "Working from the office" is a status, not something on the day.
    if kind == "workingLocation" {
        return drop();
    }
    let attendees: Vec<Attendee> = v["attendees"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|p| p["resource"] != true)
                .filter_map(|p| {
                    Some(Attendee {
                        email: text(&p["email"])?,
                        name: text(&p["displayName"]),
                        response: text(&p["responseStatus"])
                            .unwrap_or_else(|| "needsAction".to_string()),
                        organizer: p["organizer"] == true,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let mine = v["attendees"]
        .as_array()
        .and_then(|a| a.iter().find(|p| p["self"] == true));
    let response = match mine.and_then(|p| p["responseStatus"].as_str()) {
        Some("declined") => return drop(),
        Some("tentative") => Response::Tentative,
        Some("needsAction") => Response::Pending,
        _ if v["status"] == "tentative" => Response::Tentative,
        _ => Response::Accepted,
    };
    let Some(when) = when(v) else {
        return Parsed::Skip;
    };
    let attachments = v["attachments"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|f| {
                    let url = text(&f["fileUrl"])?;
                    Some(Link {
                        title: text(&f["title"]).unwrap_or_else(|| url.clone()),
                        url,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Parsed::Keep(Box::new(ExternalEvent {
        title: text(&v["summary"]).unwrap_or_else(|| "(no title)".to_string()),
        when,
        location: text(&v["location"]),
        description: text(&v["description"]),
        response,
        attendees,
        html_link: text(&v["htmlLink"]),
        meet_link: meet(v),
        attachments,
        recurring_id: text(&v["recurringEventId"]),
        kind,
        id,
    }))
}

impl ExternalEvent {
    /// Start and end as Unix seconds, for the cache's window. Whole days
    /// count from midnight UTC; callers looking up a day widen the window
    /// by a day either side, so no zone is missed.
    pub fn bounds(&self) -> (i64, i64) {
        match &self.when {
            When::Timed { start, end } => (start.timestamp(), end.timestamp()),
            When::AllDay { start, end } => {
                let ts = |d: &NaiveDate| {
                    d.and_hms_opt(0, 0, 0)
                        .map_or(0, |t| t.and_utc().timestamp())
                };
                (ts(start), ts(end))
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use serde_json::json;

    fn keep(v: Value) -> ExternalEvent {
        match from_google(&v) {
            Parsed::Keep(e) => *e,
            p => panic!("not kept: {p:?}"),
        }
    }

    #[test]
    fn a_meeting_with_its_people_and_links() {
        let e = keep(json!({
            "id": "m1",
            "status": "confirmed",
            "summary": " Weekly sync ",
            "htmlLink": "https://www.google.com/calendar/event?eid=bTE",
            "start": { "dateTime": "2026-11-10T10:00:00+01:00", "timeZone": "Europe/Madrid" },
            "end": { "dateTime": "2026-11-10T10:30:00+01:00", "timeZone": "Europe/Madrid" },
            "recurringEventId": "series",
            "conferenceData": { "entryPoints": [
                { "entryPointType": "phone", "uri": "tel:+34-555" },
                { "entryPointType": "video", "uri": "https://meet.google.com/abc" }
            ]},
            "attachments": [{ "fileUrl": "https://drive.google.com/file/d/1", "title": "Agenda" }],
            "attendees": [
                { "email": "boss@work.com", "organizer": true, "responseStatus": "accepted" },
                { "email": "me@work.com", "self": true, "responseStatus": "needsAction" },
                { "email": "room@resource.calendar.google.com", "resource": true }
            ]
        }));
        assert_eq!(e.title, "Weekly sync");
        assert_eq!(e.response, Response::Pending);
        assert_eq!(e.meet_link.as_deref(), Some("https://meet.google.com/abc"));
        assert_eq!(e.attachments[0].title, "Agenda");
        assert_eq!(e.attendees.len(), 2, "the room isn't a person");
        assert!(e.attendees[0].organizer);
        assert_eq!(e.recurring_id.as_deref(), Some("series"));
        let (s, end) = e.bounds();
        assert_eq!(end - s, 30 * 60);
    }

    #[test]
    fn a_flight_keeps_its_title_and_both_zones() {
        let e = keep(json!({
            "id": "f1",
            "eventType": "fromGmail",
            "summary": "Flight to New York (IB 6251)",
            "hangoutLink": null,
            "start": { "dateTime": "2026-12-01T10:00:00+01:00" },
            "end": { "dateTime": "2026-12-01T12:30:00-05:00" }
        }));
        assert_eq!(e.title, "Flight to New York (IB 6251)");
        assert_eq!(e.kind, "fromGmail");
        let (s, end) = e.bounds();
        assert_eq!(end - s, 8 * 3600 + 30 * 60, "8 h 30 in the air");
        let When::Timed { end, .. } = e.when else {
            panic!()
        };
        assert_eq!(
            end.format("%H:%M").to_string(),
            "12:30",
            "local time on landing"
        );
    }

    #[test]
    fn calls_written_in_the_invitation() {
        let teams = keep(json!({
            "id": "t", "summary": "Weekly",
            "start": { "dateTime": "2026-11-10T10:00:00Z" },
            "end": { "dateTime": "2026-11-10T11:00:00Z" },
            "description": "<p>Join: <a href=\"https://teams.microsoft.com/l/meetup-join/19%3ameeting_X/0?context=%7b%7d&amp;x=1\">Click here</a></p>"
        }));
        assert_eq!(
            teams.meet_link.as_deref(),
            Some("https://teams.microsoft.com/l/meetup-join/19%3ameeting_X/0?context=%7b%7d&x=1")
        );
        let zoom = keep(json!({
            "id": "z", "summary": "1:1",
            "start": { "dateTime": "2026-11-10T10:00:00Z" },
            "end": { "dateTime": "2026-11-10T11:00:00Z" },
            "location": "https://us02web.zoom.us/j/123456?pwd=abc.",
            "description": "Agenda: https://example.com/doc"
        }));
        assert_eq!(
            zoom.meet_link.as_deref(),
            Some("https://us02web.zoom.us/j/123456?pwd=abc")
        );
        assert_eq!(
            call_in("see https://example.com and https://notzoom.us/x"),
            None
        );
    }

    #[test]
    fn whole_days() {
        let e = keep(json!({
            "id": "h1",
            "summary": "Hotel",
            "start": { "date": "2026-12-01" },
            "end": { "date": "2026-12-04" }
        }));
        let d = |s| NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
        assert_eq!(
            e.when,
            When::AllDay {
                start: d("2026-12-01"),
                end: d("2026-12-04")
            }
        );
    }

    #[test]
    fn what_stays_out() {
        let at = json!({ "dateTime": "2026-11-10T10:00:00Z" });
        let base = |extra: Value| {
            let mut v = json!({ "id": "x", "start": at, "end": at });
            v.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            from_google(&v)
        };
        let dropped = Parsed::Drop("x".to_string());
        assert_eq!(base(json!({ "status": "cancelled" })), dropped);
        assert_eq!(
            base(json!({ "extendedProperties": { "private": { "tasq": "T1" } } })),
            dropped,
            "tasq's own events"
        );
        assert_eq!(base(json!({ "eventType": "workingLocation" })), dropped);
        assert_eq!(
            base(
                json!({ "attendees": [{ "email": "me@x", "self": true, "responseStatus": "declined" }] })
            ),
            dropped
        );
        assert_eq!(from_google(&json!({ "summary": "no id" })), Parsed::Skip);
        let Parsed::Keep(e) = base(json!({ "status": "tentative" })) else {
            panic!()
        };
        assert_eq!(e.response, Response::Tentative);
        assert_eq!(e.title, "(no title)");
    }
}
