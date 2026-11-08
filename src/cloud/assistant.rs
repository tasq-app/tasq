//! Claude (or another assistant) on your tasks, from any device: a remote
//! [MCP](https://modelcontextprotocol.io) server inside `tasq-relay`, with
//! OAuth 2.1 sign-in.
//!
//! This is the one place the server reads an account, and only once its
//! owner asks for it: on the sign-in page they type their tasq email and
//! password, the server makes the keys from them, and seals the keys under
//! the token it hands the assistant. The server keeps only the sealed keys
//! and the token's hash, so what it stores can't be read without a token
//! the assistant shows; while answering one, it reads the account (by
//! replaying its changes into a database in memory) and writes back like
//! one more computer. Disconnecting (tasq → Settings → Sync, or the
//! assistant's own settings) forgets the keys.
//!
//! The routes:
//!
//! - `GET /.well-known/oauth-protected-resource`,
//!   `GET /.well-known/oauth-authorization-server`: where to sign in.
//! - `POST /oauth/register`: dynamic client registration (RFC 7591).
//! - `GET`/`POST /oauth/authorize`: the sign-in page.
//! - `POST /oauth/token`: a code (PKCE S256) or a refresh token for tokens.
//! - `POST /mcp`: MCP over streamable HTTP (JSON answers).
//! - With a device's own bearer: `GET /v1/assistants` (who's connected),
//!   `POST /v1/assistants/revoke` `{"id"}` or `{"all": true}`.

use std::path::PathBuf;

use anyhow::{Result, anyhow};
use chrono::{Duration, NaiveDate, Timelike};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};

use super::client::Local;
use super::keys::{self, Keys};
use super::relay::{Incoming, Outgoing, Relay, device_account, error_reply, now};
use super::run::sync_once;
use crate::core::calendar::{self, Occurrence};
use crate::core::db::{Db, title_of};
use crate::core::{AddOutcome, CompleteOutcome, EditOutcome, Store, series};
use crate::gcal::http;
use crate::todo::{self, Task};

pub const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS oauth_clients (
    id            TEXT PRIMARY KEY,
    name          TEXT NOT NULL,
    redirect_uris TEXT NOT NULL,
    created_at    TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS oauth_codes (
    hash       TEXT PRIMARY KEY,
    client     TEXT NOT NULL,
    redirect   TEXT NOT NULL,
    challenge  TEXT NOT NULL,
    scope      TEXT NOT NULL,
    tz         TEXT NOT NULL,
    account    TEXT NOT NULL,
    wrapped    TEXT NOT NULL,
    expires    INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS assistants (
    id              TEXT PRIMARY KEY,
    account         TEXT NOT NULL,
    client          TEXT NOT NULL,
    name            TEXT NOT NULL,
    scope           TEXT NOT NULL,
    tz              TEXT NOT NULL,
    access_hash     TEXT NOT NULL,
    access_wrapped  TEXT NOT NULL,
    access_expires  INTEGER NOT NULL,
    refresh_hash    TEXT NOT NULL,
    refresh_wrapped TEXT NOT NULL,
    created_at      TEXT NOT NULL,
    last_used       TEXT
);
CREATE INDEX IF NOT EXISTS assistants_by_account ON assistants (account);
CREATE INDEX IF NOT EXISTS assistants_by_access ON assistants (access_hash);
CREATE INDEX IF NOT EXISTS assistants_by_refresh ON assistants (refresh_hash);
";

/// How long an access token lasts; the assistant refreshes it after.
const ACCESS_SECS: i64 = 3600;
/// How long a sign-in code waits to be exchanged.
const CODE_SECS: i64 = 300;
/// The widest agenda one call returns.
const MAX_DAYS: i64 = 62;

const READ: &str = "tasks:read";
const WRITE: &str = "tasks:write";

/// MCP versions this server speaks, newest first.
const VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

fn unix() -> i64 {
    chrono::Utc::now().timestamp()
}

fn json_out(code: u16, v: Value) -> Outgoing {
    Outgoing::json((code, v))
}

fn oauth_err(code: u16, error: &str, description: &str) -> Outgoing {
    json_out(
        code,
        json!({ "error": error, "error_description": description }),
    )
}

fn html_out(code: u16, page: String) -> Outgoing {
    Outgoing {
        code,
        content_type: "text/html; charset=utf-8".into(),
        headers: vec![
            ("Cache-Control".into(), "no-store".into()),
            ("X-Frame-Options".into(), "DENY".into()),
        ],
        body: page.into_bytes(),
    }
}

fn redirect(to: &str) -> Outgoing {
    Outgoing {
        code: 302,
        content_type: "text/plain".into(),
        headers: vec![("Location".into(), to.to_string())],
        body: Vec::new(),
    }
}

/// The server's public address: `TASQ_RELAY_PUBLIC_URL`, else the host the
/// request came to (HTTPS unless it's this machine).
fn base_url(req: &Incoming) -> String {
    if let Ok(u) = std::env::var("TASQ_RELAY_PUBLIC_URL")
        && !u.is_empty()
    {
        return u.trim_end_matches('/').to_string();
    }
    let host = req.host.clone().unwrap_or_else(|| "localhost".into());
    let local = host.starts_with("localhost") || host.starts_with("127.0.0.1");
    let proto = req
        .proto
        .clone()
        .unwrap_or_else(|| if local { "http" } else { "https" }.into());
    format!("{proto}://{host}")
}

/// `a=1&b=x%20y` → pairs, decoded (`+` is a space).
pub fn parse_form(s: &str) -> Vec<(String, String)> {
    fn decode(s: &str) -> String {
        let bytes = s.as_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'+' => out.push(b' '),
                b'%' if i + 2 < bytes.len() + 0 && i + 2 <= bytes.len() - 1 => {
                    match u8::from_str_radix(&s[i + 1..i + 3], 16) {
                        Ok(b) => {
                            out.push(b);
                            i += 2;
                        }
                        Err(_) => out.push(b'%'),
                    }
                }
                b => out.push(b),
            }
            i += 1;
        }
        String::from_utf8_lossy(&out).into_owned()
    }
    s.split('&')
        .filter(|kv| !kv.is_empty())
        .map(|kv| {
            let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
            (decode(k), decode(v))
        })
        .collect()
}

fn field<'a>(form: &'a [(String, String)], key: &str) -> Option<&'a str> {
    form.iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

fn esc_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Answer `req` if it's one of the assistant routes.
pub fn respond(relay: &Relay, req: &Incoming) -> Option<Outgoing> {
    let (route, query) = req.url.split_once('?').unwrap_or((&req.url, ""));
    let base = base_url(req);
    let out = match (req.method.as_str(), route) {
        ("GET", r) if r.starts_with("/.well-known/oauth-protected-resource") => json_out(
            200,
            json!({
                "resource": format!("{base}/mcp"),
                "authorization_servers": [base],
                "scopes_supported": [READ, WRITE],
                "bearer_methods_supported": ["header"],
                "resource_name": "tasq",
            }),
        ),
        ("GET", r)
            if r.starts_with("/.well-known/oauth-authorization-server")
                || r.starts_with("/.well-known/openid-configuration") =>
        {
            json_out(
                200,
                json!({
                    "issuer": base,
                    "authorization_endpoint": format!("{base}/oauth/authorize"),
                    "token_endpoint": format!("{base}/oauth/token"),
                    "registration_endpoint": format!("{base}/oauth/register"),
                    "response_types_supported": ["code"],
                    "grant_types_supported": ["authorization_code", "refresh_token"],
                    "code_challenge_methods_supported": ["S256"],
                    "token_endpoint_auth_methods_supported": ["none"],
                    "scopes_supported": [READ, WRITE],
                }),
            )
        }
        ("POST", "/oauth/register") => register(relay, &req.body),
        ("GET", "/oauth/authorize") => authorize_page(relay, &parse_form(query), None),
        ("POST", "/oauth/authorize") => {
            let form = parse_form(&String::from_utf8_lossy(&req.body));
            authorize_submit(relay, &form)
        }
        ("POST", "/oauth/token") => {
            let form = parse_form(&String::from_utf8_lossy(&req.body));
            token(relay, &form)
        }
        ("POST", "/mcp") => mcp(relay, req, &base),
        ("GET" | "DELETE", "/mcp") => json_out(405, json!({ "error": "POST only" })),
        ("GET", "/v1/assistants") => list_assistants(relay, req.auth.as_deref()),
        ("POST", "/v1/assistants/revoke") => revoke(relay, req.auth.as_deref(), &req.body),
        _ => return None,
    };
    Some(out)
}

fn with_conn<T>(relay: &Relay, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> Option<T> {
    let conn = relay.conn.lock().ok()?;
    f(&conn).ok()
}

/// A redirect an assistant may be sent back to: HTTPS, or this machine.
fn redirect_ok(uri: &str) -> bool {
    uri.starts_with("https://")
        || uri.starts_with("http://localhost")
        || uri.starts_with("http://127.0.0.1")
}

fn register(relay: &Relay, body: &[u8]) -> Outgoing {
    let Ok(v) = serde_json::from_slice::<Value>(body) else {
        return oauth_err(400, "invalid_client_metadata", "expected JSON");
    };
    let uris: Vec<String> = v["redirect_uris"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|u| u.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    if uris.is_empty() || uris.len() > 10 || !uris.iter().all(|u| redirect_ok(u)) {
        return oauth_err(
            400,
            "invalid_redirect_uri",
            "redirect_uris must be https (or localhost)",
        );
    }
    let name: String = v["client_name"]
        .as_str()
        .unwrap_or("An assistant")
        .chars()
        .take(80)
        .collect();
    let Ok(id) = keys::random_token() else {
        return oauth_err(500, "server_error", "no randomness");
    };
    let id = id[..32].to_string();
    let saved = with_conn(relay, |c| {
        c.execute(
            "INSERT INTO oauth_clients (id, name, redirect_uris, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![id, name, json!(uris).to_string(), now()],
        )
    });
    if saved.is_none() {
        return oauth_err(500, "server_error", "couldn't save the client");
    }
    json_out(
        201,
        json!({
            "client_id": id,
            "client_name": name,
            "redirect_uris": uris,
            "grant_types": ["authorization_code", "refresh_token"],
            "response_types": ["code"],
            "token_endpoint_auth_method": "none",
            "client_id_issued_at": unix(),
        }),
    )
}

struct Client {
    name: String,
    redirects: Vec<String>,
}

fn client(relay: &Relay, id: &str) -> Option<Client> {
    with_conn(relay, |c| {
        c.query_row(
            "SELECT name, redirect_uris FROM oauth_clients WHERE id = ?1",
            [id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .optional()
    })
    .flatten()
    .map(|(name, uris)| Client {
        name,
        redirects: serde_json::from_str(&uris).unwrap_or_default(),
    })
}

/// The sign-in request, checked: what the page carries in hidden fields.
struct AuthRequest {
    client_id: String,
    client: Client,
    redirect_uri: String,
    state: String,
    challenge: String,
    scope: String,
}

fn auth_request(relay: &Relay, form: &[(String, String)]) -> Result<AuthRequest, Outgoing> {
    let bad = |msg: &str| html_out(400, page_message("Can't connect", msg));
    let client_id = field(form, "client_id").unwrap_or_default().to_string();
    let Some(client) = client(relay, &client_id) else {
        return Err(bad(
            "This assistant isn't registered with tasq. Remove the connector and add it again.",
        ));
    };
    let redirect_uri = field(form, "redirect_uri").unwrap_or_default().to_string();
    if !client.redirects.iter().any(|u| *u == redirect_uri) {
        return Err(bad("The assistant asked to come back to an address it didn't register."));
    }
    if field(form, "response_type") != Some("code") {
        return Err(bad("Only the code flow is supported."));
    }
    let challenge = field(form, "code_challenge").unwrap_or_default().to_string();
    if challenge.len() < 43 || field(form, "code_challenge_method") != Some("S256") {
        return Err(bad("The assistant must use PKCE (S256)."));
    }
    let asked = field(form, "scope").unwrap_or(WRITE);
    let scope = if asked.split_whitespace().any(|s| s == WRITE) || asked.trim().is_empty() {
        WRITE
    } else {
        READ
    };
    Ok(AuthRequest {
        client_id,
        client,
        redirect_uri,
        state: field(form, "state").unwrap_or_default().to_string(),
        challenge,
        scope: scope.to_string(),
    })
}

const STYLE: &str = r#"
:root { --bg:#f6f5f2; --card:#fff; --text:#1d1d1f; --muted:#6b6b70; --line:#e2e0db;
  --accent:#5b5bd6; --warn-bg:#fff6e5; --warn:#8a5a00; --err:#b42318; }
@media (prefers-color-scheme: dark) { :root { --bg:#141416; --card:#1e1e22; --text:#ececf1;
  --muted:#9a9aa3; --line:#2e2e34; --accent:#8b8bf0; --warn-bg:#2a2214; --warn:#f0c674; --err:#ff8a80; } }
* { box-sizing:border-box; }
body { margin:0; background:var(--bg); color:var(--text);
  font:16px/1.5 ui-sans-serif, system-ui, -apple-system, "Segoe UI", sans-serif; }
main { max-width:420px; margin:0 auto; padding:40px 16px; }
.card { background:var(--card); border:1px solid var(--line); border-radius:14px; padding:24px; }
h1 { font-size:22px; margin:0 0 4px; }
p.sub { margin:0 0 20px; color:var(--muted); }
label { display:block; font-size:14px; font-weight:600; margin:14px 0 6px; }
input[type=email], input[type=password] { width:100%; padding:11px 12px; font:inherit;
  border:1px solid var(--line); border-radius:9px; background:var(--bg); color:var(--text); }
.choice { display:flex; gap:10px; align-items:flex-start; border:1px solid var(--line);
  border-radius:9px; padding:10px 12px; margin:8px 0; font-weight:400; cursor:pointer; }
.choice small { display:block; color:var(--muted); }
.note { background:var(--warn-bg); color:var(--warn); border-radius:9px; padding:10px 12px;
  font-size:14px; margin:18px 0 0; }
.error { color:var(--err); font-size:14px; margin:12px 0 0; }
button { width:100%; margin-top:20px; padding:12px; font:inherit; font-weight:600; color:#fff;
  background:var(--accent); border:0; border-radius:9px; cursor:pointer; }
.brand { font-weight:700; letter-spacing:.02em; margin-bottom:16px; color:var(--muted); }
"#;

fn page(title: &str, inner: &str) -> String {
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <title>{}</title><style>{STYLE}</style></head><body><main>\
         <div class=\"brand\">tasq</div><div class=\"card\">{inner}</div></main></body></html>",
        esc_html(title)
    )
}

fn page_message(title: &str, msg: &str) -> String {
    page(
        title,
        &format!("<h1>{}</h1><p class=\"sub\">{}</p>", esc_html(title), esc_html(msg)),
    )
}

fn authorize_page(relay: &Relay, form: &[(String, String)], error: Option<&str>) -> Outgoing {
    let ar = match auth_request(relay, form) {
        Ok(ar) => ar,
        Err(out) => return out,
    };
    let hidden: String = [
        ("client_id", ar.client_id.as_str()),
        ("redirect_uri", ar.redirect_uri.as_str()),
        ("state", ar.state.as_str()),
        ("code_challenge", ar.challenge.as_str()),
        ("code_challenge_method", "S256"),
        ("response_type", "code"),
        ("scope", ar.scope.as_str()),
    ]
    .iter()
    .map(|(k, v)| format!("<input type=\"hidden\" name=\"{k}\" value=\"{}\">", esc_html(v)))
    .collect();
    let email = field(form, "email").unwrap_or_default();
    let write = field(form, "access").map_or(ar.scope == WRITE, |a| a == "write");
    let (rc, wc) = if write { ("", " checked") } else { (" checked", "") };
    let err = error
        .map(|e| format!("<p class=\"error\">{}</p>", esc_html(e)))
        .unwrap_or_default();
    let name = esc_html(&ar.client.name);
    let inner = format!(
        r#"<h1>Connect {name} to tasq</h1>
<p class="sub">Sign in with the account your computers sync with.</p>
<form method="post" action="/oauth/authorize">{hidden}
<input type="hidden" name="tz" id="tz" value="">
<label for="email">Email</label>
<input type="email" id="email" name="email" autocomplete="username" required value="{email}">
<label for="password">Password</label>
<input type="password" id="password" name="password" autocomplete="current-password" required minlength="10">
<label>What it can do</label>
<label class="choice"><input type="radio" name="access" value="read"{rc}>
<span>See my tasks and calendar<small>What's on today, what's next, search.</small></span></label>
<label class="choice"><input type="radio" name="access" value="write"{wc}>
<span>See them, and add or change tasks<small>Add, complete and reschedule. It never deletes.</small></span></label>
<p class="note">While {name} is connected, tasq's server reads your tasks each time it asks, to answer it. The rest of the time your data stays end-to-end encrypted. Disconnect it any time in tasq → Settings → Sync.</p>
{err}
<button type="submit">Connect</button>
</form>
<script>try{{document.getElementById('tz').value=Intl.DateTimeFormat().resolvedOptions().timeZone||''}}catch(e){{}}</script>"#,
        email = esc_html(email),
    );
    html_out(if error.is_some() { 400 } else { 200 }, page("Connect to tasq", &inner))
}

fn authorize_submit(relay: &Relay, form: &[(String, String)]) -> Outgoing {
    let ar = match auth_request(relay, form) {
        Ok(ar) => ar,
        Err(out) => return out,
    };
    let email = field(form, "email").unwrap_or_default();
    let password = field(form, "password").unwrap_or_default();
    let keys = match Keys::derive(email, password) {
        Ok(k) => k,
        Err(e) => return authorize_page(relay, form, Some(&e.to_string())),
    };
    let stored: Option<String> = with_conn(relay, |c| {
        c.query_row(
            "SELECT auth_hash FROM accounts WHERE id = ?1",
            [&keys.account],
            |r| r.get(0),
        )
        .optional()
    })
    .flatten();
    match stored {
        None => {
            return authorize_page(
                relay,
                form,
                Some("There's no tasq account for that email on this server. Sign in from tasq on your computer first (Settings → Sync)."),
            );
        }
        Some(h) if !keys.matches(&h) => {
            return authorize_page(relay, form, Some("That password isn't right."));
        }
        Some(_) => {}
    }
    let scope = if field(form, "access") == Some("read") {
        READ.to_string()
    } else {
        format!("{READ} {WRITE}")
    };
    let tz = field(form, "tz")
        .filter(|t| t.parse::<chrono_tz::Tz>().is_ok())
        .unwrap_or("UTC")
        .to_string();
    let Ok(code) = keys::random_token() else {
        return html_out(500, page_message("Something went wrong", "Try again."));
    };
    let Ok(wrapped) = keys::wrap(&keys.to_secret(), &code) else {
        return html_out(500, page_message("Something went wrong", "Try again."));
    };
    let saved = with_conn(relay, |c| {
        c.execute(
            "DELETE FROM oauth_codes WHERE expires < ?1",
            [unix()],
        )?;
        c.execute(
            "INSERT INTO oauth_codes (hash, client, redirect, challenge, scope, tz, account, wrapped, expires)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                keys::token_hash(&code),
                ar.client_id,
                ar.redirect_uri,
                ar.challenge,
                scope,
                tz,
                keys.account,
                wrapped,
                unix() + CODE_SECS
            ],
        )
    });
    if saved.is_none() {
        return html_out(500, page_message("Something went wrong", "Try again."));
    }
    let sep = if ar.redirect_uri.contains('?') { '&' } else { '?' };
    let mut to = format!("{}{sep}code={}", ar.redirect_uri, http::percent(&code));
    if !ar.state.is_empty() {
        to.push_str(&format!("&state={}", http::percent(&ar.state)));
    }
    redirect(&to)
}

/// New access and refresh tokens for `secret` (the keys), sealed under each.
struct Issued {
    access: String,
    refresh: String,
    access_wrapped: String,
    refresh_wrapped: String,
}

fn issue(secret: &str) -> Result<Issued> {
    let access = keys::random_token()?;
    let refresh = keys::random_token()?;
    Ok(Issued {
        access_wrapped: keys::wrap(secret, &access)?,
        refresh_wrapped: keys::wrap(secret, &refresh)?,
        access,
        refresh,
    })
}

fn token_answer(t: &Issued, scope: &str) -> Outgoing {
    let mut out = json_out(
        200,
        json!({
            "access_token": t.access,
            "token_type": "Bearer",
            "expires_in": ACCESS_SECS,
            "refresh_token": t.refresh,
            "scope": scope,
        }),
    );
    out.headers.push(("Cache-Control".into(), "no-store".into()));
    out
}

fn token(relay: &Relay, form: &[(String, String)]) -> Outgoing {
    match field(form, "grant_type") {
        Some("authorization_code") => token_from_code(relay, form),
        Some("refresh_token") => token_from_refresh(relay, form),
        _ => oauth_err(400, "unsupported_grant_type", "authorization_code or refresh_token"),
    }
}

fn token_from_code(relay: &Relay, form: &[(String, String)]) -> Outgoing {
    let code = field(form, "code").unwrap_or_default();
    let hash = keys::token_hash(code);
    type Row = (String, String, String, String, String, String, String, i64);
    let row: Option<Row> = with_conn(relay, |c| {
        let row = c
            .query_row(
                "SELECT client, redirect, challenge, scope, tz, account, wrapped, expires
                 FROM oauth_codes WHERE hash = ?1",
                [&hash],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                        r.get(7)?,
                    ))
                },
            )
            .optional()?;
        // A code works once.
        c.execute("DELETE FROM oauth_codes WHERE hash = ?1", [&hash])?;
        Ok(row)
    })
    .flatten();
    let Some((client_id, redirect, challenge, scope, tz, account, wrapped, expires)) = row else {
        return oauth_err(400, "invalid_grant", "unknown or used code");
    };
    if expires < unix() {
        return oauth_err(400, "invalid_grant", "the code expired");
    }
    if field(form, "client_id").is_some_and(|c| c != client_id)
        || field(form, "redirect_uri").is_some_and(|r| r != redirect)
    {
        return oauth_err(400, "invalid_grant", "client or redirect_uri mismatch");
    }
    let verifier = field(form, "code_verifier").unwrap_or_default();
    if http::b64url(&http::sha256(verifier.as_bytes())) != challenge {
        return oauth_err(400, "invalid_grant", "PKCE verification failed");
    }
    let Some(secret) = keys::unwrap(&wrapped, code) else {
        return oauth_err(400, "invalid_grant", "unreadable code");
    };
    let Ok(t) = issue(&secret) else {
        return oauth_err(500, "server_error", "no randomness");
    };
    let name = client(relay, &client_id).map_or_else(|| "Assistant".into(), |c| c.name);
    let id = crate::core::db::new_ulid();
    let saved = with_conn(relay, |c| {
        c.execute(
            "INSERT INTO assistants (id, account, client, name, scope, tz, access_hash, access_wrapped,
                access_expires, refresh_hash, refresh_wrapped, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                id,
                account,
                client_id,
                name,
                scope,
                tz,
                keys::token_hash(&t.access),
                t.access_wrapped,
                unix() + ACCESS_SECS,
                keys::token_hash(&t.refresh),
                t.refresh_wrapped,
                now()
            ],
        )
    });
    if saved.is_none() {
        return oauth_err(500, "server_error", "couldn't save the connection");
    }
    token_answer(&t, &scope)
}

fn token_from_refresh(relay: &Relay, form: &[(String, String)]) -> Outgoing {
    let refresh = field(form, "refresh_token").unwrap_or_default();
    let hash = keys::token_hash(refresh);
    let row: Option<(String, String, String, String)> = with_conn(relay, |c| {
        c.query_row(
            "SELECT id, client, scope, refresh_wrapped FROM assistants WHERE refresh_hash = ?1",
            [&hash],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()
    })
    .flatten();
    let Some((id, client_id, scope, wrapped)) = row else {
        return oauth_err(400, "invalid_grant", "unknown refresh token (disconnected?)");
    };
    if field(form, "client_id").is_some_and(|c| c != client_id) {
        return oauth_err(400, "invalid_grant", "client mismatch");
    }
    let Some(secret) = keys::unwrap(&wrapped, refresh) else {
        return oauth_err(400, "invalid_grant", "unreadable refresh token");
    };
    let Ok(t) = issue(&secret) else {
        return oauth_err(500, "server_error", "no randomness");
    };
    let saved = with_conn(relay, |c| {
        c.execute(
            "UPDATE assistants SET access_hash = ?2, access_wrapped = ?3, access_expires = ?4,
                refresh_hash = ?5, refresh_wrapped = ?6 WHERE id = ?1",
            params![
                id,
                keys::token_hash(&t.access),
                t.access_wrapped,
                unix() + ACCESS_SECS,
                keys::token_hash(&t.refresh),
                t.refresh_wrapped
            ],
        )
    });
    if saved.is_none() {
        return oauth_err(500, "server_error", "couldn't save the connection");
    }
    token_answer(&t, &scope)
}

/// Who's asking, once their token checks out.
struct Caller {
    keys: Keys,
    tz: chrono_tz::Tz,
    write: bool,
}

fn caller(relay: &Relay, auth: Option<&str>) -> Option<Caller> {
    let token = auth?.strip_prefix("Bearer ")?.trim();
    let hash = keys::token_hash(token);
    let (id, scope, tz, wrapped, expires): (String, String, String, String, i64) =
        with_conn(relay, |c| {
            c.query_row(
                "SELECT id, scope, tz, access_wrapped, access_expires FROM assistants
                 WHERE access_hash = ?1",
                [&hash],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()
        })
        .flatten()?;
    if expires < unix() {
        return None;
    }
    let keys = Keys::from_secret(&keys::unwrap(&wrapped, token)?)?;
    with_conn(relay, |c| {
        c.execute(
            "UPDATE assistants SET last_used = ?2 WHERE id = ?1",
            params![id, now()],
        )
    });
    Some(Caller {
        keys,
        tz: tz.parse().unwrap_or(chrono_tz::UTC),
        write: scope.split_whitespace().any(|s| s == WRITE),
    })
}

fn unauthorized(base: &str) -> Outgoing {
    let mut out = json_out(401, json!({ "error": "sign in first" }));
    out.headers.push((
        "WWW-Authenticate".into(),
        format!(
            "Bearer resource_metadata=\"{base}/.well-known/oauth-protected-resource\", scope=\"{READ} {WRITE}\""
        ),
    ));
    out
}

fn rpc_result(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn rpc_error(id: &Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn mcp(relay: &Relay, req: &Incoming, base: &str) -> Outgoing {
    let Some(who) = caller(relay, req.auth.as_deref()) else {
        return unauthorized(base);
    };
    let Ok(msg) = serde_json::from_slice::<Value>(&req.body) else {
        return json_out(400, rpc_error(&Value::Null, -32700, "parse error"));
    };
    match msg {
        Value::Array(batch) => {
            let answers: Vec<Value> = batch
                .iter()
                .filter_map(|m| rpc(relay, &who, m))
                .collect();
            if answers.is_empty() {
                accepted()
            } else {
                json_out(200, Value::Array(answers))
            }
        }
        m => match rpc(relay, &who, &m) {
            Some(answer) => json_out(200, answer),
            None => accepted(),
        },
    }
}

fn accepted() -> Outgoing {
    Outgoing {
        code: 202,
        content_type: "application/json".into(),
        headers: Vec::new(),
        body: Vec::new(),
    }
}

const INSTRUCTIONS: &str = "tasq is the user's task manager and calendar. Use `now` for what's \
happening right now and next, `agenda` for a day or a range, `search` to find a task, `inbox` \
for tasks with no day yet. Times are the user's local time. To add, pass the user's words to \
`add_task` as they'd type them in tasq (English or Spanish, e.g. \"dentist friday at 6pm for 1h \
in health\"); tasq reads dates, times, durations, repeats and spaces from the text. Use a task's \
`id` from a previous answer to complete or reschedule it.";

/// One JSON-RPC message; `None` for a notification.
fn rpc(relay: &Relay, who: &Caller, m: &Value) -> Option<Value> {
    let id = m.get("id")?.clone();
    let method = m["method"].as_str().unwrap_or_default();
    let params = &m["params"];
    Some(match method {
        "initialize" => {
            let asked = params["protocolVersion"].as_str().unwrap_or("");
            let version = VERSIONS
                .iter()
                .find(|v| **v == asked)
                .copied()
                .unwrap_or(VERSIONS[1]);
            rpc_result(
                &id,
                json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": { "listChanged": false } },
                    "serverInfo": {
                        "name": "tasq",
                        "title": "tasq",
                        "version": env!("CARGO_PKG_VERSION"),
                    },
                    "instructions": INSTRUCTIONS,
                }),
            )
        }
        "ping" => rpc_result(&id, json!({})),
        "tools/list" => rpc_result(&id, json!({ "tools": tool_list(who.write) })),
        "tools/call" => {
            let name = params["name"].as_str().unwrap_or_default();
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            let result = match call_tool(relay, who, name, &args) {
                Ok(v) => json!({
                    "content": [{ "type": "text", "text": serde_json::to_string_pretty(&v).unwrap_or_default() }],
                    "structuredContent": v,
                    "isError": false,
                }),
                Err(e) => json!({
                    "content": [{ "type": "text", "text": e.to_string() }],
                    "isError": true,
                }),
            };
            rpc_result(&id, result)
        }
        "resources/list" => rpc_result(&id, json!({ "resources": [] })),
        "prompts/list" => rpc_result(&id, json!({ "prompts": [] })),
        _ => rpc_error(&id, -32601, "method not found"),
    })
}

fn tool(name: &str, title: &str, description: &str, props: Value, required: &[&str], read_only: bool) -> Value {
    json!({
        "name": name,
        "title": title,
        "description": description,
        "inputSchema": { "type": "object", "properties": props, "required": required },
        "annotations": {
            "title": title,
            "readOnlyHint": read_only,
            "destructiveHint": false,
            "idempotentHint": read_only,
            "openWorldHint": false,
        },
    })
}

fn tool_list(write: bool) -> Vec<Value> {
    let date = json!({ "type": "string", "description": "YYYY-MM-DD" });
    let mut tools = vec![
        tool(
            "now",
            "What's on now",
            "The user's local date and time, what they're doing right now, what comes next, and the rest of today.",
            json!({}),
            &[],
            true,
        ),
        tool(
            "agenda",
            "Agenda",
            "Everything on the calendar from `from` to `to` (inclusive, at most 62 days; both default to today): timed blocks, all-day tasks, deadlines and repeats, by day. Late open tasks show on today.",
            json!({ "from": date, "to": date }),
            &[],
            true,
        ),
        tool(
            "search",
            "Search tasks",
            "Find tasks whose title, space or tags contain the words (accents and case don't matter).",
            json!({
                "query": { "type": "string" },
                "include_done": { "type": "boolean", "description": "also completed tasks still in the list" },
            }),
            &["query"],
            true,
        ),
        tool(
            "inbox",
            "Inbox",
            "Open tasks with no day yet: what's left to put in the calendar.",
            json!({}),
            &[],
            true,
        ),
    ];
    if write {
        tools.extend([
            tool(
                "add_task",
                "Add a task",
                "Add a task from natural language, as the user would type it in tasq (English or Spanish): dates (\"tomorrow\", \"el viernes\", \"by friday\" for a deadline), times (\"at 6pm\", \"a las 10\"), durations (\"for 1h\"), repeats (\"every monday\"), spaces (\"in work\"), tags (@calls), priority (!high). Returns the task as tasq understood it.",
                json!({ "text": { "type": "string" } }),
                &["text"],
                false,
            ),
            tool(
                "complete_task",
                "Complete a task",
                "Mark a task done by its id. A repeating task moves on to its next time.",
                json!({ "id": { "type": "string" } }),
                &["id"],
                false,
            ),
            tool(
                "reschedule_task",
                "Reschedule a task",
                "Move a task by its id: a new planned `date` (YYYY-MM-DD), a new `time` (HH:MM, 24h), or both. `time: \"\"` makes it all-day.",
                json!({
                    "id": { "type": "string" },
                    "date": date,
                    "time": { "type": "string", "description": "HH:MM, or empty for all-day" },
                }),
                &["id"],
                false,
            ),
        ]);
    }
    tools
}

/// The account's tasks, read by replaying its changes into a database in
/// memory. Nothing is written to disk.
struct Account<'a> {
    relay: &'a Relay,
    keys: Keys,
    today: String,
    store: Store,
}

impl<'a> Account<'a> {
    fn open(relay: &'a Relay, keys: &Keys, today: &str) -> Result<Self> {
        let mut db = Db::in_memory();
        let mut remote = Local {
            relay,
            keys: keys.clone(),
        };
        sync_once(&mut db, &mut remote, keys, today)?;
        let store = Store::from_db(db, PathBuf::from(":memory:"), today.to_string())?;
        Ok(Self {
            relay,
            keys: keys.clone(),
            today: today.to_string(),
            store,
        })
    }

    /// Send what changed here to the account, for the user's computers.
    fn send(&mut self) -> Result<()> {
        let db = self
            .store
            .db
            .as_mut()
            .ok_or_else(|| anyhow!("no database"))?;
        let mut remote = Local {
            relay: self.relay,
            keys: self.keys.clone(),
        };
        sync_once(db, &mut remote, &self.keys, &self.today)?;
        Ok(())
    }

    fn find(&self, id: &str) -> Result<usize> {
        self.store
            .tasks()
            .iter()
            .position(|t| t.id == id)
            .ok_or_else(|| anyhow!("no task with id {id} (search for it again)"))
    }
}

fn hhmm(m: u32) -> String {
    format!("{:02}:{:02}", m / 60 % 24, m % 60)
}

fn task_json(t: &Task) -> Value {
    let body = todo::body_after_priority(&t.clean_raw);
    json!({
        "id": t.id,
        "title": title_of(&t.raw),
        "space": t.projects.first(),
        "tags": t.contexts,
        "priority": t.priority.map(|p| p.to_string()),
        "planned": t.planned,
        "due": t.due,
        "time": todo::find_kv(body, "at"),
        "duration": t.duration,
        "repeat": t.rec,
        "ends": t.end,
        "event": t.event,
        "starred": t.starred,
        "done": t.done,
    })
}

fn occurrence_json(o: &Occurrence, tasks: &[Task]) -> Value {
    let mut v = task_json(&tasks[o.abs]);
    v["date"] = json!(o.date.to_string());
    v["start"] = json!(o.start.map(hhmm));
    v["end"] = json!(o.end().map(hhmm));
    v["all_day"] = json!(o.start.is_none());
    v["deadline"] = json!(o.deadline);
    v["late"] = json!(o.late);
    v["upcoming_repeat"] = json!(o.projected);
    v
}

fn arg_date(args: &Value, key: &str) -> Result<Option<NaiveDate>> {
    match args[key].as_str().filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some(s) => NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .map(Some)
            .map_err(|_| anyhow!("{key} must be YYYY-MM-DD")),
    }
}

fn call_tool(relay: &Relay, who: &Caller, name: &str, args: &Value) -> Result<Value> {
    let local = chrono::Utc::now().with_timezone(&who.tz);
    let today = local.date_naive();
    let today_s = today.format("%Y-%m-%d").to_string();
    let writes = matches!(name, "add_task" | "complete_task" | "reschedule_task");
    if writes && !who.write {
        return Err(anyhow!(
            "this connection can only read; reconnect tasq allowing changes"
        ));
    }
    let mut acct = Account::open(relay, &who.keys, &today_s)?;
    let tasks = acct.store.tasks().to_vec();
    match name {
        "now" => {
            let minute = local.hour() * 60 + local.minute();
            let occs = calendar::occurrences(&tasks, today, today + Duration::days(1), today);
            let open = |o: &&Occurrence| !tasks[o.abs].done;
            let today_occs: Vec<&Occurrence> =
                occs.iter().filter(|o| o.date == today).filter(open).collect();
            let current: Vec<Value> = today_occs
                .iter()
                .filter(|o| o.start.is_some_and(|s| s <= minute) && o.end().is_some_and(|e| e > minute))
                .map(|o| occurrence_json(o, &tasks))
                .collect();
            let next = occs
                .iter()
                .filter(open)
                .find(|o| o.start.is_some() && (o.date > today || o.start.is_some_and(|s| s > minute)))
                .map(|o| occurrence_json(o, &tasks));
            Ok(json!({
                "now": local.format("%Y-%m-%d %H:%M (%A)").to_string(),
                "timezone": who.tz.name(),
                "current": current,
                "next": next,
                "today": today_occs.iter().map(|o| occurrence_json(o, &tasks)).collect::<Vec<_>>(),
            }))
        }
        "agenda" => {
            let from = arg_date(args, "from")?.unwrap_or(today);
            let to = arg_date(args, "to")?.unwrap_or(from);
            if to < from {
                return Err(anyhow!("`to` is before `from`"));
            }
            if (to - from).num_days() >= MAX_DAYS {
                return Err(anyhow!("at most {MAX_DAYS} days at a time"));
            }
            let occs = calendar::occurrences(&tasks, from, to, today);
            let mut days: Vec<Value> = Vec::new();
            let mut d = from;
            while d <= to {
                let items: Vec<Value> = occs
                    .iter()
                    .filter(|o| o.date == d)
                    .map(|o| occurrence_json(o, &tasks))
                    .collect();
                days.push(json!({
                    "date": d.to_string(),
                    "weekday": d.format("%A").to_string(),
                    "items": items,
                }));
                d += Duration::days(1);
            }
            Ok(json!({ "today": today_s, "timezone": who.tz.name(), "days": days }))
        }
        "search" => {
            let q = args["query"].as_str().unwrap_or_default();
            let words: Vec<String> = crate::nl_es::fold(q)
                .split_whitespace()
                .map(str::to_string)
                .collect();
            if words.is_empty() {
                return Err(anyhow!("search for something"));
            }
            let include_done = args["include_done"].as_bool().unwrap_or(false);
            let found: Vec<Value> = tasks
                .iter()
                .filter(|t| include_done || !t.done)
                .filter(|t| {
                    let hay = crate::nl_es::fold(&format!(
                        "{} {} {}",
                        title_of(&t.raw),
                        t.projects.join(" "),
                        t.contexts.join(" ")
                    ));
                    words.iter().all(|w| hay.contains(w.as_str()))
                })
                .take(50)
                .map(task_json)
                .collect();
            Ok(json!({ "count": found.len(), "tasks": found }))
        }
        "inbox" => {
            let found: Vec<Value> = tasks
                .iter()
                .filter(|t| !t.done && t.planned.is_none() && t.due.is_none())
                .map(task_json)
                .collect();
            Ok(json!({ "count": found.len(), "tasks": found }))
        }
        "add_task" => {
            let text = args["text"].as_str().unwrap_or_default().trim();
            if text.is_empty() {
                return Err(anyhow!("say what to add"));
            }
            let spaces: Vec<String> = acct
                .store
                .known_spaces()
                .iter()
                .map(|s| s.path.clone())
                .collect();
            let d = crate::nl::detect_in(text, today, &[], &spaces);
            let line = if d.is_empty() { text.to_string() } else { d.to_todo_txt() };
            match acct.store.add_finalized(&line) {
                AddOutcome::Added { abs } => {
                    let t = acct.store.tasks()[abs].clone();
                    acct.send()?;
                    Ok(json!({ "added": task_json(&t) }))
                }
                AddOutcome::Empty => Err(anyhow!("say what to add")),
                AddOutcome::Aborted(_) => Err(anyhow!("the tasks changed meanwhile; try again")),
                AddOutcome::Error(e) => Err(anyhow!("tasq couldn't read that: {e}")),
            }
        }
        "complete_task" => {
            let abs = acct.find(args["id"].as_str().unwrap_or_default())?;
            if acct.store.tasks()[abs].done {
                return Ok(json!({ "already_done": task_json(&acct.store.tasks()[abs]) }));
            }
            let next = match acct.store.toggle_complete(abs) {
                CompleteOutcome::CompletedSpawned { next, .. } => {
                    Some(task_json(&acct.store.tasks()[next]))
                }
                CompleteOutcome::Completed { .. } | CompleteOutcome::CompletedLast { .. } => None,
                other => return Err(anyhow!("couldn't complete it: {other:?}")),
            };
            let done = task_json(&acct.store.tasks()[abs]);
            acct.send()?;
            Ok(json!({ "completed": done, "next_repeat": next }))
        }
        "reschedule_task" => {
            let abs = acct.find(args["id"].as_str().unwrap_or_default())?;
            let mut raw = acct.store.tasks()[abs].raw.clone();
            if let Some(d) = arg_date(args, "date")? {
                raw = series::set_kv(&raw, "plan", Some(&d.to_string()));
            }
            if let Some(t) = args["time"].as_str() {
                let t = t.trim();
                if t.is_empty() {
                    raw = series::set_kv(&raw, "at", None);
                } else {
                    let m = calendar::parse_time(t)
                        .ok_or_else(|| anyhow!("time must be HH:MM (24h)"))?;
                    raw = series::set_kv(&raw, "at", Some(&hhmm(m)));
                }
            }
            match acct.store.edit_line(abs, &raw) {
                EditOutcome::Saved { abs } => {
                    let t = acct.store.tasks()[abs].clone();
                    acct.send()?;
                    Ok(json!({ "rescheduled": task_json(&t) }))
                }
                other => Err(anyhow!("couldn't change it: {other:?}")),
            }
        }
        other => Err(anyhow!("no tool called {other}")),
    }
}

fn list_assistants(relay: &Relay, auth: Option<&str>) -> Outgoing {
    let account = match device_account(relay, auth) {
        Ok(a) => a,
        Err(r) => return Outgoing::json(r),
    };
    let rows: Vec<Value> = with_conn(relay, |c| {
        let mut stmt = c.prepare(
            "SELECT id, name, scope, created_at, last_used FROM assistants
             WHERE account = ?1 ORDER BY created_at",
        )?;
        stmt.query_map([&account], |r| {
            Ok(json!({
                "id": r.get::<_, String>(0)?,
                "name": r.get::<_, String>(1)?,
                "write": r.get::<_, String>(2)?.contains(WRITE),
                "created_at": r.get::<_, String>(3)?,
                "last_used": r.get::<_, Option<String>>(4)?,
            }))
        })?
        .collect()
    })
    .unwrap_or_default();
    json_out(200, json!({ "assistants": rows }))
}

fn revoke(relay: &Relay, auth: Option<&str>, body: &[u8]) -> Outgoing {
    let account = match device_account(relay, auth) {
        Ok(a) => a,
        Err(r) => return Outgoing::json(r),
    };
    let v: Value = serde_json::from_slice(body).unwrap_or(json!({}));
    let gone = if v["all"].as_bool() == Some(true) {
        with_conn(relay, |c| {
            c.execute("DELETE FROM assistants WHERE account = ?1", [&account])
        })
    } else if let Some(id) = v["id"].as_str() {
        with_conn(relay, |c| {
            c.execute(
                "DELETE FROM assistants WHERE account = ?1 AND id = ?2",
                params![account, id],
            )
        })
    } else {
        return Outgoing::json(error_reply(400, "id or all needed"));
    };
    match gone {
        Some(n) => json_out(200, json!({ "revoked": n })),
        None => Outgoing::json(error_reply(500, "server error")),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::cloud::client::Remote;
    use crate::core::db::List;

    fn req(method: &str, url: &str, auth: Option<&str>, body: &str) -> Incoming {
        Incoming {
            method: method.into(),
            url: url.into(),
            auth: auth.map(str::to_string),
            host: Some("tasq.example".into()),
            proto: None,
            body: body.as_bytes().to_vec(),
        }
    }

    fn body_json(out: &Outgoing) -> Value {
        serde_json::from_slice(&out.body).unwrap()
    }

    /// A user with a computer that synced two tasks.
    fn setup() -> (Relay, Keys) {
        let relay = Relay::in_memory().unwrap();
        let keys = Keys::derive("me@x.com", "my long password").unwrap();
        let mut remote = Local {
            relay: &relay,
            keys: keys.clone(),
        };
        remote.signup().unwrap();
        let mut mac = Db::in_memory();
        let today = chrono::Utc::now()
            .with_timezone(&chrono_tz::Europe::Madrid)
            .date_naive();
        let mut live = vec![
            todo::parse_line(&format!("Standup +Work plan:{today} at:00:00 dur:23h59m")).unwrap(),
            todo::parse_line("Buy café beans").unwrap(),
        ];
        mac.save(Some(&mut live), None).unwrap();
        sync_once(&mut mac, &mut remote, &keys, &today.to_string()).unwrap();
        (relay, keys)
    }

    /// Register, sign in, swap the code: the access token.
    fn connect(relay: &Relay, access: &str) -> String {
        let reg = relay.respond(&req(
            "POST",
            "/oauth/register",
            None,
            r#"{"client_name":"Claude","redirect_uris":["https://claude.ai/api/mcp/auth_callback"]}"#,
        ));
        assert_eq!(reg.code, 201);
        let client_id = body_json(&reg)["client_id"].as_str().unwrap().to_string();
        let verifier = "a-very-long-code-verifier-of-at-least-43-characters-xyz";
        let challenge = http::b64url(&http::sha256(verifier.as_bytes()));
        let q = http::form(&[
            ("client_id", &client_id),
            ("redirect_uri", "https://claude.ai/api/mcp/auth_callback"),
            ("response_type", "code"),
            ("code_challenge", &challenge),
            ("code_challenge_method", "S256"),
            ("state", "xyz"),
            ("scope", "tasks:read tasks:write"),
        ]);
        let page = relay.respond(&req("GET", &format!("/oauth/authorize?{q}"), None, ""));
        assert_eq!(page.code, 200);
        assert!(String::from_utf8_lossy(&page.body).contains("Connect Claude to tasq"));

        // The wrong password is turned away on the page.
        let wrong = format!("{q}&email=me%40x.com&password=not+the+password&access={access}");
        let out = relay.respond(&req("POST", "/oauth/authorize", None, &wrong));
        assert_eq!(out.code, 400);
        assert!(String::from_utf8_lossy(&out.body).contains("That password isn"));

        let form = format!(
            "{q}&email=me%40x.com&password=my+long+password&access={access}&tz=Europe%2FMadrid"
        );
        let out = relay.respond(&req("POST", "/oauth/authorize", None, &form));
        assert_eq!(out.code, 302);
        let location = &out.headers.iter().find(|(k, _)| k == "Location").unwrap().1;
        assert!(location.starts_with("https://claude.ai/api/mcp/auth_callback?code="));
        assert!(location.ends_with("&state=xyz"));
        let code = location
            .split("code=")
            .nth(1)
            .unwrap()
            .split('&')
            .next()
            .unwrap();

        // A wrong verifier gets nothing (and burns the code).
        let tok = relay.respond(&req(
            "POST",
            "/oauth/token",
            None,
            &http::form(&[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("code_verifier", verifier),
                ("client_id", &client_id),
                ("redirect_uri", "https://claude.ai/api/mcp/auth_callback"),
            ]),
        ));
        assert_eq!(tok.code, 200, "{}", String::from_utf8_lossy(&tok.body));
        let again = relay.respond(&req(
            "POST",
            "/oauth/token",
            None,
            &http::form(&[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("code_verifier", verifier),
            ]),
        ));
        assert_eq!(again.code, 400);
        let v = body_json(&tok);
        // A refresh gives new tokens; the old access stops working.
        let fresh = relay.respond(&req(
            "POST",
            "/oauth/token",
            None,
            &http::form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", v["refresh_token"].as_str().unwrap()),
            ]),
        ));
        assert_eq!(fresh.code, 200);
        let old = format!("Bearer {}", v["access_token"].as_str().unwrap());
        assert_eq!(relay.respond(&req("POST", "/mcp", Some(&old), "{}")).code, 401);
        body_json(&fresh)["access_token"].as_str().unwrap().to_string()
    }

    fn call(relay: &Relay, token: &str, method: &str, params: Value) -> Value {
        let body = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });
        let out = relay.respond(&req(
            "POST",
            "/mcp",
            Some(&format!("Bearer {token}")),
            &body.to_string(),
        ));
        assert_eq!(out.code, 200);
        body_json(&out)
    }

    #[test]
    fn without_a_token_it_points_to_the_sign_in() {
        let relay = Relay::in_memory().unwrap();
        let out = relay.respond(&req("POST", "/mcp", None, "{}"));
        assert_eq!(out.code, 401);
        let h = &out.headers.iter().find(|(k, _)| k == "WWW-Authenticate").unwrap().1;
        assert!(h.contains("https://tasq.example/.well-known/oauth-protected-resource"));
        let meta = relay.respond(&req("GET", "/.well-known/oauth-authorization-server", None, ""));
        assert_eq!(
            body_json(&meta)["token_endpoint"],
            "https://tasq.example/oauth/token"
        );
        // Redirects must be HTTPS.
        let reg = relay.respond(&req(
            "POST",
            "/oauth/register",
            None,
            r#"{"redirect_uris":["http://evil.example/cb"]}"#,
        ));
        assert_eq!(reg.code, 400);
    }

    #[test]
    fn claude_reads_the_day_and_adds_a_task_the_computers_get() {
        let (relay, keys) = setup();
        let token = connect(&relay, "write");

        let init = call(&relay, &token, "initialize", json!({ "protocolVersion": "2025-06-18" }));
        assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
        let tools = call(&relay, &token, "tools/list", json!({}));
        let names: Vec<&str> = tools["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"add_task") && names.contains(&"now"));

        let now = call(&relay, &token, "tools/call", json!({ "name": "now", "arguments": {} }));
        let v = &now["result"]["structuredContent"];
        assert_eq!(v["timezone"], "Europe/Madrid");
        assert_eq!(v["current"][0]["title"], "Standup");
        assert_eq!(v["current"][0]["space"], "Work");

        let found = call(
            &relay,
            &token,
            "tools/call",
            json!({ "name": "search", "arguments": { "query": "CAFE" } }),
        );
        assert_eq!(found["result"]["structuredContent"]["count"], 1);
        let inbox = call(&relay, &token, "tools/call", json!({ "name": "inbox", "arguments": {} }));
        assert_eq!(inbox["result"]["structuredContent"]["tasks"][0]["title"], "Buy café beans");

        let added = call(
            &relay,
            &token,
            "tools/call",
            json!({ "name": "add_task", "arguments": { "text": "dentista mañana a las 18:00 in work" } }),
        );
        let t = &added["result"]["structuredContent"]["added"];
        assert_eq!(t["title"], "dentista", "{added}");
        assert_eq!(t["time"], "18:00");
        assert_eq!(t["space"], "Work");
        let id = t["id"].as_str().unwrap().to_string();

        let moved = call(
            &relay,
            &token,
            "tools/call",
            json!({ "name": "reschedule_task", "arguments": { "id": id, "time": "19:30" } }),
        );
        assert_eq!(moved["result"]["structuredContent"]["rescheduled"]["time"], "19:30");

        // The computer gets it on its next sync.
        let mut laptop = Db::in_memory();
        let mut remote = Local {
            relay: &relay,
            keys: keys.clone(),
        };
        sync_once(&mut laptop, &mut remote, &keys, "2026-11-08").unwrap();
        let titles: Vec<String> = laptop
            .load(List::Live)
            .unwrap()
            .iter()
            .map(|t| title_of(&t.raw))
            .collect();
        assert!(titles.contains(&"dentista".to_string()), "{titles:?}");

        // The computer sees who's connected and can disconnect it.
        let bearer = format!("Bearer {}", keys.bearer());
        let list = relay.respond(&req("GET", "/v1/assistants", Some(&bearer), ""));
        assert_eq!(body_json(&list)["assistants"][0]["name"], "Claude");
        let out = relay.respond(&req("POST", "/v1/assistants/revoke", Some(&bearer), r#"{"all":true}"#));
        assert_eq!(body_json(&out)["revoked"], 1);
        let body = json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" }).to_string();
        let gone = relay.respond(&req("POST", "/mcp", Some(&format!("Bearer {token}")), &body));
        assert_eq!(gone.code, 401);
    }

    #[test]
    fn a_read_only_connection_cant_add() {
        let (relay, _) = setup();
        let token = connect(&relay, "read");
        let tools = call(&relay, &token, "tools/list", json!({}));
        assert_eq!(tools["result"]["tools"].as_array().unwrap().len(), 4);
        let added = call(
            &relay,
            &token,
            "tools/call",
            json!({ "name": "add_task", "arguments": { "text": "x" } }),
        );
        assert_eq!(added["result"]["isError"], true);
    }

    #[test]
    fn what_the_server_keeps_reads_nothing_without_the_token() {
        let (relay, _) = setup();
        let _token = connect(&relay, "write");
        let conn = relay.conn.lock().unwrap();
        let wrapped: String = conn
            .query_row("SELECT access_wrapped FROM assistants", [], |r| r.get(0))
            .unwrap();
        assert!(keys::unwrap(&wrapped, "some other token").is_none());
    }

    #[test]
    fn forms_decode() {
        let f = parse_form("a=1&b=x+y%2Fz&c=%");
        assert_eq!(field(&f, "b"), Some("x y/z"));
        assert_eq!(field(&f, "c"), Some("%"));
    }
}
