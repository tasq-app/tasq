//! What falls on which day: external events as the calendar views draw
//! them, in this device's time zone.
//!
//! A timed event is placed by the moment it happens, not by the clock on
//! the invitation: a flight that leaves Madrid at 10:00 and lands in New
//! York at 12:30 local is drawn 10:00–18:30 on a Madrid screen, and
//! redrawn when the computer's zone changes. One that runs past midnight
//! is drawn on each day it touches. Whole-day events sit on every day
//! they cover.

use chrono::{DateTime, Days, NaiveDate, TimeZone, Timelike};

use super::accounts::{self, ExtAccount};
use super::cache::Cached;
use super::event::{Response, When};
use crate::core::calendar::{ExtMark, Occurrence};

/// One event's piece of one day.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalOccurrence {
    pub date: NaiveDate,
    /// Minutes after local midnight; `None` for a whole-day event.
    pub start: Option<u32>,
    /// Minutes on this day.
    pub minutes: u32,
    /// It started on an earlier day / goes on to a later one.
    pub from_before: bool,
    pub goes_on: bool,
    /// The day it starts on, how many days it touches, and which of them
    /// this is.
    pub origin: NaiveDate,
    pub span: u16,
    pub day: u16,
    pub tentative: bool,
    pub account: String,
    pub calendar: String,
    pub id: String,
    pub title: String,
}

fn minute_of_day<Tz: TimeZone>(t: &DateTime<Tz>) -> u32 {
    t.hour() * 60 + t.minute()
}

/// Every piece of `events` falling on `from..=to`, seen from `tz`, by day
/// and start.
pub fn occurrences<Tz: TimeZone>(
    events: &[Cached],
    from: NaiveDate,
    to: NaiveDate,
    tz: &Tz,
) -> Vec<ExternalOccurrence> {
    let mut out = Vec::new();
    for c in events {
        let e = &c.event;
        let piece = |date: NaiveDate, start, minutes, (origin, last): (NaiveDate, NaiveDate)| {
            ExternalOccurrence {
                date,
                start,
                minutes,
                from_before: date > origin,
                goes_on: date < last,
                origin,
                span: ((last - origin).num_days() + 1).clamp(1, 366) as u16,
                day: (date - origin).num_days().clamp(0, 365) as u16,
                tentative: e.response != Response::Accepted,
                account: c.account.clone(),
                calendar: c.calendar.clone(),
                id: e.id.clone(),
                title: e.title.clone(),
            }
        };
        match &e.when {
            When::AllDay { start, end } => {
                let last = end.pred_opt().unwrap_or(*start).max(*start);
                let mut d = (*start).max(from);
                while d < *end && d <= to {
                    out.push(piece(d, None, 0, (*start, last)));
                    let Some(n) = d.succ_opt() else { break };
                    d = n;
                }
            }
            When::Timed { start, end } => {
                let (s, e_) = (start.with_timezone(tz), end.with_timezone(tz));
                let first = s.date_naive();
                // An end at exactly midnight belongs to the day before.
                let last = if e_ > s && minute_of_day(&e_) == 0 && e_.second() == 0 {
                    e_.date_naive().pred_opt().unwrap_or(first)
                } else {
                    e_.date_naive()
                };
                let mut d = first.max(from);
                while d <= last && d <= to {
                    let begin = if d == first { minute_of_day(&s) } else { 0 };
                    let finish = if d == e_.date_naive() {
                        minute_of_day(&e_)
                    } else {
                        24 * 60
                    };
                    out.push(piece(
                        d,
                        Some(begin),
                        finish.saturating_sub(begin),
                        (first, last),
                    ));
                    let Some(n) = d.checked_add_days(Days::new(1)) else {
                        break;
                    };
                    d = n;
                }
            }
        }
    }
    out.sort_by(|a, b| (a.date, a.start, &a.title).cmp(&(b.date, b.start, &b.title)));
    out
}

/// `events` as the calendar views draw them, next to the tasks: each
/// piece an [`Occurrence`] with its calendar's colour and name, and an
/// `abs` of its own past any task list (`usize::MAX - k`, `k` its place in
/// `events`), so it never stands for a task.
pub fn calendar<Tz: TimeZone>(
    events: &[Cached],
    accounts: &[ExtAccount],
    from: NaiveDate,
    to: NaiveDate,
    tz: &Tz,
) -> Vec<Occurrence> {
    occurrences(events, from, to, tz)
        .into_iter()
        .map(|p| {
            let k = events
                .iter()
                .position(|e| {
                    e.account == p.account && e.calendar == p.calendar && e.event.id == p.id
                })
                .unwrap_or(0);
            Occurrence {
                abs: usize::MAX - k,
                date: p.date,
                start: p.start,
                minutes: p.minutes,
                deadline: false,
                projected: false,
                late: false,
                origin: p.origin,
                span: p.span,
                day: p.day,
                ext: Some(Box::new(ExtMark {
                    color: accounts::color_of(accounts, &p.account, &p.calendar),
                    calendar: accounts::calendar_name(accounts, &p.account, &p.calendar),
                    title: p.title,
                    tentative: p.tentative,
                    account: p.account,
                    calendar_id: p.calendar,
                    id: p.id,
                })),
            }
        })
        .collect()
}

/// The event an occurrence from [`calendar`] stands for.
pub fn event_of<'a>(events: &'a [Cached], o: &Occurrence) -> Option<&'a Cached> {
    o.ext.as_ref()?;
    events.get(usize::MAX - o.abs)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::extcal::event::{Parsed, from_google};
    use chrono::FixedOffset;
    use serde_json::{Value, json};

    fn cached(v: Value) -> Cached {
        let Parsed::Keep(event) = from_google(&v) else {
            panic!()
        };
        Cached {
            account: "me".into(),
            calendar: "primary".into(),
            event: *event,
        }
    }

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn madrid() -> FixedOffset {
        FixedOffset::east_opt(3600).unwrap()
    }

    #[test]
    fn a_flight_on_a_madrid_screen() {
        let f = cached(json!({
            "id": "f", "summary": "Flight",
            "start": { "dateTime": "2026-12-01T10:00:00+01:00" },
            "end": { "dateTime": "2026-12-01T12:30:00-05:00" }
        }));
        let o = occurrences(&[f], d("2026-11-30"), d("2026-12-02"), &madrid());
        assert_eq!(o.len(), 1);
        assert_eq!(o[0].start, Some(10 * 60));
        assert_eq!(o[0].minutes, 8 * 60 + 30, "10:00–18:30");
    }

    #[test]
    fn the_same_meeting_from_new_york() {
        let m = cached(json!({
            "id": "m", "summary": "Sync",
            "start": { "dateTime": "2026-12-01T10:00:00+01:00" },
            "end": { "dateTime": "2026-12-01T11:00:00+01:00" }
        }));
        let ny = FixedOffset::west_opt(5 * 3600).unwrap();
        let o = occurrences(&[m], d("2026-12-01"), d("2026-12-01"), &ny);
        assert_eq!(o[0].start, Some(4 * 60), "04:00 in New York");
        assert_eq!(o[0].minutes, 60);
    }

    #[test]
    fn past_midnight_is_drawn_on_both_days() {
        let m = cached(json!({
            "id": "n", "summary": "Night train",
            "start": { "dateTime": "2026-12-01T22:00:00+01:00" },
            "end": { "dateTime": "2026-12-02T07:00:00+01:00" }
        }));
        let o = occurrences(&[m], d("2026-12-01"), d("2026-12-03"), &madrid());
        assert_eq!(o.len(), 2);
        assert_eq!(
            (o[0].start, o[0].minutes, o[0].goes_on),
            (Some(22 * 60), 120, true)
        );
        assert_eq!(
            (o[1].start, o[1].minutes, o[1].from_before),
            (Some(0), 7 * 60, true)
        );
    }

    #[test]
    fn ending_at_midnight_stays_on_its_day() {
        let m = cached(json!({
            "id": "e", "summary": "Evening",
            "start": { "dateTime": "2026-12-01T20:00:00+01:00" },
            "end": { "dateTime": "2026-12-02T00:00:00+01:00" }
        }));
        let o = occurrences(&[m], d("2026-12-01"), d("2026-12-03"), &madrid());
        assert_eq!(o.len(), 1);
        assert_eq!(o[0].minutes, 4 * 60);
        assert!(!o[0].goes_on);
    }

    #[test]
    fn a_hotel_stay_and_a_pending_invite() {
        let h = cached(json!({
            "id": "h", "summary": "Hotel",
            "start": { "date": "2026-12-01" }, "end": { "date": "2026-12-04" }
        }));
        let p = cached(json!({
            "id": "p", "summary": "Interview",
            "start": { "dateTime": "2026-12-02T09:00:00+01:00" },
            "end": { "dateTime": "2026-12-02T10:00:00+01:00" },
            "attendees": [{ "email": "me@x", "self": true, "responseStatus": "needsAction" }]
        }));
        let o = occurrences(&[h, p], d("2026-12-02"), d("2026-12-10"), &madrid());
        let days: Vec<_> = o.iter().filter(|x| x.id == "h").map(|x| x.date).collect();
        assert_eq!(
            days,
            [d("2026-12-02"), d("2026-12-03")],
            "from the window on, end not included"
        );
        assert!(o.iter().find(|x| x.id == "h").unwrap().from_before);
        assert!(o.iter().find(|x| x.id == "p").unwrap().tentative);
        assert!(!o.iter().find(|x| x.id == "h").unwrap().tentative);
    }
}
