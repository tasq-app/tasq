//! Signing in to Google: OAuth 2.0 for native apps (RFC 8252) with PKCE.
//!
//! "Connect Google" opens the browser on Google's consent screen; the
//! answer comes back to a one-off listener on `127.0.0.1`, and the code it
//! carries is traded for tokens. The refresh token is kept in the system's
//! keychain (macOS Keychain, the Secret Service on Linux), or failing that
//! in a file only you can read; it never goes anywhere but Google.
//!
//! The app's client id and secret come with the build (`TASQ_GOOGLE_*`,
//! set by the release workflow), or from the config / environment for
//! your own Google Cloud project. A desktop app can't keep a secret, and
//! Google doesn't treat this one as one: what protects an account is that
//! its owner says yes in the browser.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail};
use serde_json::Value;

use super::http::{self, Request};

/// Access to the calendars tasq makes, and nothing else; plus who's
/// signed in, to show it.
pub const SCOPES: &str = "openid email https://www.googleapis.com/auth/calendar.app.created";

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const REVOKE_URL: &str = "https://oauth2.googleapis.com/revoke";

/// The app's OAuth client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Client {
    pub id: String,
    pub secret: String,
}

impl Client {
    /// The client to use: the environment (`TASQ_GOOGLE_CLIENT_ID` /
    /// `_SECRET`), else the config's, else the one built in.
    pub fn find(config_id: Option<&str>, config_secret: Option<&str>) -> Option<Self> {
        let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        let id = env("TASQ_GOOGLE_CLIENT_ID")
            .or_else(|| config_id.map(str::to_string))
            .or_else(|| option_env!("TASQ_GOOGLE_CLIENT_ID").map(str::to_string))
            .filter(|v| !v.is_empty())?;
        let secret = env("TASQ_GOOGLE_CLIENT_SECRET")
            .or_else(|| config_secret.map(str::to_string))
            .or_else(|| option_env!("TASQ_GOOGLE_CLIENT_SECRET").map(str::to_string))
            .unwrap_or_default();
        Some(Self { id, secret })
    }
}

/// What signing in leaves: the refresh token (kept) and who it's for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub email: String,
    pub refresh_token: String,
}

impl Account {
    fn to_json(&self) -> String {
        serde_json::json!({ "email": self.email, "refresh_token": self.refresh_token }).to_string()
    }

    fn from_json(s: &str) -> Option<Self> {
        let v: Value = serde_json::from_str(s).ok()?;
        Some(Self {
            email: v["email"].as_str()?.to_string(),
            refresh_token: v["refresh_token"].as_str()?.to_string(),
        })
    }
}

/// A short-lived access token.
#[derive(Debug, Clone)]
pub struct Access {
    pub token: String,
    pub until: Instant,
}

fn random(n: usize) -> String {
    let mut bytes = vec![0u8; n];
    let _ = getrandom::getrandom(&mut bytes);
    http::b64url(&bytes)
}

/// Open `url` in the browser.
fn open_browser(url: &str) -> Result<()> {
    let (cmd, args): (&str, Vec<&str>) = if cfg!(target_os = "macos") {
        ("open", vec![url])
    } else if cfg!(windows) {
        ("cmd", vec!["/C", "start", "", url])
    } else {
        ("xdg-open", vec![url])
    };
    std::process::Command::new(cmd)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .with_context(|| format!("couldn't open the browser ({cmd})"))?;
    Ok(())
}

/// The value of `key` in a query string.
fn query_value(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        (k == key).then(|| {
            // Percent-decoding: codes and states are URL-safe; `%2F` is the
            // one Google sends.
            let mut out = Vec::new();
            let b = v.as_bytes();
            let mut i = 0;
            while i < b.len() {
                if b[i] == b'%'
                    && i + 2 < b.len()
                    && let Ok(x) = u8::from_str_radix(&v[i + 1..i + 3], 16)
                {
                    out.push(x);
                    i += 3;
                } else {
                    out.push(if b[i] == b'+' { b' ' } else { b[i] });
                    i += 1;
                }
            }
            String::from_utf8_lossy(&out).into_owned()
        })
    })
}

/// The email in an ID token (its payload is plain base64url JSON; it came
/// straight from Google over TLS, so it's taken as it is).
fn email_of(id_token: &str) -> Option<String> {
    let payload = id_token.split('.').nth(1)?;
    let v: Value = serde_json::from_slice(&http::b64url_decode(payload)?).ok()?;
    v["email"].as_str().map(str::to_string)
}

/// Sign in: the browser, the consent, the code back on `127.0.0.1`, the
/// tokens. `on_url` is told the address in case the browser doesn't open.
pub fn sign_in(client: &Client, on_url: &dyn Fn(&str)) -> Result<(Account, Access)> {
    let server = tiny_http::Server::http("127.0.0.1:0")
        .map_err(|e| anyhow!("couldn't listen for Google's answer: {e}"))?;
    let port = server
        .server_addr()
        .to_ip()
        .map(|a| a.port())
        .ok_or_else(|| anyhow!("no port"))?;
    let redirect = format!("http://127.0.0.1:{port}");
    let verifier = random(48);
    let challenge = http::b64url(&http::sha256(verifier.as_bytes()));
    let state = random(16);
    let url = format!(
        "{AUTH_URL}?{}",
        http::form(&[
            ("client_id", &client.id),
            ("redirect_uri", &redirect),
            ("response_type", "code"),
            ("scope", SCOPES),
            ("code_challenge", &challenge),
            ("code_challenge_method", "S256"),
            ("access_type", "offline"),
            ("prompt", "consent"),
            ("state", &state),
        ])
    );
    on_url(&url);
    let _ = open_browser(&url);

    // Wait for the browser to come back (five minutes at most).
    let deadline = Instant::now() + Duration::from_secs(300);
    let code = loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            bail!("no answer from Google in five minutes");
        }
        let Some(req) = server.recv_timeout(left)? else {
            continue;
        };
        let query = req.url().split_once('?').map(|(_, q)| q.to_string());
        let Some(query) = query else {
            let _ = req.respond(tiny_http::Response::empty(404));
            continue;
        };
        let page = |msg: &str| {
            let html = format!(
                "<!doctype html><meta charset=utf-8><title>tasq</title>\
                 <body style=\"font:16px system-ui;margin:4em\"><h2>tasq</h2><p>{msg}</p>"
            );
            let mut r = tiny_http::Response::from_string(html);
            if let Ok(h) = tiny_http::Header::from_bytes("Content-Type", "text/html; charset=utf-8")
            {
                r = r.with_header(h);
            }
            r
        };
        if query_value(&query, "state").as_deref() != Some(state.as_str()) {
            let _ = req.respond(page("That answer wasn't for this request."));
            continue;
        }
        if let Some(err) = query_value(&query, "error") {
            let _ = req.respond(page("Not connected. You can close this tab."));
            bail!("Google said no: {err}");
        }
        let Some(code) = query_value(&query, "code") else {
            let _ = req.respond(tiny_http::Response::empty(400));
            continue;
        };
        let _ = req.respond(page(
            "Google Calendar is connected. You can close this tab and go back to tasq.",
        ));
        break code;
    };

    let v = token_request(&[
        ("client_id", &client.id),
        ("client_secret", &client.secret),
        ("code", &code),
        ("code_verifier", &verifier),
        ("grant_type", "authorization_code"),
        ("redirect_uri", &redirect),
    ])?;
    let refresh = v["refresh_token"]
        .as_str()
        .ok_or_else(|| anyhow!("Google gave no refresh token"))?
        .to_string();
    let email = v["id_token"]
        .as_str()
        .and_then(email_of)
        .unwrap_or_else(|| "Google account".to_string());
    Ok((
        Account {
            email,
            refresh_token: refresh,
        },
        access_from(&v)?,
    ))
}

fn access_from(v: &Value) -> Result<Access> {
    let token = v["access_token"]
        .as_str()
        .ok_or_else(|| anyhow!("no access token"))?
        .to_string();
    let secs = v["expires_in"].as_u64().unwrap_or(3600);
    // A minute early, so a token never runs out mid-request.
    Ok(Access {
        token,
        until: Instant::now() + Duration::from_secs(secs.saturating_sub(60)),
    })
}

fn token_request(pairs: &[(&str, &str)]) -> Result<Value> {
    let (code, body) = http::send(&Request {
        method: "POST",
        url: TOKEN_URL.to_string(),
        headers: vec!["Content-Type: application/x-www-form-urlencoded".to_string()],
        body: Some(http::form(pairs)),
    })?;
    let v: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
    if code != 200 {
        let why = v["error_description"]
            .as_str()
            .or_else(|| v["error"].as_str())
            .unwrap_or("unknown error");
        bail!("Google: {why}");
    }
    Ok(v)
}

/// A fresh access token for the kept account. An `invalid_grant` means the
/// access was revoked (or ran out): connect again.
pub fn refresh(client: &Client, account: &Account) -> Result<Access> {
    let v = token_request(&[
        ("client_id", &client.id),
        ("client_secret", &client.secret),
        ("refresh_token", &account.refresh_token),
        ("grant_type", "refresh_token"),
    ])?;
    access_from(&v)
}

/// Let Google know the access is given up (best effort).
pub fn revoke(account: &Account) {
    let _ = http::send(&Request {
        method: "POST",
        url: REVOKE_URL.to_string(),
        headers: vec!["Content-Type: application/x-www-form-urlencoded".to_string()],
        body: Some(http::form(&[("token", &account.refresh_token)])),
    });
}

// ---------------------------------------------------------------------------
// Keeping the account: the keychain, else a private file.
// ---------------------------------------------------------------------------

const SERVICE: &str = "tasq-google-calendar";

fn file_path() -> Option<std::path::PathBuf> {
    crate::xdg::config_home().map(|d| d.join("tasq").join("google.json"))
}

fn run(cmd: &str, args: &[&str], input: Option<&str>) -> Option<String> {
    let mut child = std::process::Command::new(cmd)
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    if let Some(i) = input {
        use std::io::Write;
        child.stdin.take()?.write_all(i.as_bytes()).ok()?;
    } else {
        drop(child.stdin.take());
    }
    let out = child.wait_with_output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Where the email (not a secret) is kept, and the whole account when
/// there's no keychain.
fn email_path() -> Option<std::path::PathBuf> {
    crate::xdg::config_home().map(|d| d.join("tasq").join("google-account"))
}

/// Whether a refresh token is safe to hand to `security -i` unquoted
/// (Google's are letters, digits and `-_./`).
fn plain(token: &str) -> bool {
    !token.is_empty()
        && token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/'))
}

fn write_private(path: &std::path::Path, text: &str) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, text)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// Keep `account` for next time: the refresh token in the keychain (macOS
/// Keychain, or the Secret Service on Linux), the email beside the config.
/// With no keychain, both in a file only you can read.
pub fn store(account: &Account) -> Result<()> {
    let token = &account.refresh_token;
    let in_keychain = if cfg!(target_os = "macos") {
        // `security -i` reads its command from stdin: the token never
        // shows in the process list.
        plain(token)
            && run(
                "security",
                &["-i"],
                Some(&format!(
                    "add-generic-password -U -a tasq -s {SERVICE} -w {token}\n"
                )),
            )
            .is_some()
    } else {
        run(
            "secret-tool",
            &[
                "store",
                "--label",
                "tasq Google Calendar",
                "service",
                SERVICE,
            ],
            Some(token),
        )
        .is_some()
    };
    let path = email_path().ok_or_else(|| anyhow!("no config directory"))?;
    if in_keychain {
        write_private(&path, &account.email)?;
        if let Some(p) = file_path() {
            let _ = std::fs::remove_file(p);
        }
        return Ok(());
    }
    let file = file_path().ok_or_else(|| anyhow!("no config directory"))?;
    write_private(&file, &account.to_json())?;
    write_private(&path, &account.email)
}

/// The kept account, if there is one.
pub fn load() -> Option<Account> {
    if let Some(a) = file_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| Account::from_json(&s))
    {
        return Some(a);
    }
    let email = email_path().and_then(|p| std::fs::read_to_string(p).ok())?;
    let token = if cfg!(target_os = "macos") {
        run(
            "security",
            &["find-generic-password", "-a", "tasq", "-s", SERVICE, "-w"],
            None,
        )
    } else {
        run("secret-tool", &["lookup", "service", SERVICE], None)
    }?;
    (!token.is_empty()).then(|| Account {
        email: email.trim().to_string(),
        refresh_token: token,
    })
}

/// The email of the kept account, without touching the keychain.
pub fn kept_email() -> Option<String> {
    email_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Forget the kept account, everywhere it may be.
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
    for p in [file_path(), email_path()].into_iter().flatten() {
        let _ = std::fs::remove_file(p);
    }
}

/// Seconds since the epoch, for "synced at" stamps.
pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pieces_of_signing_in() {
        assert_eq!(
            query_value("state=ab&code=4%2F0Ad-x&scope=x+y", "code").as_deref(),
            Some("4/0Ad-x")
        );
        assert_eq!(query_value("a=1", "b"), None);
        // header.payload.signature, the payload base64url JSON.
        let payload = http::b64url(br#"{"email":"jf@example.com"}"#);
        assert_eq!(
            email_of(&format!("x.{payload}.y")).as_deref(),
            Some("jf@example.com")
        );
        let a = Account {
            email: "jf@example.com".into(),
            refresh_token: "1//r".into(),
        };
        assert_eq!(Account::from_json(&a.to_json()), Some(a));
        // The config's client counts when the environment has none.
        if std::env::var("TASQ_GOOGLE_CLIENT_ID").is_err() {
            let c = Client::find(Some("id.apps"), Some("s")).unwrap_or(Client {
                id: String::new(),
                secret: String::new(),
            });
            assert_eq!(c.id, "id.apps");
        }
    }
}
