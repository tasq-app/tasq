//! The Google Calendar API (v3), as [`CalendarApi`]: the calendar tasq
//! makes and the events in it. The access token is refreshed when it runs
//! out, or when Google turns one down.

use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};

use super::auth::{self, Access, Account, Client};
use super::http::{self, Request};
use super::sync::CalendarApi;

const API: &str = "https://www.googleapis.com/calendar/v3";

pub struct Google {
    client: Client,
    account: Account,
    access: Option<Access>,
}

impl Google {
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

    /// A call to the API, refreshing the token once if Google says it's no
    /// good any more.
    fn call(&mut self, method: &str, path: &str, body: Option<&Value>) -> Result<(u16, Value)> {
        for attempt in 0..2 {
            let token = self.token()?;
            let mut headers = vec![format!("Authorization: Bearer {token}")];
            if body.is_some() {
                headers.push("Content-Type: application/json".to_string());
            }
            let (code, text) = http::send(&Request {
                method,
                url: format!("{API}{path}"),
                headers,
                body: body.map(Value::to_string),
            })?;
            if code == 401 && attempt == 0 {
                self.access = None;
                continue;
            }
            let v = serde_json::from_str(&text).unwrap_or(Value::Null);
            return Ok((code, v));
        }
        bail!("Google turned the access down: connect again")
    }

    fn fail(code: u16, v: &Value) -> anyhow::Error {
        let msg = v["error"]["message"].as_str().unwrap_or("unknown error");
        anyhow!("Google ({code}): {msg}")
    }
}

fn events_path(calendar: &str) -> String {
    format!("/calendars/{}/events", http::percent(calendar))
}

impl CalendarApi for Google {
    fn create_calendar(&mut self, name: &str, time_zone: &str) -> Result<String> {
        let (code, v) = self.call(
            "POST",
            "/calendars",
            Some(&json!({ "summary": name, "timeZone": time_zone })),
        )?;
        if code != 200 {
            return Err(Self::fail(code, &v));
        }
        v["id"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| anyhow!("Google gave no calendar id"))
    }

    fn calendar_exists(&mut self, id: &str) -> Result<bool> {
        let (code, v) = self.call("GET", &format!("/calendars/{}", http::percent(id)), None)?;
        match code {
            200 => Ok(true),
            404 | 410 => Ok(false),
            _ => Err(Self::fail(code, &v)),
        }
    }

    fn insert(&mut self, calendar: &str, body: &Value) -> Result<String> {
        let (code, v) = self.call("POST", &events_path(calendar), Some(body))?;
        if code != 200 {
            return Err(Self::fail(code, &v));
        }
        v["id"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| anyhow!("Google gave no event id"))
    }

    fn update(&mut self, calendar: &str, id: &str, body: &Value) -> Result<bool> {
        let path = format!("{}/{}", events_path(calendar), http::percent(id));
        let (code, v) = self.call("PUT", &path, Some(body))?;
        match code {
            200 => Ok(true),
            404 | 410 => Ok(false),
            _ => Err(Self::fail(code, &v)),
        }
    }

    fn delete(&mut self, calendar: &str, id: &str) -> Result<()> {
        let path = format!("{}/{}", events_path(calendar), http::percent(id));
        let (code, v) = self.call("DELETE", &path, None)?;
        match code {
            200 | 204 | 404 | 410 => Ok(()),
            _ => Err(Self::fail(code, &v)),
        }
    }
}
