//! The local copy of the external events: a small SQLite file of its own
//! (`external.db` in the data directory), apart from the task database so
//! the sync between devices and the calendars tasq writes never see it.
//! It can always be rebuilt from Google, so losing it costs one full sync.
//!
//! Each event is kept as Google sent it, next to its start and end for
//! lookups by day; [`super::event::from_google`] reads it back.

use std::path::{Path, PathBuf};

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;

use super::event::{self, ExternalEvent, Parsed};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS ext_events (
    account   TEXT NOT NULL,
    calendar  TEXT NOT NULL,
    id        TEXT NOT NULL,
    start_utc INTEGER NOT NULL,
    end_utc   INTEGER NOT NULL,
    raw       TEXT NOT NULL,
    PRIMARY KEY (account, calendar, id)
);
CREATE INDEX IF NOT EXISTS ext_events_span ON ext_events (start_utc, end_utc);
CREATE TABLE IF NOT EXISTS ext_sync (
    account    TEXT NOT NULL,
    calendar   TEXT NOT NULL,
    sync_token TEXT,
    last_full  INTEGER,
    last_ok    INTEGER,
    last_error TEXT,
    PRIMARY KEY (account, calendar)
);
";

/// Where the cache lives.
pub fn default_path() -> Option<PathBuf> {
    crate::xdg::data_home().map(|d| d.join("tasq").join("external.db"))
}

/// How a calendar's sync stands.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncMark {
    pub sync_token: Option<String>,
    /// Last full sync, Unix seconds.
    pub last_full: Option<i64>,
    pub last_ok: Option<i64>,
    pub last_error: Option<String>,
}

/// An event in the cache, with the account and calendar it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cached {
    pub account: String,
    pub calendar: String,
    pub event: ExternalEvent,
}

pub struct Cache {
    conn: Connection,
}

fn sql(e: rusqlite::Error) -> anyhow::Error {
    anyhow::anyhow!("external calendar cache: {e}")
}

impl Cache {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        Self::with(Connection::open(path).map_err(sql)?)
    }

    pub fn in_memory() -> Result<Self> {
        Self::with(Connection::open_in_memory().map_err(sql)?)
    }

    fn with(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(sql)?;
        conn.execute_batch(SCHEMA).map_err(sql)?;
        Ok(Self { conn })
    }

    pub fn mark(&self, account: &str, calendar: &str) -> Result<SyncMark> {
        Ok(self
            .conn
            .query_row(
                "SELECT sync_token, last_full, last_ok, last_error FROM ext_sync
                 WHERE account = ?1 AND calendar = ?2",
                params![account, calendar],
                |r| {
                    Ok(SyncMark {
                        sync_token: r.get(0)?,
                        last_full: r.get(1)?,
                        last_ok: r.get(2)?,
                        last_error: r.get(3)?,
                    })
                },
            )
            .optional()
            .map_err(sql)?
            .unwrap_or_default())
    }

    /// Note a failed round, keeping the rest of the mark.
    pub fn failed(&self, account: &str, calendar: &str, error: &str) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO ext_sync (account, calendar, last_error) VALUES (?1, ?2, ?3)
                 ON CONFLICT (account, calendar) DO UPDATE SET last_error = ?3",
                params![account, calendar, error],
            )
            .map_err(sql)?;
        Ok(())
    }

    /// Fold in one round of changes, all at once: on a `full` round the
    /// calendar's old events go first. Events ending before `keep_from`
    /// (Unix seconds) or starting after `keep_to` aren't kept.
    #[allow(clippy::too_many_arguments)]
    pub fn apply(
        &mut self,
        account: &str,
        calendar: &str,
        full: bool,
        items: &[Value],
        (keep_from, keep_to): (i64, i64),
        sync_token: Option<&str>,
        now: i64,
    ) -> Result<(usize, usize)> {
        let tx = self.conn.transaction().map_err(sql)?;
        if full {
            tx.execute(
                "DELETE FROM ext_events WHERE account = ?1 AND calendar = ?2",
                params![account, calendar],
            )
            .map_err(sql)?;
        }
        let (mut kept, mut dropped) = (0, 0);
        for item in items {
            let id = match event::from_google(item) {
                Parsed::Keep(e) => {
                    let (s, end) = e.bounds();
                    if end >= keep_from && s <= keep_to {
                        tx.execute(
                            "INSERT OR REPLACE INTO ext_events
                             (account, calendar, id, start_utc, end_utc, raw)
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                            params![account, calendar, e.id, s, end, item.to_string()],
                        )
                        .map_err(sql)?;
                        kept += 1;
                        continue;
                    }
                    e.id
                }
                Parsed::Drop(id) => id,
                Parsed::Skip => continue,
            };
            dropped += tx
                .execute(
                    "DELETE FROM ext_events WHERE account = ?1 AND calendar = ?2 AND id = ?3",
                    params![account, calendar, id],
                )
                .map_err(sql)?;
        }
        tx.execute(
            "DELETE FROM ext_events WHERE account = ?1 AND calendar = ?2 AND end_utc < ?3",
            params![account, calendar, keep_from],
        )
        .map_err(sql)?;
        tx.execute(
            "INSERT INTO ext_sync (account, calendar, sync_token, last_full, last_ok, last_error)
             VALUES (?1, ?2, ?3, CASE WHEN ?4 THEN ?5 END, ?5, NULL)
             ON CONFLICT (account, calendar) DO UPDATE SET
                 sync_token = ?3,
                 last_full = CASE WHEN ?4 THEN ?5 ELSE last_full END,
                 last_ok = ?5,
                 last_error = NULL",
            params![account, calendar, sync_token, full, now],
        )
        .map_err(sql)?;
        tx.commit().map_err(sql)?;
        Ok((kept, dropped))
    }

    /// Every cached event overlapping `from..to` (Unix seconds).
    pub fn between(&self, from: i64, to: i64) -> Result<Vec<Cached>> {
        let mut st = self
            .conn
            .prepare(
                "SELECT account, calendar, raw FROM ext_events
                 WHERE start_utc < ?2 AND end_utc >= ?1
                 ORDER BY start_utc, id",
            )
            .map_err(sql)?;
        let rows = st
            .query_map(params![from, to], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .map_err(sql)?;
        let mut out = Vec::new();
        for row in rows {
            let (account, calendar, raw) = row.map_err(sql)?;
            let Ok(v) = serde_json::from_str::<Value>(&raw) else {
                continue;
            };
            if let Parsed::Keep(event) = event::from_google(&v) {
                out.push(Cached {
                    account,
                    calendar,
                    event: *event,
                });
            }
        }
        Ok(out)
    }

    /// Forget a calendar (no longer chosen) or, with `None`, a whole
    /// account (disconnected).
    pub fn forget(&self, account: &str, calendar: Option<&str>) -> Result<()> {
        for table in ["ext_events", "ext_sync"] {
            match calendar {
                Some(c) => self.conn.execute(
                    &format!("DELETE FROM {table} WHERE account = ?1 AND calendar = ?2"),
                    params![account, c],
                ),
                None => self.conn.execute(
                    &format!("DELETE FROM {table} WHERE account = ?1"),
                    params![account],
                ),
            }
            .map_err(sql)?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ev(id: &str, start: &str, end: &str) -> Value {
        json!({ "id": id, "summary": id, "start": { "dateTime": start }, "end": { "dateTime": end } })
    }

    #[test]
    fn keeps_what_falls_in_the_window_and_forgets_on_request() {
        let mut c = Cache::in_memory().unwrap();
        let items = [
            ev("a", "2026-11-10T10:00:00Z", "2026-11-10T11:00:00Z"),
            ev("old", "2020-01-01T10:00:00Z", "2020-01-01T11:00:00Z"),
        ];
        let from = 1_700_000_000; // 2023
        let to = 1_900_000_000; // 2030
        assert_eq!(
            c.apply("me@x", "primary", true, &items, (from, to), Some("t1"), 5)
                .unwrap(),
            (1, 0)
        );
        let all = c.between(from, to).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].event.id, "a");
        let m = c.mark("me@x", "primary").unwrap();
        assert_eq!(m.sync_token.as_deref(), Some("t1"));
        assert_eq!((m.last_full, m.last_ok), (Some(5), Some(5)));

        // An incremental round: "a" was cancelled.
        let gone = [json!({ "id": "a", "status": "cancelled" })];
        assert_eq!(
            c.apply("me@x", "primary", false, &gone, (from, to), Some("t2"), 9)
                .unwrap(),
            (0, 1)
        );
        assert!(c.between(from, to).unwrap().is_empty());
        assert_eq!(c.mark("me@x", "primary").unwrap().last_full, Some(5));

        c.failed("me@x", "primary", "offline").unwrap();
        assert_eq!(
            c.mark("me@x", "primary").unwrap().last_error.as_deref(),
            Some("offline")
        );
        c.forget("me@x", None).unwrap();
        assert_eq!(c.mark("me@x", "primary").unwrap(), SyncMark::default());
    }
}
