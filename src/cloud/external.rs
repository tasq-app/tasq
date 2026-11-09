//! Your other calendars, read by tasq's server: the events of the Google
//! accounts you put on "all my devices", for the web calendar and Claude.
//!
//! Only while one of them is connected (see [`super::assistant`]), and
//! only with what that connection can read: the account's own sync, where
//! each shared account travels with its access (sealed, like your tasks).
//! The server asks Google for the calendars you show, keeps what Google
//! sends **in memory only** — never on disk — and asks again for what
//! changed (Google's sync tokens) at most every couple of minutes. An
//! account left alone for a while is forgotten, and so is one you stop
//! sharing.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, RwLock};

use chrono::{NaiveDate, TimeZone};
use serde_json::{Value, json};

use crate::core::calendar::Occurrence;
use crate::extcal::accounts::{self, ExtAccount};
use crate::extcal::cache::{Cache, Cached};
use crate::extcal::event::{Response, When};
use crate::extcal::sync::{EventsApi, sync_calendar};
use crate::gcal::auth::{Account, Client};

/// How often the server asks Google again (seconds).
pub const REFRESH_SECS: i64 = 120;
/// An account nobody looked at for this long is let go (seconds).
const FORGET_SECS: i64 = 30 * 60;

/// Makes the reader of one Google account: its email, its refresh token,
/// and the app's client the token belongs to.
pub type ApiMaker =
    dyn Fn(&str, &str, Option<Client>) -> Option<Box<dyn EventsApi + Send>> + Send + Sync;

fn google(email: &str, token: &str, client: Option<Client>) -> Option<Box<dyn EventsApi + Send>> {
    let client = client.or_else(|| Client::find(None, None))?;
    let account = Account {
        email: email.to_string(),
        refresh_token: token.to_string(),
    };
    Some(Box::new(crate::extcal::google::GoogleEvents::new(
        client, account, None,
    )))
}

/// One tasq account's other calendars, as the server holds them.
struct Held {
    cache: Cache,
    /// The shared accounts as last read, with their access.
    shared: Vec<(String, String)>,
    readers: HashMap<String, (String, Box<dyn EventsApi + Send>)>,
    last_round: i64,
    last_seen: i64,
    status: BTreeMap<String, String>,
}

/// Every account's, keyed by the account's id.
pub struct Externals {
    held: Mutex<HashMap<String, Arc<Mutex<Held>>>>,
    maker: RwLock<Box<ApiMaker>>,
}

impl Default for Externals {
    fn default() -> Self {
        Self {
            held: Mutex::new(HashMap::new()),
            maker: RwLock::new(Box::new(google)),
        }
    }
}

/// The other calendars' events for one answer.
#[derive(Default)]
pub struct ExtCalendar {
    pub accounts: Vec<ExtAccount>,
    /// The events of the calendars you show.
    pub events: Vec<Cached>,
    /// Per Google account: what went wrong last time, if anything.
    pub status: BTreeMap<String, String>,
}

impl Externals {
    /// Read Google accounts with `maker` instead (tests).
    #[cfg(test)]
    pub fn set_maker(&self, maker: Box<ApiMaker>) {
        if let Ok(mut m) = self.maker.write() {
            *m = maker;
        }
    }

    /// The events of `account`'s shared calendars between `from` and `to`
    /// (Unix seconds), fetching what changed if it's been a while. `shared`
    /// is the account's shared rows (email, value) as its sync has them.
    pub fn read(
        &self,
        account: &str,
        shared: &[(String, String)],
        (from, to): (i64, i64),
        now: i64,
    ) -> ExtCalendar {
        let parsed: Vec<(ExtAccount, String, String)> = shared
            .iter()
            .filter_map(|(email, value)| {
                let (a, token) = accounts::from_shared(email, value)?;
                Some((a, token, value.clone()))
            })
            .collect();
        let held = {
            let Ok(mut all) = self.held.lock() else {
                return ExtCalendar::default();
            };
            all.retain(|_, h| h.lock().is_ok_and(|h| now - h.last_seen < FORGET_SECS));
            if parsed.is_empty() {
                all.remove(account);
                return ExtCalendar::default();
            }
            let Ok(cache) = Cache::in_memory() else {
                return ExtCalendar::default();
            };
            all.entry(account.to_string())
                .or_insert_with(|| {
                    Arc::new(Mutex::new(Held {
                        cache,
                        shared: Vec::new(),
                        readers: HashMap::new(),
                        last_round: 0,
                        last_seen: now,
                        status: BTreeMap::new(),
                    }))
                })
                .clone()
        };
        let Ok(mut h) = held.lock() else {
            return ExtCalendar::default();
        };
        h.last_seen = now;
        let rows: Vec<(String, String)> = parsed
            .iter()
            .map(|(a, _, v)| (a.email.clone(), v.clone()))
            .collect();
        let changed = h.shared != rows;
        if changed {
            // One no longer shared, or a calendar no longer shown, goes.
            let before = std::mem::take(&mut h.shared);
            for (email, _) in &before {
                match parsed.iter().find(|(a, _, _)| a.email == *email) {
                    None => {
                        let _ = h.cache.forget(email, None);
                        h.readers.remove(email.as_str());
                        h.status.remove(email.as_str());
                    }
                    Some((a, _, _)) => {
                        for c in a.calendars.iter().filter(|c| !c.shown) {
                            let _ = h.cache.forget(email, Some(&c.id));
                        }
                    }
                }
            }
            h.shared = rows;
        }
        if changed || now - h.last_round >= REFRESH_SECS {
            h.last_round = now;
            self.round(&mut h, &parsed, now);
        }
        let accounts: Vec<ExtAccount> = parsed.into_iter().map(|(a, _, _)| a).collect();
        let events = h
            .cache
            .between(from, to)
            .unwrap_or_default()
            .into_iter()
            .filter(|e| accounts::is_shown(&accounts, &e.account, &e.calendar))
            .collect();
        ExtCalendar {
            accounts,
            events,
            status: h.status.clone(),
        }
    }

    fn round(&self, h: &mut Held, parsed: &[(ExtAccount, String, String)], now: i64) {
        let Ok(maker) = self.maker.read() else {
            return;
        };
        for (a, token, value) in parsed {
            let fresh = h.readers.get(&a.email).is_none_or(|(t, _)| t != token);
            if fresh {
                match maker(&a.email, token, accounts::client_of_shared(value)) {
                    Some(api) => {
                        h.readers.insert(a.email.clone(), (token.clone(), api));
                    }
                    None => {
                        h.status.insert(
                            a.email.clone(),
                            "the server has no Google client to read it with".into(),
                        );
                        continue;
                    }
                }
            }
            let Some((_, api)) = h.readers.get_mut(&a.email) else {
                continue;
            };
            let mut failed = None;
            for c in a.shown() {
                if let Err(e) = sync_calendar(api.as_mut(), &mut h.cache, &a.email, &c.id, now) {
                    failed = Some(e.to_string());
                }
            }
            match failed {
                Some(e) => h.status.insert(a.email.clone(), e),
                None => h.status.remove(&a.email),
            };
        }
    }
}

impl ExtCalendar {
    /// The events as the calendar draws them, seen from `tz`.
    pub fn occurrences<Tz: TimeZone>(
        &self,
        from: NaiveDate,
        to: NaiveDate,
        tz: &Tz,
    ) -> Vec<Occurrence> {
        crate::extcal::occur::calendar(&self.events, &self.accounts, from, to, tz)
    }

    pub fn event_of(&self, o: &Occurrence) -> Option<&Cached> {
        crate::extcal::occur::event_of(&self.events, o)
    }
}

fn hhmm(m: u32) -> String {
    format!("{:02}:{:02}", m / 60 % 24, m % 60)
}

/// An event from another calendar as Claude sees it: one piece of it (a
/// day), with its links opening in its own account.
pub fn event_json(o: &Occurrence, c: &Cached, calendar: &str) -> Value {
    let e = &c.event;
    let link = |u: &Option<String>| {
        u.as_deref()
            .map(|u| crate::extcal::as_account(u, &c.account))
    };
    let mut v = json!({
        "kind": "external_event",
        "title": e.title,
        "calendar": calendar,
        "account": c.account,
        "on": o.date.to_string(),
        "start": o.start.map(hhmm),
        "end": o.end().map(hhmm),
        "all_day": o.start.is_none(),
        "tentative": e.response != Response::Accepted,
        "location": e.location,
        "video_call": link(&e.meet_link),
        "link": link(&e.html_link),
        "read_only": true,
    });
    if o.span > 1 {
        v["day_of_span"] = json!(format!("{}/{}", o.day + 1, o.span));
    }
    if let When::Timed { start, end } = &e.when {
        v["starts_at"] = json!(start.to_rfc3339());
        v["ends_at"] = json!(end.to_rfc3339());
    }
    v
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::extcal::sync::tests::{Fake, meeting};

    fn shared(email: &str, shown: bool) -> (String, String) {
        let mut a = ExtAccount::new(email);
        a.calendars.push(accounts::CalendarChoice {
            id: "primary".into(),
            name: "Work".into(),
            primary: true,
            shown,
            color: None,
        });
        (
            email.to_string(),
            accounts::shared_value(&a, "1//tok", None),
        )
    }

    #[test]
    fn reads_shared_calendars_and_asks_google_again_only_after_a_while() {
        let x = Externals::default();
        let calls = Arc::new(Mutex::new(0usize));
        let counter = calls.clone();
        x.set_maker(Box::new(move |email, token, _| {
            assert_eq!((email, token), ("me@work.com", "1//tok"));
            *counter.lock().unwrap() += 1;
            let mut f = Fake::default();
            f.put(meeting("standup", "2026-11-10T09:00"));
            Some(Box::new(f))
        }));
        let now = 1_794_000_000; // 2026-11-07
        let window = (now, now + 30 * 86_400);
        let rows = [shared("me@work.com", true)];
        let got = x.read("acct", &rows, window, now);
        assert_eq!(got.events.len(), 1);
        assert_eq!(got.events[0].event.title, "standup");
        // Soon after: from memory, Google isn't asked.
        x.read("acct", &rows, window, now + 10);
        assert_eq!(*calls.lock().unwrap(), 1);
        // A calendar no longer shown goes.
        let hidden = [shared("me@work.com", false)];
        assert!(x.read("acct", &hidden, window, now + 20).events.is_empty());
        // Stop sharing it: nothing kept.
        assert!(x.read("acct", &[], window, now + 30).events.is_empty());
        assert!(x.held.lock().unwrap().is_empty());
    }

    #[test]
    fn without_a_google_client_it_says_so() {
        let x = Externals::default();
        x.set_maker(Box::new(|_, _, _| None));
        let rows = [shared("me@work.com", true)];
        let got = x.read("acct", &rows, (0, 1), 1);
        assert!(got.events.is_empty());
        assert!(got.status["me@work.com"].contains("Google client"));
    }
}
