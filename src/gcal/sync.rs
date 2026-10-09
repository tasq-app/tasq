//! Keeping the "tasq" calendar in Google up to date: what each task should
//! look like there, against what was sent last time, and only the
//! difference goes — an insert, an update, a delete.
//!
//! The API itself is behind [`CalendarApi`], so the engine runs the same
//! against Google and against the fake the tests use.

use std::collections::BTreeMap;

use anyhow::{Result, bail};
use serde_json::{Value, json};

/// The calendar tasq creates and keeps its events in.
pub const CALENDAR_NAME: &str = "tasq";

/// What sync needs from Google Calendar.
pub trait CalendarApi {
    /// Create the calendar; its id.
    fn create_calendar(&mut self, name: &str, time_zone: &str) -> Result<String>;
    /// Whether the calendar `id` still exists (the user may delete it).
    fn calendar_exists(&mut self, id: &str) -> Result<bool>;
    /// Add an event; its id.
    fn insert(&mut self, calendar: &str, body: &Value) -> Result<String>;
    /// Replace an event. `Ok(false)` when it's gone (deleted in Google).
    fn update(&mut self, calendar: &str, id: &str, body: &Value) -> Result<bool>;
    /// Remove an event (gone already is fine).
    fn delete(&mut self, calendar: &str, id: &str) -> Result<()>;
}

/// What was sent: the calendar, and per task its event and a fingerprint
/// of the body it was sent with. Kept in the data directory.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncState {
    pub calendar_id: Option<String>,
    /// Task id → (event id, fingerprint).
    pub events: BTreeMap<String, (String, u64)>,
}

impl SyncState {
    pub fn to_json(&self) -> String {
        let events: serde_json::Map<String, Value> = self
            .events
            .iter()
            .map(|(task, (event, hash))| (task.clone(), json!([event, hash.to_string()])))
            .collect();
        json!({ "calendar": self.calendar_id, "events": events }).to_string()
    }

    pub fn from_json(s: &str) -> Self {
        let Ok(v) = serde_json::from_str::<Value>(s) else {
            return Self::default();
        };
        let events = v["events"]
            .as_object()
            .map(|m| {
                m.iter()
                    .filter_map(|(task, pair)| {
                        let event = pair.get(0)?.as_str()?.to_string();
                        let hash = pair.get(1)?.as_str()?.parse().ok()?;
                        Some((task.clone(), (event, hash)))
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self {
            calendar_id: v["calendar"].as_str().map(str::to_string),
            events,
        }
    }
}

/// A stable fingerprint of an event body (FNV-1a over its JSON), the same
/// from one run to the next.
pub fn fingerprint(body: &Value) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in body.to_string().bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// What one sync did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub inserted: usize,
    pub updated: usize,
    pub deleted: usize,
    /// Events that failed, with why; the rest still went.
    pub errors: Vec<String>,
}

/// Bring the calendar in line with `desired` (task id → event body):
/// create the calendar if it's missing, add what's new, update what
/// changed, delete what's no longer there. `state` is updated as it goes,
/// so a sync cut short resumes where it stopped.
pub fn sync(
    api: &mut dyn CalendarApi,
    state: &mut SyncState,
    desired: &BTreeMap<String, Value>,
    time_zone: &str,
) -> Result<Report> {
    let mut report = Report::default();
    // Everything gone at once is more likely a list that didn't load than
    // a clean slate: don't empty the calendar on it.
    if desired.is_empty() && state.events.len() >= 3 {
        bail!("no tasks to send · the calendar is left as it is");
    }
    // The calendar: kept, or made again (and every event with it).
    let calendar = match state.calendar_id.clone() {
        Some(id) if api.calendar_exists(&id)? => id,
        _ => {
            let id = api.create_calendar(CALENDAR_NAME, time_zone)?;
            state.calendar_id = Some(id.clone());
            state.events.clear();
            id
        }
    };

    // Gone from tasq (done, deleted, no day any more): out of the calendar.
    let stale: Vec<String> = state
        .events
        .keys()
        .filter(|k| !desired.contains_key(*k))
        .cloned()
        .collect();
    for task in stale {
        let Some((event, _)) = state.events.get(&task).cloned() else {
            continue;
        };
        match api.delete(&calendar, &event) {
            Ok(()) => {
                state.events.remove(&task);
                report.deleted += 1;
            }
            Err(e) => report.errors.push(format!("delete {task}: {e}")),
        }
    }

    for (task, body) in desired {
        let hash = fingerprint(body);
        match state.events.get(task).cloned() {
            Some((_, h)) if h == hash => {}
            Some((event, _)) => match api.update(&calendar, &event, body) {
                Ok(true) => {
                    state.events.insert(task.clone(), (event, hash));
                    report.updated += 1;
                }
                // Deleted in Google: put it back.
                Ok(false) => match api.insert(&calendar, body) {
                    Ok(id) => {
                        state.events.insert(task.clone(), (id, hash));
                        report.inserted += 1;
                    }
                    Err(e) => report.errors.push(format!("insert {task}: {e}")),
                },
                Err(e) => report.errors.push(format!("update {task}: {e}")),
            },
            None => match api.insert(&calendar, body) {
                Ok(id) => {
                    state.events.insert(task.clone(), (id, hash));
                    report.inserted += 1;
                }
                Err(e) => report.errors.push(format!("insert {task}: {e}")),
            },
        }
    }
    Ok(report)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
pub(crate) mod tests {
    use super::*;

    /// Google Calendar in memory.
    #[derive(Default)]
    pub(crate) struct FakeGoogle {
        pub calendars: BTreeMap<String, BTreeMap<String, Value>>,
        pub next: usize,
        pub calls: usize,
    }

    impl CalendarApi for FakeGoogle {
        fn create_calendar(&mut self, _name: &str, _tz: &str) -> Result<String> {
            self.next += 1;
            let id = format!("cal{}", self.next);
            self.calendars.insert(id.clone(), BTreeMap::new());
            Ok(id)
        }
        fn calendar_exists(&mut self, id: &str) -> Result<bool> {
            Ok(self.calendars.contains_key(id))
        }
        fn insert(&mut self, calendar: &str, body: &Value) -> Result<String> {
            self.calls += 1;
            self.next += 1;
            let id = format!("ev{}", self.next);
            self.calendars
                .get_mut(calendar)
                .unwrap()
                .insert(id.clone(), body.clone());
            Ok(id)
        }
        fn update(&mut self, calendar: &str, id: &str, body: &Value) -> Result<bool> {
            self.calls += 1;
            let cal = self.calendars.get_mut(calendar).unwrap();
            Ok(match cal.get_mut(id) {
                Some(b) => {
                    *b = body.clone();
                    true
                }
                None => false,
            })
        }
        fn delete(&mut self, calendar: &str, id: &str) -> Result<()> {
            self.calls += 1;
            self.calendars.get_mut(calendar).unwrap().remove(id);
            Ok(())
        }
    }

    fn want(pairs: &[(&str, &str)]) -> BTreeMap<String, Value> {
        pairs
            .iter()
            .map(|(k, s)| (k.to_string(), json!({ "summary": s })))
            .collect()
    }

    #[test]
    fn only_what_changed_goes_to_google() {
        let mut g = FakeGoogle::default();
        let mut st = SyncState::default();
        let r = sync(&mut g, &mut st, &want(&[("a", "A"), ("b", "B")]), "UTC").unwrap();
        assert_eq!((r.inserted, r.updated, r.deleted), (2, 0, 0));
        let cal = st.calendar_id.clone().unwrap();
        assert_eq!(g.calendars[&cal].len(), 2);

        // Nothing changed: not a single call.
        let calls = g.calls;
        let r = sync(&mut g, &mut st, &want(&[("a", "A"), ("b", "B")]), "UTC").unwrap();
        assert_eq!((r.inserted, r.updated, r.deleted), (0, 0, 0));
        assert_eq!(g.calls, calls);

        // b edited, a done (gone), c new.
        let r = sync(&mut g, &mut st, &want(&[("b", "B2"), ("c", "C")]), "UTC").unwrap();
        assert_eq!((r.inserted, r.updated, r.deleted), (1, 1, 1));
        let titles: Vec<&str> = g.calendars[&cal]
            .values()
            .map(|v| v["summary"].as_str().unwrap())
            .collect();
        assert_eq!(titles, ["B2", "C"]);

        // The state survives a restart.
        assert_eq!(SyncState::from_json(&st.to_json()), st);
    }

    #[test]
    fn an_empty_list_doesnt_empty_the_calendar() {
        let mut g = FakeGoogle::default();
        let mut st = SyncState::default();
        sync(
            &mut g,
            &mut st,
            &want(&[("a", "A"), ("b", "B"), ("c", "C")]),
            "UTC",
        )
        .unwrap();
        assert!(sync(&mut g, &mut st, &BTreeMap::new(), "UTC").is_err());
        assert_eq!(st.events.len(), 3);
    }

    #[test]
    fn deleted_in_google_comes_back() {
        let mut g = FakeGoogle::default();
        let mut st = SyncState::default();
        sync(&mut g, &mut st, &want(&[("a", "A")]), "UTC").unwrap();
        let cal = st.calendar_id.clone().unwrap();
        // An event deleted by hand, then the task edited: it's put back.
        g.calendars.get_mut(&cal).unwrap().clear();
        let r = sync(&mut g, &mut st, &want(&[("a", "A2")]), "UTC").unwrap();
        assert_eq!(r.inserted, 1);
        // The whole calendar deleted: made again, with everything.
        g.calendars.clear();
        let r = sync(&mut g, &mut st, &want(&[("a", "A2")]), "UTC").unwrap();
        assert_eq!(r.inserted, 1);
        assert_ne!(st.calendar_id.as_deref(), Some(cal.as_str()));
    }
}
