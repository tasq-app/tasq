//! The sync server (`tasq-relay`): it keeps each account's changes, sealed,
//! in the order they came, and hands them out. It can't read them — they're
//! encrypted on the devices with a key it never sees — so all it knows is
//! an account name (a hash of the email), the hash of its auth key, and how
//! many bytes each device sent when.
//!
//! The API, JSON over HTTP (put it behind HTTPS):
//!
//! - `POST /v1/signup` `{"account", "auth"}`: make the account (again with
//!   the same auth is fine: `200`; someone else's: `409`).
//! - With `Authorization: Bearer <account>.<auth>`:
//!   - `GET /v1/check`: `200` if the keys are right.
//!   - `POST /v1/push` `{"device", "blob"}` → `{"seq"}`.
//!   - `GET /v1/pull?after=N&limit=M` → `{"items": [{"seq", "device",
//!     "blob"}], "more"}`, oldest first.
//! - `GET /health`: `ok`.

use std::sync::Mutex;

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};

use super::keys::auth_hash;

/// The largest blob a device may send (a big first sync is split into
/// batches well under it).
pub const MAX_BLOB: usize = 4 * 1024 * 1024;
const MAX_PULL: i64 = 500;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS accounts (
    id          TEXT PRIMARY KEY,
    auth_hash   TEXT NOT NULL,
    created_at  TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS blobs (
    seq         INTEGER PRIMARY KEY AUTOINCREMENT,
    account     TEXT NOT NULL REFERENCES accounts (id),
    device      TEXT NOT NULL,
    blob        TEXT NOT NULL,
    at          TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS blobs_by_account ON blobs (account, seq);
";

pub struct Relay {
    conn: Mutex<Connection>,
}

/// An answer: status and JSON body.
pub type Reply = (u16, Value);

fn err(code: u16, msg: &str) -> Reply {
    (code, json!({ "error": msg }))
}

fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn now() -> String {
    chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string()
}

impl Relay {
    pub fn open(path: &std::path::Path) -> rusqlite::Result<Self> {
        Self::init(Connection::open(path)?)
    }

    pub fn in_memory() -> rusqlite::Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> rusqlite::Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Answer one request. `path` includes the query string.
    pub fn handle(&self, method: &str, path: &str, auth: Option<&str>, body: &[u8]) -> Reply {
        let (route, query) = path.split_once('?').unwrap_or((path, ""));
        let Ok(conn) = self.conn.lock() else {
            return err(500, "server error");
        };
        let result = match (method, route) {
            ("GET", "/health") => return (200, json!("ok")),
            ("POST", "/v1/signup") => signup(&conn, body),
            ("GET", "/v1/check") => account_of(&conn, auth).map(|_| (200, json!({ "ok": true }))),
            ("POST", "/v1/push") => {
                account_of(&conn, auth).and_then(|account| push(&conn, &account, body))
            }
            ("GET", "/v1/pull") => {
                account_of(&conn, auth).and_then(|account| pull(&conn, &account, query))
            }
            _ => return err(404, "no such endpoint"),
        };
        match result {
            Ok(r) | Err(r) => r,
        }
    }
}

type Step<T> = Result<T, Reply>;

fn db_err(_: rusqlite::Error) -> Reply {
    err(500, "server error")
}

/// The account a bearer token is for, if its auth key is right.
fn account_of(conn: &Connection, auth: Option<&str>) -> Step<String> {
    let token = auth
        .and_then(|a| a.strip_prefix("Bearer "))
        .ok_or_else(|| err(401, "sign in first"))?;
    let (account, key) = token.split_once('.').ok_or_else(|| err(401, "bad token"))?;
    if !is_hex64(account) || !is_hex64(key) {
        return Err(err(401, "bad token"));
    }
    let stored: Option<String> = conn
        .query_row(
            "SELECT auth_hash FROM accounts WHERE id = ?1",
            [account],
            |r| r.get(0),
        )
        .optional()
        .map_err(db_err)?;
    match stored {
        Some(h) if h == auth_hash(key) => Ok(account.to_string()),
        Some(_) => Err(err(401, "wrong password")),
        None => Err(err(401, "no such account")),
    }
}

fn json_body(body: &[u8]) -> Step<Value> {
    serde_json::from_slice(body).map_err(|_| err(400, "expected JSON"))
}

fn signup(conn: &Connection, body: &[u8]) -> Step<Reply> {
    let v = json_body(body)?;
    let (Some(account), Some(auth)) = (v["account"].as_str(), v["auth"].as_str()) else {
        return Err(err(400, "account and auth needed"));
    };
    if !is_hex64(account) || !is_hex64(auth) {
        return Err(err(400, "bad account or auth"));
    }
    let hash = auth_hash(auth);
    let stored: Option<String> = conn
        .query_row(
            "SELECT auth_hash FROM accounts WHERE id = ?1",
            [account],
            |r| r.get(0),
        )
        .optional()
        .map_err(db_err)?;
    match stored {
        Some(h) if h == hash => Ok((200, json!({ "ok": true }))),
        Some(_) => Err(err(
            409,
            "that email already has an account (with another password)",
        )),
        None => {
            conn.execute(
                "INSERT INTO accounts (id, auth_hash, created_at) VALUES (?1, ?2, ?3)",
                params![account, hash, now()],
            )
            .map_err(db_err)?;
            Ok((201, json!({ "ok": true })))
        }
    }
}

fn push(conn: &Connection, account: &str, body: &[u8]) -> Step<Reply> {
    if body.len() > MAX_BLOB + 1024 {
        return Err(err(413, "too big"));
    }
    let v = json_body(body)?;
    let (Some(device), Some(blob)) = (v["device"].as_str(), v["blob"].as_str()) else {
        return Err(err(400, "device and blob needed"));
    };
    if device.is_empty() || device.len() > 64 || blob.is_empty() || blob.len() > MAX_BLOB {
        return Err(err(400, "bad device or blob"));
    }
    conn.execute(
        "INSERT INTO blobs (account, device, blob, at) VALUES (?1, ?2, ?3, ?4)",
        params![account, device, blob, now()],
    )
    .map_err(db_err)?;
    Ok((200, json!({ "seq": conn.last_insert_rowid() })))
}

fn pull(conn: &Connection, account: &str, query: &str) -> Step<Reply> {
    let param = |k: &str| {
        query
            .split('&')
            .find_map(|kv| kv.strip_prefix(k)?.strip_prefix('='))
            .and_then(|v| v.parse::<i64>().ok())
    };
    let after = param("after").unwrap_or(0);
    let limit = param("limit").unwrap_or(MAX_PULL).clamp(1, MAX_PULL);
    let mut stmt = conn
        .prepare(
            "SELECT seq, device, blob FROM blobs WHERE account = ?1 AND seq > ?2
             ORDER BY seq LIMIT ?3",
        )
        .map_err(db_err)?;
    let items: Vec<Value> = stmt
        .query_map(params![account, after, limit + 1], |r| {
            Ok(json!({
                "seq": r.get::<_, i64>(0)?,
                "device": r.get::<_, String>(1)?,
                "blob": r.get::<_, String>(2)?,
            }))
        })
        .map_err(db_err)?
        .collect::<Result<_, _>>()
        .map_err(db_err)?;
    let more = items.len() as i64 > limit;
    let items: Vec<Value> = items.into_iter().take(limit as usize).collect();
    Ok((200, json!({ "items": items, "more": more })))
}

/// Serve on `addr` (e.g. `0.0.0.0:8080`) until killed.
pub fn serve(addr: &str, relay: Relay) -> anyhow::Result<()> {
    let server = tiny_http::Server::http(addr)
        .map_err(|e| anyhow::anyhow!("can't listen on {addr}: {e}"))?;
    serve_on(server, relay)
}

/// Serve on an open `server`, with a few worker threads.
pub fn serve_on(server: tiny_http::Server, relay: Relay) -> anyhow::Result<()> {
    let server = std::sync::Arc::new(server);
    let relay = std::sync::Arc::new(relay);
    let workers: Vec<_> = (0..4)
        .map(|_| {
            let server = server.clone();
            let relay = relay.clone();
            std::thread::spawn(move || {
                for mut req in server.incoming_requests() {
                    let mut body = Vec::new();
                    let too_big = {
                        use std::io::Read;
                        let mut limited = req.as_reader().take((MAX_BLOB + 2048) as u64);
                        limited.read_to_end(&mut body).is_err()
                    };
                    let auth = req
                        .headers()
                        .iter()
                        .find(|h| h.field.equiv("Authorization"))
                        .map(|h| h.value.as_str().to_string());
                    let (code, value) = if too_big {
                        err(400, "couldn't read the request")
                    } else {
                        relay.handle(req.method().as_str(), req.url(), auth.as_deref(), &body)
                    };
                    let mut resp =
                        tiny_http::Response::from_string(value.to_string()).with_status_code(code);
                    if let Ok(h) = tiny_http::Header::from_bytes(
                        &b"Content-Type"[..],
                        &b"application/json"[..],
                    ) {
                        resp.add_header(h);
                    }
                    let _ = req.respond(resp);
                }
            })
        })
        .collect();
    for w in workers {
        let _ = w.join();
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::cloud::keys::Keys;

    fn signup(r: &Relay, k: &Keys) -> u16 {
        let (account, auth) = k
            .bearer()
            .split_once('.')
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .unwrap();
        let body = json!({ "account": account, "auth": auth }).to_string();
        r.handle("POST", "/v1/signup", None, body.as_bytes()).0
    }

    #[test]
    fn accounts_keep_to_themselves() {
        let r = Relay::in_memory().unwrap();
        let me = Keys::derive("me@x.com", "my long password").unwrap();
        let you = Keys::derive("you@x.com", "your long password").unwrap();
        assert_eq!(signup(&r, &me), 201);
        assert_eq!(signup(&r, &me), 200);
        assert_eq!(signup(&r, &you), 201);
        // The same email with another password can't take the account.
        let thief = Keys::derive("me@x.com", "a guessed password").unwrap();
        assert_eq!(signup(&r, &thief), 409);
        let bearer = |k: &Keys| format!("Bearer {}", k.bearer());
        assert_eq!(
            r.handle("GET", "/v1/check", Some(&bearer(&thief)), b"").0,
            401
        );
        assert_eq!(r.handle("GET", "/v1/check", None, b"").0, 401);
        assert_eq!(r.handle("GET", "/v1/check", Some(&bearer(&me)), b"").0, 200);

        let body = json!({ "device": "MAC", "blob": "c2VhbGVk" }).to_string();
        let (code, v) = r.handle("POST", "/v1/push", Some(&bearer(&me)), body.as_bytes());
        assert_eq!(code, 200);
        assert_eq!(v["seq"], 1);
        assert_eq!(
            r.handle("POST", "/v1/push", Some(&bearer(&thief)), body.as_bytes())
                .0,
            401
        );

        let (_, mine) = r.handle("GET", "/v1/pull?after=0", Some(&bearer(&me)), b"");
        assert_eq!(mine["items"][0]["device"], "MAC");
        let (_, yours) = r.handle("GET", "/v1/pull?after=0", Some(&bearer(&you)), b"");
        assert_eq!(yours["items"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn pulls_come_in_pages() {
        let r = Relay::in_memory().unwrap();
        let me = Keys::derive("me@x.com", "my long password").unwrap();
        signup(&r, &me);
        let auth = format!("Bearer {}", me.bearer());
        for i in 0..5 {
            let body = json!({ "device": "D", "blob": format!("blob{i}") }).to_string();
            r.handle("POST", "/v1/push", Some(&auth), body.as_bytes());
        }
        let (_, page) = r.handle("GET", "/v1/pull?after=0&limit=2", Some(&auth), b"");
        assert_eq!(page["items"].as_array().unwrap().len(), 2);
        assert_eq!(page["more"], true);
        let (_, rest) = r.handle("GET", "/v1/pull?after=2&limit=10", Some(&auth), b"");
        assert_eq!(rest["items"][0]["seq"], 3);
        assert_eq!(rest["more"], false);
        assert_eq!(r.handle("GET", "/nope", None, b"").0, 404);
        assert_eq!(
            r.handle("POST", "/v1/push", Some(&auth), b"not json").0,
            400
        );
    }
}
