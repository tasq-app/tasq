//! Google Calendar, read-only, as [`EventsApi`]: the account's calendars
//! and their events. The access token is refreshed when it runs out, or
//! when Google turns one down.
//!
//! The scopes asked for are the narrowest that read: the list of
//! calendars, and their events. Nothing here can change a calendar.

use anyhow::{Result, anyhow, bail};
use serde_json::Value;

use super::sync::{CalendarInfo, EventsApi, Listed, Page};
use crate::gcal::auth::{self, Access, Account, Client};
use crate::gcal::http::{self, Request};

/// What connecting an account to read from asks Google for.
pub const SCOPES: &str = "openid email \
    https://www.googleapis.com/auth/calendar.calendarlist.readonly \
    https://www.googleapis.com/auth/calendar.events.readonly";

const API: &str = "https://www.googleapis.com/calendar/v3";

/// The kinds of event worth showing; "working location" is left out.
const EVENT_TYPES: &[&str] = &[
    "default",
    "fromGmail",
    "outOfOffice",
    "focusTime",
    "birthday",
];

pub struct GoogleEvents {
    client: Client,
    account: Account,
    access: Option<Access>,
}

impl GoogleEvents {
    pub fn new(client: Client, account: Account, access: Option<Access>) -> Self {
        Self {
            client,
            account,
            access,
        }
    }

    fn token(&mut self) -> Result<String> {
        match &self.access {
            Some(a) if a.until > std::time::Instant::now() => Ok(a.token.clone()),
            _ => {
                let a = auth::refresh(&self.client, &self.account)?;
                let t = a.token.clone();
                self.access = Some(a);
                Ok(t)
            }
        }
    }

    fn get(&mut self, path_and_query: &str) -> Result<(u16, Value)> {
        for attempt in 0..2 {
            let token = self.token()?;
            let (code, text) = http::send(&Request {
                method: "GET",
                url: format!("{API}{path_and_query}"),
                headers: vec![format!("Authorization: Bearer {token}")],
                body: None,
            })?;
            if code == 401 && attempt == 0 {
                self.access = None;
                continue;
            }
            return Ok((code, serde_json::from_str(&text).unwrap_or(Value::Null)));
        }
        bail!("Google turned the access down: connect again")
    }
}

fn fail(code: u16, v: &Value) -> anyhow::Error {
    let msg = v["error"]["message"].as_str().unwrap_or("unknown error");
    anyhow!("Google ({code}): {msg}")
}

/// The query for one page of `events.list`. With a sync token Google
/// takes no `timeMin`: the token already carries the round's filters.
pub fn events_query(sync_token: Option<&str>, time_min: &str, page_token: Option<&str>) -> String {
    let mut q = vec![
        ("singleEvents".to_string(), "true".to_string()),
        ("maxResults".to_string(), "250".to_string()),
    ];
    match sync_token {
        Some(t) => q.push(("syncToken".into(), t.into())),
        None => q.push(("timeMin".into(), time_min.into())),
    }
    for t in EVENT_TYPES {
        q.push(("eventTypes".into(), (*t).into()));
    }
    if let Some(p) = page_token {
        q.push(("pageToken".into(), p.into()));
    }
    q.iter()
        .map(|(k, v)| format!("{}={}", http::percent(k), http::percent(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// Read one page of `calendarList.list`.
pub fn calendars_from(v: &Value) -> Vec<CalendarInfo> {
    v["items"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter(|c| c["deleted"] != true)
                .filter_map(|c| {
                    Some(CalendarInfo {
                        id: c["id"].as_str()?.to_string(),
                        name: c["summaryOverride"]
                            .as_str()
                            .or(c["summary"].as_str())
                            .unwrap_or_default()
                            .to_string(),
                        primary: c["primary"] == true,
                        color: c["backgroundColor"].as_str().map(str::to_string),
                    })
                })
                // tasq's own calendar holds your tasks: they're here already.
                .filter(|c| c.name != crate::gcal::sync::CALENDAR_NAME || c.primary)
                .collect()
        })
        .unwrap_or_default()
}

impl EventsApi for GoogleEvents {
    fn calendars(&mut self) -> Result<Vec<CalendarInfo>> {
        let mut out = Vec::new();
        let mut page: Option<String> = None;
        loop {
            let q = match &page {
                Some(p) => format!("?pageToken={}", http::percent(p)),
                None => String::new(),
            };
            let (code, v) = self.get(&format!("/users/me/calendarList{q}"))?;
            if code != 200 {
                return Err(fail(code, &v));
            }
            out.extend(calendars_from(&v));
            match v["nextPageToken"].as_str() {
                Some(p) => page = Some(p.to_string()),
                None => return Ok(out),
            }
        }
    }

    fn events(
        &mut self,
        calendar: &str,
        sync_token: Option<&str>,
        time_min: &str,
        page_token: Option<&str>,
    ) -> Result<Listed> {
        let path = format!(
            "/calendars/{}/events?{}",
            http::percent(calendar),
            events_query(sync_token, time_min, page_token)
        );
        let (code, v) = self.get(&path)?;
        match code {
            200 => Ok(Listed::Page(Page {
                items: v["items"].as_array().cloned().unwrap_or_default(),
                next_page: v["nextPageToken"].as_str().map(str::to_string),
                next_sync: v["nextSyncToken"].as_str().map(str::to_string),
            })),
            410 => Ok(Listed::Gone),
            _ => Err(fail(code, &v)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_query_for_a_full_and_an_incremental_round() {
        let full = events_query(None, "2026-10-01T00:00:00Z", None);
        assert!(
            full.starts_with("singleEvents=true&maxResults=250&timeMin=2026-10-01T00%3A00%3A00Z")
        );
        assert!(full.contains("eventTypes=fromGmail"));
        assert!(!full.contains("workingLocation"));
        let inc = events_query(Some("tok/=="), "ignored", Some("p2"));
        assert!(inc.contains("syncToken=tok%2F%3D%3D"));
        assert!(!inc.contains("timeMin"));
        assert!(inc.ends_with("pageToken=p2"));
    }

    #[test]
    fn the_calendar_list_without_tasqs_own() {
        let v = json!({ "items": [
            { "id": "me@x", "summary": "me@x", "primary": true, "backgroundColor": "#9fe1e7" },
            { "id": "t@group", "summary": "tasq" },
            { "id": "h@group", "summary": "Holidays", "summaryOverride": "Festivos" },
            { "id": "d@group", "summary": "Old", "deleted": true }
        ]});
        let c = calendars_from(&v);
        assert_eq!(c.len(), 2);
        assert!(c[0].primary);
        assert_eq!(c[0].color.as_deref(), Some("#9fe1e7"));
        assert_eq!(c[1].name, "Festivos");
    }
}
