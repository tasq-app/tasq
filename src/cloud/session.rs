//! The account this device is signed in to: the server, the email, and
//! the keys made from the password (never the password). The keys go in
//! the system keychain (macOS Keychain, or the Secret Service through
//! `secret-tool`); the server and email beside the config. With no
//! keychain, all three in a file only you can read.

use std::path::PathBuf;

use anyhow::{Result, anyhow};
use serde_json::{Value, json};

use super::keys::Keys;
use crate::gcal::auth::{plain, run, write_private};

/// The server release builds sync through, unless told another.
pub const DEFAULT_SERVER: &str = match option_env!("TASQ_SYNC_URL") {
    Some(url) => url,
    None => "https://tasq-sync.82.70.89.188.sslip.io",
};

const SERVICE: &str = "tasq-sync";

/// The server to use: `TASQ_SYNC_URL`, else `sync_url` from the config,
/// else [`DEFAULT_SERVER`].
pub fn server(config: Option<&str>) -> String {
    std::env::var("TASQ_SYNC_URL")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| config.map(str::to_string))
        .unwrap_or_else(|| DEFAULT_SERVER.to_string())
}

#[derive(Debug, Clone)]
pub struct Session {
    pub server: String,
    pub email: String,
    pub keys: Keys,
}

fn path() -> Option<PathBuf> {
    crate::xdg::config_home().map(|d| d.join("tasq").join("sync.json"))
}

fn keychain_get() -> Option<String> {
    if cfg!(target_os = "macos") {
        run(
            "security",
            &["find-generic-password", "-a", "tasq", "-s", SERVICE, "-w"],
            None,
        )
    } else {
        run("secret-tool", &["lookup", "service", SERVICE], None)
    }
    .filter(|s| !s.is_empty())
}

fn keychain_put(secret: &str) -> bool {
    if cfg!(target_os = "macos") {
        plain(secret)
            && run(
                "security",
                &["-i"],
                Some(&format!(
                    "add-generic-password -U -a tasq -s {SERVICE} -w {secret}\n"
                )),
            )
            .is_some()
    } else {
        run(
            "secret-tool",
            &["store", "--label", "tasq sync", "service", SERVICE],
            Some(secret),
        )
        .is_some()
    }
}

impl Session {
    /// Who this device is signed in as, without touching the keychain.
    pub fn signed_in_as() -> Option<(String, String)> {
        let v: Value = serde_json::from_str(&std::fs::read_to_string(path()?).ok()?).ok()?;
        Some((
            v["server"].as_str()?.to_string(),
            v["email"].as_str()?.to_string(),
        ))
    }

    /// The whole session, keys and all.
    pub fn load() -> Option<Self> {
        let v: Value = serde_json::from_str(&std::fs::read_to_string(path()?).ok()?).ok()?;
        let secret = match v["keys"].as_str() {
            Some(s) => s.to_string(),
            None => keychain_get()?,
        };
        Some(Self {
            server: v["server"].as_str()?.to_string(),
            email: v["email"].as_str()?.to_string(),
            keys: Keys::from_secret(&secret)?,
        })
    }

    /// Keep it: the keys in the keychain when there is one.
    pub fn save(&self) -> Result<()> {
        let path = path().ok_or_else(|| anyhow!("no config directory"))?;
        let secret = self.keys.to_secret();
        let mut body = json!({ "server": self.server, "email": self.email });
        if !keychain_put(&secret) {
            body["keys"] = json!(secret);
        }
        write_private(&path, &body.to_string())
    }

    /// Forget it, keychain and file.
    pub fn forget() {
        if cfg!(target_os = "macos") {
            let _ = run(
                "security",
                &["delete-generic-password", "-a", "tasq", "-s", SERVICE],
                None,
            );
        } else {
            let _ = run("secret-tool", &["clear", "service", SERVICE], None);
        }
        if let Some(p) = path() {
            let _ = std::fs::remove_file(p);
        }
    }
}
