//! Talking to a sync server: [`Remote`], over HTTPS with the system's curl
//! ([`Http`]), or straight to a [`Relay`] in the same process (tests).

use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};

use super::keys::Keys;
use super::relay::Relay;
use crate::gcal::http::{self, Request};

/// One sealed batch the server holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub seq: i64,
    pub device: String,
    pub blob: String,
}

/// What a sync round needs from the server.
pub trait Remote {
    /// Make the account (fine if it's already there with these keys).
    fn signup(&mut self) -> Result<()>;
    /// Check the keys are right.
    fn check(&mut self) -> Result<()>;
    /// Store a sealed batch; its place in the account's log.
    fn push(&mut self, device: &str, blob: &str) -> Result<i64>;
    /// The batches after `after`, oldest first, and whether there are more.
    fn pull(&mut self, after: i64) -> Result<(Vec<Item>, bool)>;
}

/// Read a server's answer: the JSON on success, its message otherwise.
fn answer(code: u16, v: &Value) -> Result<()> {
    if (200..300).contains(&code) {
        return Ok(());
    }
    let msg = v["error"].as_str().unwrap_or("unexpected answer");
    bail!("sync server ({code}): {msg}")
}

fn items(v: &Value) -> Result<(Vec<Item>, bool)> {
    let list = v["items"]
        .as_array()
        .ok_or_else(|| anyhow!("sync server: no items"))?;
    let items = list
        .iter()
        .map(|i| {
            Some(Item {
                seq: i["seq"].as_i64()?,
                device: i["device"].as_str()?.to_string(),
                blob: i["blob"].as_str()?.to_string(),
            })
        })
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| anyhow!("sync server: a bad item"))?;
    Ok((items, v["more"].as_bool().unwrap_or(false)))
}

fn signup_body(keys: &Keys) -> String {
    let bearer = keys.bearer();
    let (account, auth) = bearer.split_once('.').unwrap_or(("", ""));
    json!({ "account": account, "auth": auth }).to_string()
}

/// A server over HTTP(S), e.g. `https://sync.example.com`.
pub struct Http {
    base: String,
    keys: Keys,
}

impl Http {
    pub fn new(base: &str, keys: Keys) -> Self {
        Self {
            base: base.trim_end_matches('/').to_string(),
            keys,
        }
    }

    fn call(&self, method: &str, path: &str, body: Option<String>) -> Result<(u16, Value)> {
        let mut headers = vec![format!("Authorization: Bearer {}", self.keys.bearer())];
        if body.is_some() {
            headers.push("Content-Type: application/json".to_string());
        }
        let (code, text) = http::send(&Request {
            method,
            url: format!("{}{path}", self.base),
            headers,
            body,
        })?;
        Ok((code, serde_json::from_str(&text).unwrap_or(Value::Null)))
    }
}

/// An assistant (Claude) connected to the account, as the server lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assistant {
    pub id: String,
    pub name: String,
    /// It may change tasks, not only read them.
    pub write: bool,
}

impl Http {
    /// The assistants connected to this account.
    pub fn assistants(&self) -> Result<Vec<Assistant>> {
        let (code, v) = self.call("GET", "/v1/assistants", None)?;
        answer(code, &v)?;
        Ok(v["assistants"]
            .as_array()
            .map(|list| {
                list.iter()
                    .filter_map(|a| {
                        Some(Assistant {
                            id: a["id"].as_str()?.to_string(),
                            name: a["name"].as_str()?.to_string(),
                            write: a["write"].as_bool().unwrap_or(false),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Disconnect every assistant: the server forgets their keys.
    pub fn revoke_assistants(&self) -> Result<()> {
        let (code, v) = self.call(
            "POST",
            "/v1/assistants/revoke",
            Some(json!({ "all": true }).to_string()),
        )?;
        answer(code, &v)
    }
}

impl Remote for Http {
    fn signup(&mut self) -> Result<()> {
        let (code, v) = self.call("POST", "/v1/signup", Some(signup_body(&self.keys)))?;
        answer(code, &v)
    }

    fn check(&mut self) -> Result<()> {
        let (code, v) = self.call("GET", "/v1/check", None)?;
        answer(code, &v)
    }

    fn push(&mut self, device: &str, blob: &str) -> Result<i64> {
        let body = json!({ "device": device, "blob": blob }).to_string();
        let (code, v) = self.call("POST", "/v1/push", Some(body))?;
        answer(code, &v)?;
        v["seq"]
            .as_i64()
            .ok_or_else(|| anyhow!("sync server: no seq"))
    }

    fn pull(&mut self, after: i64) -> Result<(Vec<Item>, bool)> {
        let (code, v) = self.call("GET", &format!("/v1/pull?after={after}"), None)?;
        answer(code, &v)?;
        items(&v)
    }
}

/// A [`Relay`] in the same process, for tests.
pub struct Local<'a> {
    pub relay: &'a Relay,
    pub keys: Keys,
}

impl Local<'_> {
    fn call(&self, method: &str, path: &str, body: &str) -> (u16, Value) {
        let auth = format!("Bearer {}", self.keys.bearer());
        self.relay
            .handle(method, path, Some(&auth), body.as_bytes())
    }
}

impl Remote for Local<'_> {
    fn signup(&mut self) -> Result<()> {
        let (code, v) = self.call("POST", "/v1/signup", &signup_body(&self.keys));
        answer(code, &v)
    }

    fn check(&mut self) -> Result<()> {
        let (code, v) = self.call("GET", "/v1/check", "");
        answer(code, &v)
    }

    fn push(&mut self, device: &str, blob: &str) -> Result<i64> {
        let body = json!({ "device": device, "blob": blob }).to_string();
        let (code, v) = self.call("POST", "/v1/push", &body);
        answer(code, &v)?;
        v["seq"].as_i64().ok_or_else(|| anyhow!("no seq"))
    }

    fn pull(&mut self, after: i64) -> Result<(Vec<Item>, bool)> {
        let (code, v) = self.call("GET", &format!("/v1/pull?after={after}"), "");
        answer(code, &v)?;
        items(&v)
    }
}
