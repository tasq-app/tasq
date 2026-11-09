//! Keeping the cache current, one calendar at a time.
//!
//! The first round asks Google for everything from a month ago on, with
//! each repeat expanded (`singleEvents`), and keeps the `nextSyncToken`
//! Google hands back. Every round after sends that token and gets only
//! what changed since: usually nothing, one small request. When Google
//! says the token is too old (`410 Gone`) the calendar starts over.
//!
//! Only a window is kept — a month back to a year ahead — and a full round
//! runs again once a week, so events that drift into the window as time
//! passes (a series far ahead) show up without having changed. If Google
//! ever answers a full round without a token, every round is a full one:
//! slower, never wrong.

use anyhow::{Result, bail};
use serde_json::Value;

use super::cache::Cache;

/// One page of `events.list`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Page {
    pub items: Vec<Value>,
    pub next_page: Option<String>,
    pub next_sync: Option<String>,
}

/// What `events.list` answered.
#[derive(Debug, Clone, PartialEq)]
pub enum Listed {
    Page(Page),
    /// The sync token is no longer good: start over.
    Gone,
}

/// A calendar in the account's list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarInfo {
    pub id: String,
    pub name: String,
    pub primary: bool,
    /// Google's own colour for it, as `#rrggbb`: a hint for the colour
    /// tasq offers first.
    pub color: Option<String>,
}

/// What sync needs from a calendar provider.
pub trait EventsApi {
    /// The calendars the account can see.
    fn calendars(&mut self) -> Result<Vec<CalendarInfo>>;
    /// A page of events: with `sync_token`, only what changed since it;
    /// without, everything from `time_min` (RFC 3339) on.
    fn events(
        &mut self,
        calendar: &str,
        sync_token: Option<&str>,
        time_min: &str,
        page_token: Option<&str>,
    ) -> Result<Listed>;
}

/// Days kept before today, and after.
pub const DAYS_BACK: i64 = 30;
pub const DAYS_AHEAD: i64 = 366;
/// A full round at least this often (seconds).
pub const FULL_EVERY: i64 = 7 * 24 * 3600;
/// More pages than this in one round is a runaway: stop.
const MAX_PAGES: usize = 200;

/// What a round did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Report {
    pub full: bool,
    pub kept: usize,
    pub dropped: usize,
}

fn rfc3339(secs: i64) -> String {
    chrono::DateTime::from_timestamp(secs, 0)
        .unwrap_or_default()
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}

/// Every page of one round; `Ok(None)` when the token was turned down.
fn fetch(
    api: &mut dyn EventsApi,
    calendar: &str,
    token: Option<&str>,
    time_min: &str,
) -> Result<Option<(Vec<Value>, Option<String>)>> {
    let mut items = Vec::new();
    let mut page_token: Option<String> = None;
    for _ in 0..MAX_PAGES {
        let page = match api.events(calendar, token, time_min, page_token.as_deref())? {
            Listed::Gone => return Ok(None),
            Listed::Page(p) => p,
        };
        items.extend(page.items);
        match page.next_page {
            Some(next) => page_token = Some(next),
            None => return Ok(Some((items, page.next_sync))),
        }
    }
    bail!("too many pages from {calendar}")
}

/// Bring one calendar of `account` up to date at `now` (Unix seconds).
/// A failure is noted against the calendar and returned; what was cached
/// stays as it was.
pub fn sync_calendar(
    api: &mut dyn EventsApi,
    cache: &mut Cache,
    account: &str,
    calendar: &str,
    now: i64,
) -> Result<Report> {
    let r = round(api, cache, account, calendar, now);
    if let Err(e) = &r {
        let _ = cache.failed(account, calendar, &e.to_string());
    }
    r
}

fn round(
    api: &mut dyn EventsApi,
    cache: &mut Cache,
    account: &str,
    calendar: &str,
    now: i64,
) -> Result<Report> {
    let day = 24 * 3600;
    let window = (now - DAYS_BACK * day, now + DAYS_AHEAD * day);
    let time_min = rfc3339(window.0);
    let mark = cache.mark(account, calendar)?;
    let stale = mark.last_full.is_none_or(|t| now - t >= FULL_EVERY);
    let token = mark.sync_token.filter(|_| !stale);

    if let Some(t) = &token
        && let Some((items, next)) = fetch(api, calendar, Some(t), &time_min)?
    {
        let (kept, dropped) = cache.apply(
            account,
            calendar,
            false,
            &items,
            window,
            next.as_deref(),
            now,
        )?;
        return Ok(Report {
            full: false,
            kept,
            dropped,
        });
    }
    let Some((items, next)) = fetch(api, calendar, None, &time_min)? else {
        bail!("Google turned down a full sync of {calendar}");
    };
    let (kept, dropped) = cache.apply(
        account,
        calendar,
        true,
        &items,
        window,
        next.as_deref(),
        now,
    )?;
    Ok(Report {
        full: true,
        kept,
        dropped,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeMap;

    /// A calendar on a pretend server: events by id, a change counter, and
    /// what each sync token saw.
    #[derive(Default)]
    pub struct Fake {
        pub events: BTreeMap<String, (u32, Value)>,
        pub clock: u32,
        pub page_size: usize,
        /// Tokens the server no longer takes.
        pub expired: bool,
        pub calls: Vec<String>,
    }

    impl Fake {
        pub fn put(&mut self, v: Value) {
            self.clock += 1;
            let id = v["id"].as_str().unwrap().to_string();
            self.events.insert(id, (self.clock, v));
        }
        pub fn cancel(&mut self, id: &str) {
            self.put(json!({ "id": id, "status": "cancelled" }));
        }
    }

    impl EventsApi for Fake {
        fn calendars(&mut self) -> Result<Vec<CalendarInfo>> {
            Ok(vec![CalendarInfo {
                id: "primary".into(),
                name: "Me".into(),
                primary: true,
                color: None,
            }])
        }

        fn events(
            &mut self,
            _calendar: &str,
            sync_token: Option<&str>,
            _time_min: &str,
            page_token: Option<&str>,
        ) -> Result<Listed> {
            self.calls.push(format!(
                "{}{}",
                if sync_token.is_some() { "inc" } else { "full" },
                page_token.map(|p| format!("@{p}")).unwrap_or_default()
            ));
            if sync_token.is_some() && self.expired {
                return Ok(Listed::Gone);
            }
            let since: u32 = sync_token.map_or(0, |t| t.parse().unwrap());
            let all: Vec<Value> = self
                .events
                .values()
                .filter(|(at, v)| *at > since && (since > 0 || v["status"] != "cancelled"))
                .map(|(_, v)| v.clone())
                .collect();
            let size = self.page_size.max(1);
            let start: usize = page_token.map_or(0, |p| p.parse().unwrap());
            let items = all.iter().skip(start).take(size).cloned().collect();
            let more = start + size < all.len();
            Ok(Listed::Page(Page {
                items,
                next_page: more.then(|| (start + size).to_string()),
                next_sync: (!more).then(|| self.clock.to_string()),
            }))
        }
    }

    pub fn meeting(id: &str, at: &str) -> Value {
        json!({
            "id": id, "summary": id,
            "start": { "dateTime": format!("{at}:00+01:00") },
            "end": { "dateTime": format!("{at}:00+01:00") }
        })
    }

    const NOW: i64 = 1_793_000_000; // 2026-10-26

    #[test]
    fn full_then_only_what_changed() {
        let mut api = Fake {
            page_size: 2,
            ..Fake::default()
        };
        for (i, at) in ["2026-11-02T09:00", "2026-11-03T09:00", "2026-11-04T09:00"]
            .iter()
            .enumerate()
        {
            api.put(meeting(&format!("m{i}"), at));
        }
        let mut cache = Cache::in_memory().unwrap();
        let r = sync_calendar(&mut api, &mut cache, "me", "primary", NOW).unwrap();
        assert_eq!(
            r,
            Report {
                full: true,
                kept: 3,
                dropped: 0
            }
        );
        assert_eq!(api.calls, ["full", "full@2"], "both pages");

        api.calls.clear();
        api.cancel("m1");
        api.put(meeting("m3", "2026-11-05T09:00"));
        let r = sync_calendar(&mut api, &mut cache, "me", "primary", NOW + 300).unwrap();
        assert_eq!(
            r,
            Report {
                full: false,
                kept: 1,
                dropped: 1
            }
        );
        assert_eq!(api.calls, ["inc"]);
        let ids: Vec<String> = cache
            .between(NOW, NOW + 86400 * 30)
            .unwrap()
            .into_iter()
            .map(|c| c.event.id)
            .collect();
        assert_eq!(ids, ["m0", "m2", "m3"]);
    }

    #[test]
    fn an_expired_token_starts_over() {
        let mut api = Fake::default();
        api.put(meeting("a", "2026-11-02T09:00"));
        let mut cache = Cache::in_memory().unwrap();
        sync_calendar(&mut api, &mut cache, "me", "primary", NOW).unwrap();
        api.expired = true;
        api.calls.clear();
        let r = sync_calendar(&mut api, &mut cache, "me", "primary", NOW + 60).unwrap();
        assert!(r.full);
        assert_eq!(api.calls, ["inc", "full"]);
        assert_eq!(cache.between(NOW, NOW + 86400 * 30).unwrap().len(), 1);
    }

    #[test]
    fn a_full_round_once_a_week_picks_up_what_drifted_in() {
        let mut api = Fake::default();
        // Fourteen months ahead: outside the window today.
        api.put(meeting("far", "2027-12-20T09:00"));
        let mut cache = Cache::in_memory().unwrap();
        let r = sync_calendar(&mut api, &mut cache, "me", "primary", NOW).unwrap();
        assert_eq!(r.kept, 0);
        // Two months on, nothing changed in Google; the weekly full round
        // brings it in.
        let later = NOW + 60 * 86400;
        api.calls.clear();
        let r = sync_calendar(&mut api, &mut cache, "me", "primary", later).unwrap();
        assert!(r.full);
        assert_eq!(r.kept, 1);
    }

    #[test]
    fn a_failure_keeps_the_cache_and_is_noted() {
        struct Down;
        impl EventsApi for Down {
            fn calendars(&mut self) -> Result<Vec<CalendarInfo>> {
                bail!("offline")
            }
            fn events(
                &mut self,
                _: &str,
                _: Option<&str>,
                _: &str,
                _: Option<&str>,
            ) -> Result<Listed> {
                bail!("offline")
            }
        }
        let mut api = Fake::default();
        api.put(meeting("a", "2026-11-02T09:00"));
        let mut cache = Cache::in_memory().unwrap();
        sync_calendar(&mut api, &mut cache, "me", "primary", NOW).unwrap();
        assert!(sync_calendar(&mut Down, &mut cache, "me", "primary", NOW + 60).is_err());
        assert_eq!(cache.between(NOW, NOW + 86400 * 30).unwrap().len(), 1);
        let m = cache.mark("me", "primary").unwrap();
        assert_eq!(m.last_error.as_deref(), Some("offline"));
        assert!(m.sync_token.is_some(), "the token survives");
    }
}
