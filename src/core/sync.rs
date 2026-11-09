//! Sync, the local half: what changed on this device since it last synced,
//! and how changes from another device are folded in. No network here —
//! a transport (phase 2) moves [`Change`]s between devices, encrypted.
//!
//! Three kinds of record travel: tasks (by id: the todo.txt line and its
//! list), notes (by path: the body) and spaces (by path: hidden, colour).
//! Each record carries a hybrid logical clock stamp ([`Hlc`]): the time it
//! was edited, a counter, and the device. When two devices changed the same
//! record, the later stamp wins on both — last writer wins, per record —
//! so every device ends up the same whatever order changes arrive in. A
//! deleted record leaves a tombstone, so it isn't brought back by a device
//! that still has the old copy.
//!
//! Local changes are found by comparison, not by hooking every writer:
//! [`Db::sync_scan`] compares each record with the version last sent or
//! received (`sync_state`) and queues what differs (`sync_outbox`). So an
//! edit from the CLI, the phone inbox or another tasq is caught the same
//! way. The order of tasks within a list doesn't travel: it's this
//! device's own.

use std::collections::HashMap;

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};

use super::db::{self, Db, List, io_err, write_row};
use crate::todo;

/// What a record is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    Task,
    Note,
    Space,
    /// Another calendar's account, shown on all your devices.
    Account,
    /// A theme of your own (its file's text), for the web calendar.
    Theme,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Task => "task",
            Kind::Note => "note",
            Kind::Space => "space",
            Kind::Account => "account",
            Kind::Theme => "theme",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "task" => Some(Kind::Task),
            "note" => Some(Kind::Note),
            "space" => Some(Kind::Space),
            "account" => Some(Kind::Account),
            "theme" => Some(Kind::Theme),
            _ => None,
        }
    }
}

/// A hybrid logical clock stamp: milliseconds since the epoch, a counter
/// for changes within the same millisecond (or made on a device whose clock
/// is behind), and the device it comes from to break the last ties. Ordered
/// in that order, which is also the order of [`Hlc::encode`]'s text.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Hlc {
    pub ms: i64,
    pub n: u32,
    pub node: String,
}

impl Hlc {
    /// `0001762430000000.000000.NODE`: sorts as text the way it sorts.
    pub fn encode(&self) -> String {
        format!("{:016}.{:06}.{}", self.ms.max(0), self.n, self.node)
    }

    pub fn decode(s: &str) -> Option<Self> {
        let mut parts = s.splitn(3, '.');
        let ms = parts.next()?.parse().ok()?;
        let n = parts.next()?.parse().ok()?;
        let node = parts.next()?.to_string();
        (!node.is_empty()).then_some(Self { ms, n, node })
    }

    /// The stamp for a change made at `at` on `node` to a record last
    /// stamped `prev`: `at` itself, unless that wouldn't come after `prev`
    /// (a clock behind, or a quick second edit), then just after `prev`.
    fn next(prev: Option<&Hlc>, at: i64, node: &str) -> Hlc {
        let at = Hlc {
            ms: at,
            n: 0,
            node: node.to_string(),
        };
        match prev {
            Some(p) if at <= *p => Hlc {
                ms: p.ms,
                n: p.n + 1,
                node: node.to_string(),
            },
            _ => at,
        }
    }
}

/// One change to one record: its new value (canonical JSON), or `None`
/// when it was deleted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub kind: Kind,
    pub key: String,
    pub hlc: Hlc,
    pub value: Option<String>,
}

impl Change {
    pub fn to_json(&self) -> Value {
        json!({
            "kind": self.kind.as_str(),
            "key": self.key,
            "hlc": self.hlc.encode(),
            "value": self.value,
        })
    }

    pub fn from_json(v: &Value) -> Option<Self> {
        Some(Self {
            kind: Kind::parse(v["kind"].as_str()?)?,
            key: v["key"].as_str()?.to_string(),
            hlc: Hlc::decode(v["hlc"].as_str()?)?,
            value: match &v["value"] {
                Value::Null => None,
                Value::String(s) => Some(s.clone()),
                _ => return None,
            },
        })
    }
}

/// What [`Db::sync_apply`] did with a batch.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ApplyReport {
    /// Changes that were newer and are now in the database.
    pub applied: usize,
    /// Changes older than what's here (or already here).
    pub ignored: usize,
}

// ---------------------------------------------------------------------------
// Canonical values
// ---------------------------------------------------------------------------

fn task_value(raw: &str, list: &str) -> String {
    json!({ "list": list, "raw": raw }).to_string()
}

fn note_value(body: &str) -> String {
    json!({ "body": body }).to_string()
}

fn space_value(hidden: bool, color: Option<&str>) -> String {
    json!({ "color": color, "hidden": hidden }).to_string()
}

/// A value from another device, rebuilt the way this device writes it (so
/// its fingerprint matches what the next scan reads back).
fn canonical(kind: Kind, value: &str) -> Option<String> {
    let v: Value = serde_json::from_str(value).ok()?;
    Some(match kind {
        Kind::Task => task_value(v["raw"].as_str()?, v["list"].as_str()?),
        Kind::Note => note_value(v["body"].as_str()?),
        Kind::Space => space_value(v["hidden"].as_bool()?, v["color"].as_str()),
        // Kept as the other device wrote it: this one stores it the same.
        Kind::Account | Kind::Theme => v.to_string(),
    })
}

/// A stable fingerprint (FNV-1a), the same from one build to the next.
fn fingerprint(s: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

fn rfc3339_ms(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.timestamp_millis())
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Every record as it is now: value, and when it was last edited if the
/// database knows.
type Records = HashMap<(Kind, String), (String, Option<i64>)>;

fn current(conn: &Connection) -> rusqlite::Result<Records> {
    let mut out = Records::new();
    let mut stmt = conn.prepare("SELECT id, raw, list, updated_at FROM tasks")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
        ))
    })?;
    for row in rows {
        let (id, raw, list, updated) = row?;
        out.insert(
            (Kind::Task, id),
            (task_value(&raw, &list), rfc3339_ms(&updated)),
        );
    }
    let mut stmt = conn.prepare("SELECT path, body, updated_at FROM notes")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })?;
    for row in rows {
        let (path, body, updated) = row?;
        out.insert(
            (Kind::Note, path),
            (note_value(&body), rfc3339_ms(&updated)),
        );
    }
    let mut stmt = conn.prepare("SELECT path, hidden, color FROM spaces")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)? != 0,
            r.get::<_, Option<String>>(2)?,
        ))
    })?;
    for row in rows {
        let (path, hidden, color) = row?;
        // Spaces don't keep when they were changed: the scan's time.
        out.insert(
            (Kind::Space, path),
            (space_value(hidden, color.as_deref()), None),
        );
    }
    let mut stmt = conn.prepare("SELECT email, value, updated_at FROM shared_accounts")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })?;
    for row in rows {
        let (email, value, updated) = row?;
        let value = serde_json::from_str::<Value>(&value)
            .map(|v| v.to_string())
            .unwrap_or(value);
        out.insert((Kind::Account, email), (value, rfc3339_ms(&updated)));
    }
    let mut stmt = conn.prepare("SELECT name, value, updated_at FROM shared_themes")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })?;
    for row in rows {
        let (name, value, updated) = row?;
        let value = serde_json::from_str::<Value>(&value)
            .map(|v| v.to_string())
            .unwrap_or(value);
        out.insert((Kind::Theme, name), (value, rfc3339_ms(&updated)));
    }
    Ok(out)
}

/// When each removed record went (see the `sync_deleted` triggers).
fn deleted_at(conn: &Connection) -> rusqlite::Result<HashMap<(Kind, String), i64>> {
    let mut stmt = conn.prepare("SELECT kind, key, at FROM sync_deleted")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })?;
    let mut out = HashMap::new();
    for row in rows {
        let (kind, key, at) = row?;
        if let (Some(kind), Some(at)) = (Kind::parse(&kind), rfc3339_ms(&at)) {
            out.insert((kind, key), at);
        }
    }
    Ok(out)
}

/// Every record's last known version: stamp and fingerprint ("" for a
/// tombstone).
fn known(conn: &Connection) -> rusqlite::Result<HashMap<(Kind, String), (Hlc, String)>> {
    let mut stmt = conn.prepare("SELECT kind, key, hlc, hash FROM sync_state")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
        ))
    })?;
    let mut out = HashMap::new();
    for row in rows {
        let (kind, key, hlc, hash) = row?;
        if let (Some(kind), Some(hlc)) = (Kind::parse(&kind), Hlc::decode(&hlc)) {
            out.insert((kind, key), (hlc, hash));
        }
    }
    Ok(out)
}

fn remember(
    conn: &Connection,
    kind: Kind,
    key: &str,
    hlc: &Hlc,
    hash: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO sync_state (kind, key, hlc, hash) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT (kind, key) DO UPDATE SET hlc = excluded.hlc, hash = excluded.hash",
        params![kind.as_str(), key, hlc.encode(), hash],
    )?;
    Ok(())
}

impl Db {
    /// This device's name in stamps, made the first time it's asked for.
    pub fn sync_node(&self) -> std::io::Result<String> {
        let node: Option<String> = self
            .conn
            .query_row("SELECT value FROM meta WHERE key = 'sync_node'", [], |r| {
                r.get(0)
            })
            .optional()
            .map_err(io_err)?;
        if let Some(n) = node {
            return Ok(n);
        }
        // The random end of a ULID: ten characters, unique enough.
        let n: String = db::new_ulid().chars().skip(16).collect();
        self.conn
            .execute(
                "INSERT INTO meta (key, value) VALUES ('sync_node', ?1)",
                [&n],
            )
            .map_err(io_err)?;
        Ok(n)
    }

    /// Give this device a new name in stamps (when it joins an account: a
    /// database copied from another device would otherwise share its name).
    pub fn sync_reset_node(&mut self) -> std::io::Result<String> {
        self.conn
            .execute("DELETE FROM meta WHERE key = 'sync_node'", [])
            .map_err(io_err)?;
        self.sync_node()
    }

    /// Forget everything sync knew (signing out, or into another account):
    /// the next round starts from nothing and sends every record again.
    /// The tasks themselves stay.
    pub fn sync_forget(&mut self) -> std::io::Result<()> {
        self.conn
            .execute_batch(
                "DELETE FROM sync_state; DELETE FROM sync_outbox; DELETE FROM sync_deleted;
                 DELETE FROM meta WHERE key IN ('sync_cursor', 'sync_node');",
            )
            .map_err(io_err)
    }

    /// How far into the account's log this device has read (0: not yet).
    pub fn sync_cursor(&self) -> std::io::Result<i64> {
        let v: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'sync_cursor'",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(io_err)?;
        Ok(v.and_then(|v| v.parse().ok()).unwrap_or(0))
    }

    pub fn set_sync_cursor(&mut self, seq: i64) -> std::io::Result<()> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO meta (key, value) VALUES ('sync_cursor', ?1)",
                [seq.to_string()],
            )
            .map(|_| ())
            .map_err(io_err)
    }

    /// Queue every record that changed since it was last sent or received.
    /// Returns how many.
    pub fn sync_scan(&mut self) -> std::io::Result<usize> {
        self.sync_scan_at(now_ms())
    }

    fn sync_scan_at(&mut self, now: i64) -> std::io::Result<usize> {
        let node = self.sync_node()?;
        let tx = self.conn.transaction().map_err(io_err)?;
        let known = known(&tx).map_err(io_err)?;
        let current = current(&tx).map_err(io_err)?;
        let mut queued = 0;
        let mut queue = |kind: Kind, key: &str, hlc: &Hlc, value: Option<&str>| {
            let hash = value.map(fingerprint).unwrap_or_default();
            remember(&tx, kind, key, hlc, &hash)?;
            tx.execute(
                "INSERT INTO sync_outbox (kind, key, hlc, value) VALUES (?1, ?2, ?3, ?4)",
                params![kind.as_str(), key, hlc.encode(), value],
            )?;
            queued += 1;
            Ok::<(), rusqlite::Error>(())
        };
        for ((kind, key), (value, edited)) in &current {
            let prev = known.get(&(*kind, key.clone()));
            if prev.is_some_and(|(_, h)| *h == fingerprint(value)) {
                continue;
            }
            let hlc = Hlc::next(prev.map(|p| &p.0), edited.unwrap_or(now), &node);
            queue(*kind, key, &hlc, Some(value)).map_err(io_err)?;
        }
        let gone_at = deleted_at(&tx).map_err(io_err)?;
        for ((kind, key), (prev, hash)) in &known {
            if hash.is_empty() || current.contains_key(&(*kind, key.clone())) {
                continue;
            }
            let at = gone_at.get(&(*kind, key.clone())).copied().unwrap_or(now);
            let hlc = Hlc::next(Some(prev), at, &node);
            queue(*kind, key, &hlc, None).map_err(io_err)?;
        }
        // Every delete is accounted for now (or was of something never sent).
        tx.execute("DELETE FROM sync_deleted", []).map_err(io_err)?;
        tx.commit().map_err(io_err)?;
        Ok(queued)
    }

    /// The changes waiting to be sent, oldest first, with their place in
    /// the queue (for [`Db::sync_ack`]).
    pub fn sync_pending(&self, limit: usize) -> std::io::Result<Vec<(i64, Change)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT seq, kind, key, hlc, value FROM sync_outbox ORDER BY seq LIMIT ?1")
            .map_err(io_err)?;
        let rows = stmt
            .query_map([limit as i64], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                ))
            })
            .map_err(io_err)?;
        let mut out = Vec::new();
        for row in rows {
            let (seq, kind, key, hlc, value) = row.map_err(io_err)?;
            if let (Some(kind), Some(hlc)) = (Kind::parse(&kind), Hlc::decode(&hlc)) {
                out.push((
                    seq,
                    Change {
                        kind,
                        key,
                        hlc,
                        value,
                    },
                ));
            }
        }
        Ok(out)
    }

    /// The changes up to `seq` have been sent: drop them from the queue.
    pub fn sync_ack(&mut self, seq: i64) -> std::io::Result<()> {
        self.conn
            .execute("DELETE FROM sync_outbox WHERE seq <= ?1", [seq])
            .map(|_| ())
            .map_err(io_err)
    }

    /// Fold in changes from other devices: each one newer than what's here
    /// replaces it (a deleted task goes to the trash, dated `today`); older
    /// ones are ignored. Local changes not yet scanned are queued first, so
    /// they're compared, not overwritten unseen.
    ///
    /// The tables change under the app's feet: run it on a connection of
    /// its own, so the app sees another writer and reloads.
    pub fn sync_apply(&mut self, changes: &[Change], today: &str) -> std::io::Result<ApplyReport> {
        self.sync_scan()?;
        let mut changes: Vec<&Change> = changes.iter().collect();
        changes.sort_by(|a, b| a.hlc.cmp(&b.hlc));
        let now = db::now_rfc3339();
        let tx = self.conn.transaction().map_err(io_err)?;
        let mut report = ApplyReport::default();
        for c in changes {
            let here: Option<String> = tx
                .query_row(
                    "SELECT hlc FROM sync_state WHERE kind = ?1 AND key = ?2",
                    params![c.kind.as_str(), c.key],
                    |r| r.get(0),
                )
                .optional()
                .map_err(io_err)?;
            if here
                .and_then(|h| Hlc::decode(&h))
                .is_some_and(|h| h >= c.hlc)
            {
                report.ignored += 1;
                continue;
            }
            let value = match &c.value {
                Some(v) => match canonical(c.kind, v) {
                    Some(v) => Some(v),
                    // Not something this build understands: leave it.
                    None => {
                        report.ignored += 1;
                        continue;
                    }
                },
                None => None,
            };
            apply_one(&tx, c.kind, &c.key, value.as_deref(), today, &now)?;
            let hash = value.as_deref().map(fingerprint).unwrap_or_default();
            remember(&tx, c.kind, &c.key, &c.hlc, &hash).map_err(io_err)?;
            report.applied += 1;
        }
        tx.commit().map_err(io_err)?;
        Ok(report)
    }
}

/// Write one record from another device into the tables.
fn apply_one(
    tx: &rusqlite::Transaction<'_>,
    kind: Kind,
    key: &str,
    value: Option<&str>,
    today: &str,
    now: &str,
) -> std::io::Result<()> {
    let v: Option<Value> = value.and_then(|v| serde_json::from_str(v).ok());
    match (kind, v) {
        (Kind::Task, Some(v)) => {
            let (Some(raw), Some(list)) = (v["raw"].as_str(), v["list"].as_str()) else {
                return Ok(());
            };
            let Ok(mut task) = todo::parse_line(raw) else {
                return Ok(());
            };
            task.id = key.to_string();
            let list_e = if list == "archive" {
                List::Archive
            } else {
                List::Live
            };
            let pos: Option<i64> = tx
                .query_row(
                    "SELECT position FROM tasks WHERE id = ?1 AND list = ?2",
                    params![key, list],
                    |r| r.get(0),
                )
                .optional()
                .map_err(io_err)?;
            let exists: bool = tx
                .query_row("SELECT 1 FROM tasks WHERE id = ?1", [key], |_| Ok(()))
                .optional()
                .map_err(io_err)?
                .is_some();
            // A task new to this list goes at its end.
            let pos = match pos {
                Some(p) => p,
                None => tx
                    .query_row(
                        "SELECT COALESCE(MAX(position) + 1, 0) FROM tasks WHERE list = ?1",
                        [list],
                        |r| r.get(0),
                    )
                    .map_err(io_err)?,
            };
            write_row(tx, &task, list_e, pos, (!exists).then_some(now), now)?;
            if list_e == List::Live {
                db::add_spaces(tx, &task.projects, now).map_err(io_err)?;
            }
        }
        (Kind::Task, None) => {
            let raw: Option<String> = tx
                .query_row("SELECT raw FROM tasks WHERE id = ?1", [key], |r| r.get(0))
                .optional()
                .map_err(io_err)?;
            if let Some(raw) = raw {
                tx.execute(
                    "INSERT INTO trash (id, raw, deleted_on) VALUES (?1, ?2, ?3)",
                    params![db::new_ulid(), raw, today],
                )
                .map_err(io_err)?;
                tx.execute("DELETE FROM tasks WHERE id = ?1", [key])
                    .map_err(io_err)?;
            }
        }
        (Kind::Note, Some(v)) => {
            let Some(body) = v["body"].as_str() else {
                return Ok(());
            };
            tx.execute(
                "INSERT INTO notes (id, path, body, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?4)
                 ON CONFLICT (path) DO UPDATE SET body = excluded.body,
                     updated_at = excluded.updated_at",
                params![db::new_ulid(), key, body, now],
            )
            .map_err(io_err)?;
        }
        (Kind::Note, None) => {
            tx.execute("DELETE FROM notes WHERE path = ?1", [key])
                .map_err(io_err)?;
        }
        (Kind::Space, Some(v)) => {
            tx.execute(
                "INSERT INTO spaces (path, hidden, created_at, color) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (path) DO UPDATE SET hidden = excluded.hidden,
                     color = excluded.color",
                params![
                    key,
                    v["hidden"].as_bool().unwrap_or(false),
                    now,
                    v["color"].as_str()
                ],
            )
            .map_err(io_err)?;
        }
        (Kind::Space, None) => {
            tx.execute("DELETE FROM spaces WHERE path = ?1", [key])
                .map_err(io_err)?;
        }
        (Kind::Account, Some(v)) => {
            tx.execute(
                "INSERT INTO shared_accounts (email, value, updated_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT (email) DO UPDATE SET value = excluded.value,
                     updated_at = excluded.updated_at",
                params![key, v.to_string(), now],
            )
            .map_err(io_err)?;
        }
        (Kind::Account, None) => {
            tx.execute("DELETE FROM shared_accounts WHERE email = ?1", [key])
                .map_err(io_err)?;
        }
        (Kind::Theme, Some(v)) => {
            tx.execute(
                "INSERT INTO shared_themes (name, value, updated_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT (name) DO UPDATE SET value = excluded.value,
                     updated_at = excluded.updated_at",
                params![key, v.to_string(), now],
            )
            .map_err(io_err)?;
        }
        (Kind::Theme, None) => {
            tx.execute("DELETE FROM shared_themes WHERE name = ?1", [key])
                .map_err(io_err)?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::todo::Task;

    /// Everything that syncs on a device: tasks, notes, spaces.
    type State = (
        Vec<(String, String, String)>,
        Vec<(String, String)>,
        Vec<(String, bool, Option<String>)>,
    );

    /// A device: its database and its live list as the app holds it.
    struct Device {
        db: Db,
        live: Vec<Task>,
    }

    impl Device {
        fn new() -> Self {
            Self {
                db: Db::in_memory(),
                live: Vec::new(),
            }
        }

        /// Reload the list from the database (as the app does when another
        /// writer committed).
        fn reload(&mut self) {
            self.live = self.db.load(List::Live).unwrap();
        }

        fn add(&mut self, line: &str) -> String {
            self.live.push(todo::parse_line(line).unwrap());
            self.save();
            self.live.last().unwrap().id.clone()
        }

        fn edit(&mut self, id: &str, line: &str) {
            let i = self.live.iter().position(|t| t.id == id).unwrap();
            let mut t = todo::parse_line(line).unwrap();
            t.id = id.to_string();
            self.live[i] = t;
            self.save();
        }

        fn delete(&mut self, id: &str) {
            self.live.retain(|t| t.id != id);
            self.save();
        }

        fn save(&mut self) {
            self.db.save(Some(&mut self.live), None).unwrap();
            // Edits a few milliseconds apart, so "later" means later.
            std::thread::sleep(std::time::Duration::from_millis(2));
        }

        fn note(&mut self, path: &str, body: &str) {
            self.db
                .conn
                .execute(
                    "INSERT INTO notes (id, path, body, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?4)
                     ON CONFLICT (path) DO UPDATE SET body = excluded.body,
                         updated_at = excluded.updated_at",
                    params![db::new_ulid(), path, body, db::now_rfc3339()],
                )
                .unwrap();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }

        /// Scan and take everything waiting to go.
        fn outgoing(&mut self) -> Vec<Change> {
            self.db.sync_scan().unwrap();
            let pending = self.db.sync_pending(10_000).unwrap();
            if let Some((seq, _)) = pending.last() {
                self.db.sync_ack(*seq).unwrap();
            }
            pending.into_iter().map(|(_, c)| c).collect()
        }

        fn receive(&mut self, changes: &[Change]) -> ApplyReport {
            let r = self.db.sync_apply(changes, "2026-11-06").unwrap();
            self.reload();
            r
        }

        /// Everything that syncs, in a comparable form.
        fn state(&self) -> State {
            let c = &self.db.conn;
            let mut tasks: Vec<(String, String, String)> = c
                .prepare("SELECT id, raw, list FROM tasks")
                .unwrap()
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .unwrap()
                .map(Result::unwrap)
                .collect();
            tasks.sort();
            let mut notes: Vec<(String, String)> = c
                .prepare("SELECT path, body FROM notes")
                .unwrap()
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap()
                .map(Result::unwrap)
                .collect();
            notes.sort();
            let spaces = self.db.load_spaces().unwrap();
            let spaces = spaces
                .into_iter()
                .map(|s| (s.path, s.hidden, s.color))
                .collect();
            (tasks, notes, spaces)
        }

        fn raw(&self, id: &str) -> Option<String> {
            self.live.iter().find(|t| t.id == id).map(|t| t.raw.clone())
        }
    }

    /// Swap changes both ways until neither has anything to send.
    fn sync(a: &mut Device, b: &mut Device) {
        for _ in 0..4 {
            let from_a = a.outgoing();
            let from_b = b.outgoing();
            if from_a.is_empty() && from_b.is_empty() {
                return;
            }
            b.receive(&from_a);
            a.receive(&from_b);
        }
        panic!("sync never settled");
    }

    #[test]
    fn a_shared_account_reaches_the_other_device_and_leaves_it() {
        let mut a = Device::new();
        let mut b = Device::new();
        a.add("Something to start the sync");
        sync(&mut a, &mut b);
        let value = r#"{"calendars":[],"token":"1//x"}"#;
        a.db.put_shared_account("me@work.com", value).unwrap();
        sync(&mut a, &mut b);
        assert_eq!(
            b.db.shared_accounts().unwrap(),
            [("me@work.com".to_string(), value.to_string())]
        );
        // Received, not sent back.
        assert!(b.outgoing().is_empty());
        a.db.drop_shared_account("me@work.com").unwrap();
        sync(&mut a, &mut b);
        assert!(b.db.shared_accounts().unwrap().is_empty());
    }

    #[test]
    fn stamps_sort_as_text_the_way_they_sort() {
        let a = Hlc {
            ms: 9,
            n: 3,
            node: "B".into(),
        };
        let b = Hlc {
            ms: 10,
            n: 0,
            node: "A".into(),
        };
        assert!(a < b && a.encode() < b.encode());
        assert_eq!(Hlc::decode(&a.encode()), Some(a.clone()));
        // A clock behind still moves forward.
        let next = Hlc::next(Some(&b), 5, "C");
        assert!(next > b);
        assert_eq!((next.ms, next.n), (10, 1));
        let c = Change {
            kind: Kind::Task,
            key: "T".into(),
            hlc: b,
            value: None,
        };
        assert_eq!(Change::from_json(&c.to_json()), Some(c));
    }

    #[test]
    fn a_scan_queues_only_what_changed() {
        let mut d = Device::new();
        let id = d.add("Buy milk +Home");
        // The task and its space.
        assert_eq!(d.db.sync_scan().unwrap(), 2);
        assert_eq!(d.db.sync_scan().unwrap(), 0);
        d.edit(&id, "Buy oat milk +Home");
        assert_eq!(d.db.sync_scan().unwrap(), 1);
        d.delete(&id);
        assert_eq!(d.db.sync_scan().unwrap(), 1);
        let last = d.db.sync_pending(10).unwrap().pop().unwrap().1;
        assert_eq!((last.kind, last.value), (Kind::Task, None));
    }

    #[test]
    fn a_new_device_gets_everything() {
        let mut mac = Device::new();
        mac.add("(A) Finish thesis +Uni/Thesis plan:2026-11-10");
        mac.add("Call mom @phone");
        mac.note("tasks/thesis/plan.md", "# Chapter 3");
        mac.db.set_space_color("Uni", Some("#ed8796")).unwrap();
        let mut new = Device::new();
        sync(&mut mac, &mut new);
        assert_eq!(new.state(), mac.state());
        assert_eq!(new.live.len(), 2);
    }

    #[test]
    fn offline_edits_on_both_sides_end_up_the_same_everywhere() {
        let mut a = Device::new();
        let mut b = Device::new();
        let shared = a.add("Pay rent");
        let other = a.add("Book flights");
        sync(&mut a, &mut b);

        // Offline: both edit the same task (b later), a adds one, b deletes one.
        a.edit(&shared, "Pay rent due:2026-12-01");
        b.edit(&shared, "Pay rent and bills due:2026-12-01");
        a.add("Renew passport");
        b.delete(&other);
        b.note("unlinked/ideas.md", "tasq on the phone");

        sync(&mut a, &mut b);
        assert_eq!(a.state(), b.state());
        // The later edit won on both.
        assert_eq!(
            a.raw(&shared).as_deref(),
            Some("Pay rent and bills due:2026-12-01")
        );
        // The delete went through, and into a's trash.
        assert!(a.raw(&other).is_none());
        assert_eq!(a.db.trash_list().unwrap()[0].1, "Book flights");
        assert_eq!(a.live.len(), 2);
    }

    #[test]
    fn the_last_change_wins_between_an_edit_and_a_delete() {
        let mut a = Device::new();
        let mut b = Device::new();
        let id = a.add("Water plants");
        sync(&mut a, &mut b);
        a.delete(&id);
        b.edit(&id, "Water plants every sunday");
        sync(&mut a, &mut b);
        // Edited after the delete: it's back, on both.
        assert_eq!(a.state(), b.state());
        assert!(a.raw(&id).is_some());

        b.edit(&id, "Water the plants");
        a.delete(&id);
        sync(&mut a, &mut b);
        assert_eq!(a.state(), b.state());
        assert!(b.raw(&id).is_none());
    }

    #[test]
    fn the_order_changes_arrive_in_doesnt_matter() {
        let mut a = Device::new();
        let id = a.add("Draft");
        let first = a.outgoing();
        a.edit(&id, "Draft v2");
        let second = a.outgoing();
        a.edit(&id, "Draft v3");
        let third = a.outgoing();

        let mut in_order = Device::new();
        for batch in [&first, &second, &third] {
            in_order.receive(batch);
        }
        let mut shuffled = Device::new();
        for batch in [&third, &first, &second, &third] {
            shuffled.receive(batch);
        }
        assert_eq!(in_order.state(), shuffled.state());
        assert_eq!(in_order.raw(&id).as_deref(), Some("Draft v3"));
        // Old news is ignored, not reapplied.
        let r = shuffled.receive(&first);
        assert_eq!((r.applied, r.ignored), (0, first.len()));
    }

    #[test]
    fn what_was_received_isnt_sent_back() {
        let mut a = Device::new();
        let mut b = Device::new();
        a.add("Only once");
        let changes = a.outgoing();
        b.receive(&changes);
        // b already has exactly this: nothing to say about it.
        assert!(b.outgoing().is_empty());
    }

    /// Three devices change things at random while offline; a server keeps
    /// every change and hands each device the others' in a shuffled order.
    /// However it goes, they all end the same.
    #[test]
    fn three_devices_converge_whatever_the_order() {
        // A tiny deterministic generator: the same run every time.
        struct Rng(u64);
        impl Rng {
            fn next(&mut self, n: usize) -> usize {
                self.0 = self
                    .0
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                ((self.0 >> 33) as usize) % n.max(1)
            }
        }
        for seed in 0..50u64 {
            let mut rng = Rng(seed);
            let mut devices = [Device::new(), Device::new(), Device::new()];
            // What the server holds: every change, and who sent it.
            let mut server: Vec<(usize, Change)> = Vec::new();
            let mut seen = [0usize; 3];
            for round in 0..6 {
                for (d, dev) in devices.iter_mut().enumerate() {
                    for _ in 0..rng.next(4) {
                        let ids: Vec<String> = dev.live.iter().map(|t| t.id.clone()).collect();
                        match rng.next(4) {
                            0 => {
                                dev.add(&format!(
                                    "Task {d}.{round}.{} +S{}",
                                    rng.next(100),
                                    rng.next(3)
                                ));
                            }
                            1 if !ids.is_empty() => {
                                let id = &ids[rng.next(ids.len())];
                                dev.edit(id, &format!("Edited by {d} in {round}"));
                            }
                            2 if !ids.is_empty() => {
                                let id = ids[rng.next(ids.len())].clone();
                                dev.delete(&id);
                            }
                            _ => dev
                                .note(&format!("unlinked/n{}.md", rng.next(3)), &format!("by {d}")),
                        }
                    }
                    for c in dev.outgoing() {
                        server.push((d, c));
                    }
                }
                // Each device takes some of what it hasn't seen, shuffled.
                for (d, dev) in devices.iter_mut().enumerate() {
                    let upto = seen[d] + rng.next(server.len() - seen[d] + 1);
                    let mut batch: Vec<Change> = server[seen[d]..upto]
                        .iter()
                        .filter(|(from, _)| *from != d)
                        .map(|(_, c)| c.clone())
                        .collect();
                    for i in (1..batch.len()).rev() {
                        batch.swap(i, rng.next(i + 1));
                    }
                    dev.receive(&batch);
                    seen[d] = upto;
                }
            }
            // Finally everyone catches up, until nothing is left to send.
            for _ in 0..4 {
                for (d, dev) in devices.iter_mut().enumerate() {
                    let batch: Vec<Change> = server[seen[d]..]
                        .iter()
                        .filter(|(from, _)| *from != d)
                        .map(|(_, c)| c.clone())
                        .collect();
                    dev.receive(&batch);
                    seen[d] = server.len();
                }
                for (d, dev) in devices.iter_mut().enumerate() {
                    for c in dev.outgoing() {
                        server.push((d, c));
                    }
                }
            }
            let first = devices[0].state();
            assert_eq!(devices[1].state(), first, "seed {seed}: device 1 differs");
            assert_eq!(devices[2].state(), first, "seed {seed}: device 2 differs");
        }
    }
}
