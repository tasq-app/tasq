//! The accounts you connected to read from, and which of their calendars
//! you show.
//!
//! Two halves, kept apart: what's no secret (the email, the calendars and
//! your choices) in `external-calendars.json` next to the config, and each
//! account's refresh token in the system keychain — macOS Keychain, the
//! Secret Service on Linux — or, with no keychain, in a file only you can
//! read.

use std::path::PathBuf;

use anyhow::{Result, anyhow};
use serde_json::{Value, json};

use super::sync::CalendarInfo;
use crate::gcal::auth::{plain, run, write_private};

/// One of an account's calendars, and whether you show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarChoice {
    pub id: String,
    pub name: String,
    pub primary: bool,
    pub shown: bool,
    /// The colour you chose (a palette slot or `#rrggbb`); `None` is
    /// automatic.
    pub color: Option<String>,
}

/// A connected account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtAccount {
    pub email: String,
    pub calendars: Vec<CalendarChoice>,
}

impl ExtAccount {
    pub fn new(email: &str) -> Self {
        Self {
            email: email.to_string(),
            calendars: Vec::new(),
        }
    }

    /// Take in the account's calendar list as Google has it now: names
    /// follow Google, your choices stay, a calendar gone from Google goes,
    /// and a new one comes in shown only if it's the main one.
    pub fn refresh_calendars(&mut self, list: &[CalendarInfo]) {
        let old = std::mem::take(&mut self.calendars);
        self.calendars = list
            .iter()
            .map(|c| CalendarChoice {
                id: c.id.clone(),
                name: c.name.clone(),
                primary: c.primary,
                shown: old
                    .iter()
                    .find(|o| o.id == c.id)
                    .map_or(c.primary, |o| o.shown),
                color: old
                    .iter()
                    .find(|o| o.id == c.id)
                    .and_then(|o| o.color.clone()),
            })
            .collect();
        // The main calendar first, then by name.
        self.calendars.sort_by(|a, b| {
            (!a.primary, a.name.to_lowercase()).cmp(&(!b.primary, b.name.to_lowercase()))
        });
    }

    /// The calendars you show.
    pub fn shown(&self) -> impl Iterator<Item = &CalendarChoice> {
        self.calendars.iter().filter(|c| c.shown)
    }

    fn to_json(&self) -> Value {
        json!({
            "email": self.email,
            "calendars": self.calendars.iter().map(|c| json!({
                "id": c.id, "name": c.name, "primary": c.primary, "shown": c.shown,
                "color": c.color,
            })).collect::<Vec<_>>(),
        })
    }

    fn from_json(v: &Value) -> Option<Self> {
        Some(Self {
            email: v["email"].as_str()?.to_string(),
            calendars: v["calendars"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|c| {
                            Some(CalendarChoice {
                                id: c["id"].as_str()?.to_string(),
                                name: c["name"].as_str().unwrap_or_default().to_string(),
                                primary: c["primary"] == true,
                                shown: c["shown"] == true,
                                color: c["color"].as_str().map(str::to_string),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default(),
        })
    }
}

/// The accounts as text, for the file.
pub fn to_json(accounts: &[ExtAccount]) -> String {
    let v = json!({ "accounts": accounts.iter().map(ExtAccount::to_json).collect::<Vec<_>>() });
    serde_json::to_string_pretty(&v).unwrap_or_default()
}

pub fn from_json(s: &str) -> Vec<ExtAccount> {
    serde_json::from_str::<Value>(s)
        .ok()
        .and_then(|v| {
            v["accounts"]
                .as_array()
                .map(|a| a.iter().filter_map(ExtAccount::from_json).collect())
        })
        .unwrap_or_default()
}

fn accounts_path() -> Option<PathBuf> {
    crate::xdg::config_home().map(|d| d.join("tasq").join("external-calendars.json"))
}

/// The kept accounts.
pub fn load() -> Vec<ExtAccount> {
    accounts_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|s| from_json(&s))
        .unwrap_or_default()
}

pub fn save(accounts: &[ExtAccount]) -> Result<()> {
    let p = accounts_path().ok_or_else(|| anyhow!("no config directory"))?;
    write_private(&p, &to_json(accounts))
}

// ---------------------------------------------------------------------------
// The tokens: the keychain, else a private file.
// ---------------------------------------------------------------------------

const SERVICE: &str = "tasq-external-calendar";

fn tokens_path() -> Option<PathBuf> {
    crate::xdg::config_home().map(|d| d.join("tasq").join("external-tokens.json"))
}

fn file_tokens() -> serde_json::Map<String, Value> {
    tokens_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

fn write_file_tokens(map: &serde_json::Map<String, Value>) -> Result<()> {
    let p = tokens_path().ok_or_else(|| anyhow!("no config directory"))?;
    if map.is_empty() {
        let _ = std::fs::remove_file(p);
        return Ok(());
    }
    write_private(&p, &Value::Object(map.clone()).to_string())
}

/// Keep `email`'s refresh token.
pub fn store_token(email: &str, token: &str) -> Result<()> {
    let in_keychain = if cfg!(target_os = "macos") {
        // `security -i` reads its command from stdin: the token never
        // shows in the process list. The email is quoted for it.
        plain(token)
            && !email.contains('"')
            && run(
                "security",
                &["-i"],
                Some(&format!(
                    "add-generic-password -U -a \"{email}\" -s {SERVICE} -w {token}\n"
                )),
            )
            .is_some()
    } else {
        run(
            "secret-tool",
            &[
                "store",
                "--label",
                &format!("tasq calendar · {email}"),
                "service",
                SERVICE,
                "account",
                email,
            ],
            Some(token),
        )
        .is_some()
    };
    let mut map = file_tokens();
    if in_keychain {
        if map.remove(email).is_some() {
            write_file_tokens(&map)?;
        }
        return Ok(());
    }
    map.insert(email.to_string(), Value::String(token.to_string()));
    write_file_tokens(&map)
}

/// `email`'s refresh token, if kept.
pub fn load_token(email: &str) -> Option<String> {
    if let Some(t) = file_tokens().get(email).and_then(Value::as_str) {
        return Some(t.to_string());
    }
    let t = if cfg!(target_os = "macos") {
        run(
            "security",
            &["find-generic-password", "-a", email, "-s", SERVICE, "-w"],
            None,
        )
    } else {
        run(
            "secret-tool",
            &["lookup", "service", SERVICE, "account", email],
            None,
        )
    }?;
    (!t.is_empty()).then_some(t)
}

/// Forget `email`'s token, everywhere it may be.
pub fn forget_token(email: &str) {
    if cfg!(target_os = "macos") {
        let _ = run(
            "security",
            &["delete-generic-password", "-a", email, "-s", SERVICE],
            None,
        );
    } else {
        let _ = run(
            "secret-tool",
            &["clear", "service", SERVICE, "account", email],
            None,
        );
    }
    let mut map = file_tokens();
    if map.remove(email).is_some() {
        let _ = write_file_tokens(&map);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn info(id: &str, name: &str, primary: bool) -> CalendarInfo {
        CalendarInfo {
            id: id.into(),
            name: name.into(),
            primary,
            color: None,
        }
    }

    #[test]
    fn choices_survive_a_new_calendar_list() {
        let mut a = ExtAccount::new("me@work.com");
        a.refresh_calendars(&[info("h", "Holidays", false), info("me", "Me", true)]);
        assert_eq!(a.calendars[0].id, "me", "the main one first");
        assert!(a.calendars[0].shown);
        assert!(!a.calendars[1].shown, "others start hidden");
        a.calendars[1].shown = true;
        a.calendars[0].shown = false;
        // Google renames one, drops none, adds one.
        a.refresh_calendars(&[
            info("me", "Me", true),
            info("h", "Festivos", false),
            info("t", "Team", false),
        ]);
        let by = |id: &str| a.calendars.iter().find(|c| c.id == id).unwrap().clone();
        assert!(!by("me").shown);
        assert!(by("h").shown);
        assert_eq!(by("h").name, "Festivos");
        assert!(!by("t").shown);
        a.refresh_calendars(&[info("me", "Me", true)]);
        assert_eq!(a.calendars.len(), 1);
    }

    #[test]
    fn the_file_round_trips() {
        let mut a = ExtAccount::new("me@x");
        a.refresh_calendars(&[info("me", "Me", true)]);
        let b = ExtAccount::new("work@y");
        let all = vec![a, b];
        assert_eq!(from_json(&to_json(&all)), all);
        assert!(from_json("not json").is_empty());
    }
}
