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
use crate::core::spaces::{AUTO_SLOTS, PALETTE_SLOTS, SpaceColor};
use crate::gcal::auth::{Client, plain, run, write_private};

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
    /// Shown on all your devices (its access travels with sync), not
    /// only on this one.
    pub shared: bool,
}

impl ExtAccount {
    pub fn new(email: &str) -> Self {
        Self {
            email: email.to_string(),
            calendars: Vec::new(),
            shared: false,
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

    fn calendars_json(&self) -> Value {
        json!(
            self.calendars
                .iter()
                .map(|c| json!({
                    "id": c.id, "name": c.name, "primary": c.primary, "shown": c.shown,
                    "color": c.color,
                }))
                .collect::<Vec<_>>()
        )
    }

    fn to_json(&self) -> Value {
        json!({
            "email": self.email,
            "shared": self.shared,
            "calendars": self.calendars.iter().map(|c| json!({
                "id": c.id, "name": c.name, "primary": c.primary, "shown": c.shown,
                "color": c.color,
            })).collect::<Vec<_>>(),
        })
    }

    fn from_json(v: &Value) -> Option<Self> {
        Some(Self {
            email: v["email"].as_str()?.to_string(),
            shared: v["shared"] == true,
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

/// What sync carries of a shared account: its calendar choices and its
/// access, and the app's Google client the access was given to (so tasq's
/// server can read the account for the web calendar and Claude, when you
/// turn that on, without being set up for it). Keys sorted, so the same
/// account always reads the same.
pub fn shared_value(a: &ExtAccount, token: &str, client: Option<&Client>) -> String {
    let mut v = json!({ "calendars": a.calendars_json(), "token": token });
    if let Some(c) = client {
        v["client"] = json!({ "id": c.id, "secret": c.secret });
    }
    v.to_string()
}

/// The Google client a shared account's access belongs to, if the device
/// that shared it said.
pub fn client_of_shared(value: &str) -> Option<Client> {
    let v: Value = serde_json::from_str(value).ok()?;
    let id = v["client"]["id"].as_str().filter(|s| !s.is_empty())?;
    Some(Client {
        id: id.to_string(),
        secret: v["client"]["secret"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
    })
}

/// The colour a calendar is painted in: the one you chose, else one of
/// the palette's second eight by its place among all the calendars (the
/// spaces take the first eight).
pub fn color_of(accounts: &[ExtAccount], account: &str, calendar: &str) -> SpaceColor {
    let chosen = accounts
        .iter()
        .find(|a| a.email == account)
        .and_then(|a| a.calendars.iter().find(|c| c.id == calendar))
        .and_then(|c| c.color.as_deref())
        .and_then(SpaceColor::parse);
    if let Some(c) = chosen {
        return c;
    }
    let k = accounts
        .iter()
        .flat_map(|a| {
            a.calendars
                .iter()
                .map(move |c| (a.email.as_str(), c.id.as_str()))
        })
        .position(|x| x == (account, calendar))
        .unwrap_or(0);
    SpaceColor::Slot((AUTO_SLOTS + k) % PALETTE_SLOTS)
}

/// A calendar's name, as its account lists it.
pub fn calendar_name(accounts: &[ExtAccount], account: &str, calendar: &str) -> String {
    accounts
        .iter()
        .find(|a| a.email == account)
        .and_then(|a| a.calendars.iter().find(|c| c.id == calendar))
        .map(|c| c.name.clone())
        .unwrap_or_default()
}

/// Whether the calendar is one you show.
pub fn is_shown(accounts: &[ExtAccount], account: &str, calendar: &str) -> bool {
    accounts
        .iter()
        .any(|a| a.email == account && a.calendars.iter().any(|c| c.id == calendar && c.shown))
}

/// A shared account back from sync: the account and its token.
pub fn from_shared(email: &str, value: &str) -> Option<(ExtAccount, String)> {
    let v: Value = serde_json::from_str(value).ok()?;
    let token = v["token"].as_str().filter(|t| !t.is_empty())?.to_string();
    let mut a = ExtAccount::from_json(&json!({ "email": email, "calendars": v["calendars"] }))?;
    a.shared = true;
    Some((a, token))
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
        // What sync carries, and back.
        let mut a = all[0].clone();
        a.calendars[0].color = Some("#8aadf4".into());
        let v = shared_value(&a, "1//tok", None);
        let (back, token) = from_shared("me@x", &v).unwrap();
        assert_eq!(token, "1//tok");
        assert!(back.shared);
        assert_eq!(back.calendars, a.calendars);
        assert_eq!(
            shared_value(&back, "1//tok", None),
            v,
            "the same text every time"
        );
        assert!(client_of_shared(&v).is_none());
        let client = Client {
            id: "app.apps.googleusercontent.com".into(),
            secret: "s".into(),
        };
        let with = shared_value(&back, "1//tok", Some(&client));
        assert_eq!(client_of_shared(&with), Some(client));
        assert_eq!(from_shared("me@x", &with).unwrap().1, "1//tok");
        assert!(
            from_shared("me@x", r#"{"calendars":[]}"#).is_none(),
            "no token, no account"
        );
    }
}
