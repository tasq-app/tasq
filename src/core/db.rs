//! SQLite storage: the local database that is the source of truth for the
//! task list and its archive. todo.txt stays an import/export format.
//!
//! Every task is a row with a stable id (a ULID). The todo.txt line is kept
//! verbatim in `raw` — it is what the app edits — and the fields it carries
//! (title, dates, priority, tags…) are stored next to it as columns, ready
//! for queries, calendar export and sync.
//!
//! Saving writes the whole list in one transaction but only touches rows
//! that changed, so `updated_at` means "last edited". Edits made by another
//! process (a second tasq, the CLI, the phone-capture server) are spotted
//! with SQLite's `data_version`, which moves whenever another connection
//! commits.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, params};

use super::spaces::{self, Space};
use crate::todo::{self, Task};

/// The schema version this build reads and writes.
const SCHEMA_VERSION: i64 = 5;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS tasks (
    id          TEXT PRIMARY KEY,
    list        TEXT NOT NULL CHECK (list IN ('live', 'archive')),
    position    INTEGER NOT NULL,
    raw         TEXT NOT NULL,
    title       TEXT NOT NULL,
    done        INTEGER NOT NULL,
    done_on     TEXT,
    created_on  TEXT,
    priority    TEXT,
    starred     INTEGER NOT NULL,
    due         TEXT,
    show_from   TEXT,
    repeat      TEXT,
    time        TEXT,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS tasks_by_list ON tasks (list, position);
CREATE TABLE IF NOT EXISTS notes (
    id          TEXT PRIMARY KEY,
    path        TEXT NOT NULL UNIQUE,
    body        TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS task_tags (
    task_id TEXT NOT NULL REFERENCES tasks (id) ON DELETE CASCADE,
    kind    TEXT NOT NULL CHECK (kind IN ('project', 'context')),
    name    TEXT NOT NULL,
    PRIMARY KEY (task_id, kind, name)
);
";

/// Which of the two lists a task belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum List {
    Live,
    Archive,
}

impl List {
    fn as_str(self) -> &'static str {
        match self {
            List::Live => "live",
            List::Archive => "archive",
        }
    }
}

pub struct Db {
    conn: Connection,
    path: PathBuf,
    /// `PRAGMA data_version` as of our last read or write.
    data_version: i64,
}

fn io_err(e: rusqlite::Error) -> std::io::Error {
    std::io::Error::other(e)
}

impl Db {
    /// Open (creating if needed) the database at `path`, including any
    /// missing parent directories.
    pub fn open(path: &Path) -> std::io::Result<Self> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path).map_err(io_err)?;
        Self::init(conn, path.to_path_buf())
    }

    /// An in-memory database, for tests.
    #[cfg(test)]
    pub fn in_memory() -> Self {
        #[allow(clippy::unwrap_used)]
        Self::init(
            Connection::open_in_memory().unwrap(),
            PathBuf::from(":memory:"),
        )
        .unwrap()
    }

    fn init(conn: Connection, path: PathBuf) -> std::io::Result<Self> {
        // WAL lets the phone-capture server or the CLI write while the TUI
        // reads; the busy timeout makes concurrent writers wait, not fail.
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(io_err)?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(io_err)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(io_err)?;
        conn.execute_batch(SCHEMA).map_err(io_err)?;
        let version: Option<String> = conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'schema_version'",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(io_err)?;
        // `SCHEMA` creates the version-1 tables; later versions are steps.
        let stored = version.is_some();
        let version = version.and_then(|v| v.parse::<i64>().ok()).unwrap_or(1);
        if version > SCHEMA_VERSION {
            return Err(std::io::Error::other(format!(
                "{} was written by a newer tasq (schema {version}); update tasq to open it",
                path.display()
            )));
        }
        if version < SCHEMA_VERSION || !stored {
            migrate(&conn, version).map_err(io_err)?;
        }
        let mut db = Self {
            conn,
            path,
            data_version: 0,
        };
        db.data_version = db.read_data_version().map_err(io_err)?;
        Ok(db)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Put deleted task lines in the trash, dated `on`.
    pub fn trash_put(&mut self, raws: &[String], on: &str) -> std::io::Result<()> {
        for raw in raws {
            self.conn
                .execute(
                    "INSERT INTO trash (id, raw, deleted_on) VALUES (?1, ?2, ?3)",
                    params![new_ulid(), raw, on],
                )
                .map_err(io_err)?;
        }
        Ok(())
    }

    /// Everything in the trash, newest first: `(id, raw, deleted_on)`.
    pub fn trash_list(&self) -> std::io::Result<Vec<(String, String, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, raw, deleted_on FROM trash ORDER BY deleted_on DESC, id DESC")
            .map_err(io_err)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map_err(io_err)?;
        rows.collect::<Result<_, _>>().map_err(io_err)
    }

    /// Drop trash rows: one by id, or every one deleted before `before`.
    pub fn trash_remove(&mut self, id: Option<&str>, before: Option<&str>) -> std::io::Result<()> {
        match (id, before) {
            (Some(id), _) => self.conn.execute("DELETE FROM trash WHERE id = ?1", [id]),
            (None, Some(d)) => self
                .conn
                .execute("DELETE FROM trash WHERE deleted_on < ?1", [d]),
            (None, None) => self.conn.execute("DELETE FROM trash", []),
        }
        .map(|_| ())
        .map_err(io_err)
    }

    fn read_data_version(&self) -> rusqlite::Result<i64> {
        self.conn.query_row("PRAGMA data_version", [], |r| r.get(0))
    }

    /// True when another connection has committed since our last look.
    pub fn changed_externally(&mut self) -> std::io::Result<bool> {
        let v = self.read_data_version().map_err(io_err)?;
        if v == self.data_version {
            return Ok(false);
        }
        self.data_version = v;
        Ok(true)
    }

    /// True when the database holds no task at all, live or archived.
    pub fn is_empty(&self) -> std::io::Result<bool> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM tasks", [], |r| r.get(0))
            .map_err(io_err)?;
        Ok(n == 0)
    }

    /// The tasks of `list`, in order. Rows whose line no longer parses are
    /// skipped (it can only happen if the file was edited by hand).
    pub fn load(&self, list: List) -> std::io::Result<Vec<Task>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, raw FROM tasks WHERE list = ?1 ORDER BY position")
            .map_err(io_err)?;
        let rows = stmt
            .query_map([list.as_str()], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(io_err)?;
        let mut out = Vec::new();
        for row in rows {
            let (id, raw) = row.map_err(io_err)?;
            if let Ok(mut task) = todo::parse_line(&raw) {
                task.id = id;
                out.push(task);
            }
        }
        Ok(out)
    }

    /// Write `live` and/or `archive` as the full contents of those lists, in
    /// one transaction. Tasks without an id get one (written back into the
    /// slice). Rows of a given list that are not in it are deleted — unless
    /// the task just moved to the other list, which is a plain update.
    pub fn save(
        &mut self,
        live: Option<&mut [Task]>,
        archive: Option<&mut [Task]>,
    ) -> std::io::Result<()> {
        let now = now_rfc3339();
        let tx = self.conn.transaction().map_err(io_err)?;
        let existing: HashMap<String, (String, String, i64)> = {
            let mut stmt = tx
                .prepare("SELECT id, list, raw, position FROM tasks")
                .map_err(io_err)?;
            let rows = stmt
                .query_map([], |r| {
                    Ok((r.get::<_, String>(0)?, (r.get(1)?, r.get(2)?, r.get(3)?)))
                })
                .map_err(io_err)?;
            rows.collect::<Result<_, _>>().map_err(io_err)?
        };
        // Every space a live task uses gets a row of its own, so it stays
        // around (empty) after its last task is gone.
        let live_spaces: Option<Vec<String>> = live
            .as_deref()
            .map(|tasks| tasks.iter().flat_map(|t| t.projects.clone()).collect());
        let mut kept: HashSet<String> = HashSet::new();
        let mut written: Vec<List> = Vec::new();
        for (list, tasks) in [(List::Live, live), (List::Archive, archive)] {
            let Some(tasks) = tasks else { continue };
            written.push(list);
            for (pos, task) in tasks.iter_mut().enumerate() {
                if task.id.is_empty() || kept.contains(&task.id) {
                    // A duplicate id (a copied task) gets its own.
                    task.id = new_ulid();
                }
                kept.insert(task.id.clone());
                let pos = pos as i64;
                match existing.get(&task.id) {
                    Some((l, raw, p)) if l == list.as_str() && *raw == task.raw && *p == pos => {}
                    Some((_, raw, _)) if *raw == task.raw => {
                        tx.execute(
                            "UPDATE tasks SET list = ?2, position = ?3 WHERE id = ?1",
                            params![task.id, list.as_str(), pos],
                        )
                        .map_err(io_err)?;
                    }
                    Some(_) => write_row(&tx, task, list, pos, None, &now)?,
                    None => write_row(&tx, task, list, pos, Some(&now), &now)?,
                }
            }
        }
        for (id, (l, _, _)) in &existing {
            let in_written = written.iter().any(|w| w.as_str() == l);
            if in_written && !kept.contains(id) {
                tx.execute("DELETE FROM tasks WHERE id = ?1", [id])
                    .map_err(io_err)?;
            }
        }
        if let Some(live) = live_spaces {
            add_spaces(&tx, &live, &now).map_err(io_err)?;
        }
        tx.commit().map_err(io_err)?;
        // Our own commit doesn't move our data_version; re-read anyway so a
        // commit from elsewhere that landed just before ours isn't mistaken
        // for news later (we've just overwritten the lists with our view).
        self.data_version = self.read_data_version().map_err(io_err)?;
        Ok(())
    }
}

impl Db {
    /// Every space kept in the database, by path.
    pub fn load_spaces(&self) -> std::io::Result<Vec<Space>> {
        let mut stmt = self
            .conn
            .prepare("SELECT path, hidden, color FROM spaces ORDER BY path")
            .map_err(io_err)?;
        let rows = stmt
            .query_map([], |r| {
                Ok(Space {
                    path: r.get(0)?,
                    hidden: r.get::<_, i64>(1)? != 0,
                    color: r.get(2)?,
                })
            })
            .map_err(io_err)?;
        rows.collect::<Result<_, _>>().map_err(io_err)
    }

    /// Hide or show the space `path` (its sub-spaces follow it in the
    /// views; their own setting is kept).
    pub fn set_space_hidden(&mut self, path: &str, hidden: bool) -> std::io::Result<()> {
        add_spaces(&self.conn, &[path.to_string()], &now_rfc3339()).map_err(io_err)?;
        self.conn
            .execute(
                "UPDATE spaces SET hidden = ?2 WHERE path = ?1",
                params![path, hidden],
            )
            .map_err(io_err)?;
        self.data_version = self.read_data_version().map_err(io_err)?;
        Ok(())
    }

    /// Set the colour of the space `path` (see `SpaceColor`), or go back
    /// to the automatic one with `None`.
    pub fn set_space_color(&mut self, path: &str, color: Option<&str>) -> std::io::Result<()> {
        add_spaces(&self.conn, &[path.to_string()], &now_rfc3339()).map_err(io_err)?;
        self.conn
            .execute(
                "UPDATE spaces SET color = ?2 WHERE path = ?1",
                params![path, color],
            )
            .map_err(io_err)?;
        self.data_version = self.read_data_version().map_err(io_err)?;
        Ok(())
    }

    /// Make the kept spaces exactly `spaces` (undo).
    pub fn replace_spaces(&mut self, spaces: &[Space]) -> std::io::Result<()> {
        let now = now_rfc3339();
        let tx = self.conn.transaction().map_err(io_err)?;
        tx.execute("DELETE FROM spaces", []).map_err(io_err)?;
        for s in spaces {
            tx.execute(
                "INSERT INTO spaces (path, hidden, created_at, color) VALUES (?1, ?2, ?3, ?4)",
                params![s.path, s.hidden, now, s.color],
            )
            .map_err(io_err)?;
        }
        tx.commit().map_err(io_err)?;
        self.data_version = self.read_data_version().map_err(io_err)?;
        Ok(())
    }

    /// Forget the space `path` and its sub-spaces.
    pub fn delete_space(&mut self, path: &str) -> std::io::Result<()> {
        self.conn
            .execute(&format!("DELETE FROM spaces WHERE {WITHIN}"), [path])
            .map_err(io_err)?;
        self.data_version = self.read_data_version().map_err(io_err)?;
        Ok(())
    }

    /// Rename the space `from` (and so its sub-spaces) to `to`, keeping
    /// their settings. A space that already exists under the new name is
    /// merged into.
    pub fn rename_space(&mut self, from: &str, to: &str) -> std::io::Result<()> {
        let tx = self.conn.transaction().map_err(io_err)?;
        type Row = (String, i64, String, Option<String>);
        let rows: Vec<Row> = {
            let mut stmt = tx
                .prepare(&format!(
                    "SELECT path, hidden, created_at, color FROM spaces WHERE {WITHIN}"
                ))
                .map_err(io_err)?;
            stmt.query_map([from], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
                .map_err(io_err)?
                .collect::<Result<_, _>>()
                .map_err(io_err)?
        };
        tx.execute(&format!("DELETE FROM spaces WHERE {WITHIN}"), [from])
            .map_err(io_err)?;
        for (path, hidden, created_at, color) in rows {
            let Some(new_path) = spaces::renamed(&path, from, to) else {
                continue;
            };
            tx.execute(
                "INSERT OR IGNORE INTO spaces (path, hidden, created_at, color)
                 VALUES (?1, ?2, ?3, ?4)",
                params![new_path, hidden, created_at, color],
            )
            .map_err(io_err)?;
        }
        add_spaces(&tx, &[to.to_string()], &now_rfc3339()).map_err(io_err)?;
        tx.commit().map_err(io_err)?;
        self.data_version = self.read_data_version().map_err(io_err)?;
        Ok(())
    }
}

/// SQL condition: `path` is the space `?1` or inside it. Spelled with
/// `substr` rather than `LIKE`, whose `%` and `_` could be in a name.
const WITHIN: &str = "(path = ?1 OR substr(path, 1, length(?1) + 1) = ?1 || '/')";

/// Make sure each space in `paths`, and every space above it, has a row.
fn add_spaces(conn: &Connection, paths: &[String], now: &str) -> rusqlite::Result<()> {
    let mut stmt =
        conn.prepare("INSERT OR IGNORE INTO spaces (path, hidden, created_at) VALUES (?1, 0, ?2)")?;
    for path in paths {
        for p in spaces::with_ancestors(path) {
            stmt.execute(params![p, now])?;
        }
    }
    Ok(())
}

/// Bring a database at `from` up to [`SCHEMA_VERSION`], in one transaction.
fn migrate(conn: &Connection, from: i64) -> rusqlite::Result<()> {
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let result = (|| {
        if from < 2 {
            // Planned date, duration and reminders as columns, filled in
            // from the stored lines.
            conn.execute_batch(
                "ALTER TABLE tasks ADD COLUMN planned TEXT;
                 ALTER TABLE tasks ADD COLUMN duration_min INTEGER;
                 ALTER TABLE tasks ADD COLUMN reminders TEXT;",
            )?;
            let rows: Vec<(String, String)> = {
                let mut stmt = conn.prepare("SELECT id, raw FROM tasks")?;
                stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .collect::<Result<_, _>>()?
            };
            for (id, raw) in rows {
                if let Ok(task) = todo::parse_line(&raw) {
                    let (planned, duration, reminders) = extra_columns(&task);
                    conn.execute(
                        "UPDATE tasks SET planned = ?2, duration_min = ?3, reminders = ?4
                         WHERE id = ?1",
                        params![id, planned, duration, reminders],
                    )?;
                }
            }
        }
        if from < 3 {
            // Spaces as rows of their own, seeded from the live tasks.
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS spaces (
                     path        TEXT PRIMARY KEY,
                     hidden      INTEGER NOT NULL DEFAULT 0,
                     created_at  TEXT NOT NULL
                 );",
            )?;
            let raws: Vec<String> = {
                let mut stmt = conn.prepare("SELECT raw FROM tasks WHERE list = 'live'")?;
                stmt.query_map([], |r| r.get(0))?
                    .collect::<Result<_, _>>()?
            };
            let paths: Vec<String> = raws
                .iter()
                .filter_map(|raw| todo::parse_line(raw).ok())
                .flat_map(|t| t.projects)
                .collect();
            add_spaces(conn, &paths, &now_rfc3339())?;
        }
        if from < 4 {
            // A colour per space (a palette slot or `#rrggbb`).
            conn.execute_batch("ALTER TABLE spaces ADD COLUMN color TEXT;")?;
        }
        if from < 5 {
            // Deleted tasks, kept for a while so they can come back.
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS trash (
                     id          TEXT PRIMARY KEY,
                     raw         TEXT NOT NULL,
                     deleted_on  TEXT NOT NULL
                 );",
            )?;
        }
        conn.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES ('schema_version', ?1)",
            [SCHEMA_VERSION.to_string()],
        )?;
        Ok(())
    })();
    match result {
        Ok(()) => conn.execute_batch("COMMIT"),
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(e)
        }
    }
}

/// The columns added in schema 2: planned date, duration in minutes and
/// reminders (minutes before, comma-separated).
fn extra_columns(task: &Task) -> (Option<String>, Option<u32>, Option<String>) {
    let duration = task
        .duration
        .as_deref()
        .and_then(crate::duration::parse_minutes);
    let reminders = task
        .reminders
        .as_deref()
        .and_then(crate::duration::parse_reminders)
        .map(|list| {
            list.iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(",")
        });
    (task.planned.clone(), duration, reminders)
}

/// Insert or fully rewrite one task row (and its tags). `created` is set for
/// a new row; an existing row keeps its `created_at`.
fn write_row(
    tx: &rusqlite::Transaction<'_>,
    task: &Task,
    list: List,
    pos: i64,
    created: Option<&str>,
    now: &str,
) -> std::io::Result<()> {
    let time = todo::find_kv(todo::body_after_priority(&task.raw), "at");
    let priority = task.priority.map(|c| c.to_string());
    let title = title_of(&task.raw);
    let (planned, duration, reminders) = extra_columns(task);
    match created {
        Some(created) => tx.execute(
            "INSERT INTO tasks (id, list, position, raw, title, done, done_on, created_on,
                 priority, starred, due, show_from, repeat, time, created_at, updated_at,
                 planned, duration_min, reminders)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                 ?17, ?18, ?19)",
            params![
                task.id,
                list.as_str(),
                pos,
                task.raw,
                title,
                task.done,
                task.done_date,
                task.created_date,
                priority,
                task.starred,
                task.due,
                task.threshold,
                task.rec,
                time,
                created,
                now,
                planned,
                duration,
                reminders
            ],
        ),
        None => tx.execute(
            "UPDATE tasks SET list = ?2, position = ?3, raw = ?4, title = ?5, done = ?6,
                 done_on = ?7, created_on = ?8, priority = ?9, starred = ?10, due = ?11,
                 show_from = ?12, repeat = ?13, time = ?14, updated_at = ?15,
                 planned = ?16, duration_min = ?17, reminders = ?18
             WHERE id = ?1",
            params![
                task.id,
                list.as_str(),
                pos,
                task.raw,
                title,
                task.done,
                task.done_date,
                task.created_date,
                priority,
                task.starred,
                task.due,
                task.threshold,
                task.rec,
                time,
                now,
                planned,
                duration,
                reminders
            ],
        ),
    }
    .map_err(io_err)?;
    tx.execute("DELETE FROM task_tags WHERE task_id = ?1", [&task.id])
        .map_err(io_err)?;
    for (kind, names) in [("project", &task.projects), ("context", &task.contexts)] {
        for name in names {
            tx.execute(
                "INSERT OR IGNORE INTO task_tags (task_id, kind, name) VALUES (?1, ?2, ?3)",
                params![task.id, kind, name],
            )
            .map_err(io_err)?;
        }
    }
    Ok(())
}

/// The task's words without the todo.txt markup: no completion mark, dates,
/// priority, `+project`, `@context` or `key:value` tags.
pub fn title_of(raw: &str) -> String {
    todo::body_after_priority(raw)
        .split_whitespace()
        .filter(|tok| {
            let tag = (tok.starts_with('+') || tok.starts_with('@')) && tok.len() > 1;
            let kv = tok.split_once(':').is_some_and(|(k, v)| {
                todo::is_valid_key(k) && !v.is_empty() && !v.starts_with("//")
            });
            !tag && !kv
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Current UTC time as RFC 3339 with milliseconds.
pub(crate) fn now_rfc3339() -> String {
    chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string()
}

/// A new ULID: 48 bits of milliseconds since the epoch, 80 random bits,
/// Crockford base32. Sorts by creation time, unique without coordination.
pub fn new_ulid() -> String {
    const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let ms = chrono::Utc::now().timestamp_millis().max(0) as u128;
    let mut rand = [0u8; 10];
    // A failing OS RNG is not worth crashing over; the timestamp still
    // spreads ids, and duplicates are re-rolled on save.
    let _ = getrandom::getrandom(&mut rand);
    let mut n: u128 = ms << 80;
    for (i, b) in rand.iter().enumerate() {
        n |= u128::from(*b) << (8 * (9 - i));
    }
    (0..26)
        .map(|i| ALPHABET[((n >> (5 * (25 - i))) & 31) as usize] as char)
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn tasks(lines: &[&str]) -> Vec<Task> {
        lines.iter().map(|l| todo::parse_line(l).unwrap()).collect()
    }

    fn updated(db: &Db, id: &str) -> String {
        db.conn
            .query_row("SELECT updated_at FROM tasks WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .unwrap()
    }

    #[test]
    fn ulids_are_26_chars_and_distinct() {
        let a = new_ulid();
        let b = new_ulid();
        assert_eq!(a.len(), 26);
        assert_ne!(a, b);
        assert!(a.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn save_assigns_ids_and_load_round_trips() {
        let mut db = Db::in_memory();
        let mut live = tasks(&[
            "(A) Pay rent +home @bank due:2026-06-01",
            "Call anna at:18:00",
        ]);
        db.save(Some(&mut live), None).unwrap();
        assert!(live.iter().all(|t| t.id.len() == 26));
        let back = db.load(List::Live).unwrap();
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].id, live[0].id);
        assert_eq!(back[0].raw, live[0].raw);
        let (title, time): (String, Option<String>) = db
            .conn
            .query_row(
                "SELECT title, time FROM tasks WHERE id = ?1",
                [&live[1].id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(title, "Call anna");
        assert_eq!(time.as_deref(), Some("18:00"));
        let tags: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM task_tags", [], |r| r.get(0))
            .unwrap();
        assert_eq!(tags, 2);
    }

    #[test]
    fn unchanged_rows_keep_their_updated_at_and_removed_rows_go() {
        let mut db = Db::in_memory();
        let mut live = tasks(&["one", "two", "three"]);
        db.save(Some(&mut live), None).unwrap();
        let before = updated(&db, &live[0].id);
        std::thread::sleep(std::time::Duration::from_millis(5));
        // Edit "two", drop "three", reorder.
        let mut edited = todo::parse_line("two, edited").unwrap();
        edited.id = live[1].id.clone();
        let mut next = vec![edited, live[0].clone()];
        db.save(Some(&mut next), None).unwrap();
        assert_eq!(updated(&db, &live[0].id), before, "only moved");
        assert_ne!(updated(&db, &live[1].id), before, "edited");
        let back = db.load(List::Live).unwrap();
        assert_eq!(
            back.iter().map(|t| t.raw.as_str()).collect::<Vec<_>>(),
            ["two, edited", "one"]
        );
    }

    #[test]
    fn archiving_moves_rows_between_lists() {
        let mut db = Db::in_memory();
        let mut live = tasks(&["x 2026-10-03 2026-10-01 done thing", "open thing"]);
        db.save(Some(&mut live), None).unwrap();
        let mut archive = vec![live[0].clone()];
        let mut remaining = vec![live[1].clone()];
        db.save(Some(&mut remaining), Some(&mut archive)).unwrap();
        assert_eq!(db.load(List::Live).unwrap().len(), 1);
        let arch = db.load(List::Archive).unwrap();
        assert_eq!(arch.len(), 1);
        assert_eq!(arch[0].id, live[0].id, "same row, now archived");
    }

    #[test]
    fn duplicate_ids_are_split() {
        let mut db = Db::in_memory();
        let mut live = tasks(&["a"]);
        db.save(Some(&mut live), None).unwrap();
        let mut twice = vec![live[0].clone(), live[0].clone()];
        db.save(Some(&mut twice), None).unwrap();
        assert_ne!(twice[0].id, twice[1].id);
        assert_eq!(db.load(List::Live).unwrap().len(), 2);
    }

    #[test]
    fn another_connection_is_noticed() {
        let dir = std::env::temp_dir().join(format!("tasq-db-test-{}", new_ulid()));
        let path = dir.join("tasq.db");
        let mut a = Db::open(&path).unwrap();
        let mut b = Db::open(&path).unwrap();
        assert!(!a.changed_externally().unwrap());
        let mut live = tasks(&["from b"]);
        b.save(Some(&mut live), None).unwrap();
        assert!(a.changed_externally().unwrap());
        assert!(!a.changed_externally().unwrap(), "only once");
        let mut mine = a.load(List::Live).unwrap();
        mine.push(todo::parse_line("from a").unwrap());
        a.save(Some(&mut mine), None).unwrap();
        assert!(!a.changed_externally().unwrap(), "own writes aren't news");
        assert_eq!(b.load(List::Live).unwrap().len(), 2);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_version_1_database_is_migrated() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        conn.execute(
            "INSERT INTO meta (key, value) VALUES ('schema_version', '1')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO tasks (id, list, position, raw, title, done, starred, created_at, updated_at)
             VALUES ('A', 'live', 0, 'Gym plan:2026-10-05 dur:1h remind:15m,1d +Health/Gym', 'Gym', 0, 0, 'x', 'x')",
            [],
        )
        .unwrap();
        let db = Db::init(conn, PathBuf::from(":memory:")).unwrap();
        let row: (Option<String>, Option<i64>, Option<String>, String) = db
            .conn
            .query_row(
                "SELECT planned, duration_min, reminders,
                     (SELECT value FROM meta WHERE key = 'schema_version') FROM tasks",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            row,
            (
                Some("2026-10-05".into()),
                Some(60),
                Some("15,1440".into()),
                "5".into()
            )
        );
        let paths: Vec<String> = db
            .load_spaces()
            .unwrap()
            .into_iter()
            .map(|s| s.path)
            .collect();
        assert_eq!(paths, ["Health", "Health/Gym"]);
    }

    fn space_paths(db: &Db) -> Vec<String> {
        db.load_spaces()
            .unwrap()
            .into_iter()
            .map(|s| s.path)
            .collect()
    }

    #[test]
    fn spaces_outlive_their_tasks() {
        let mut db = Db::in_memory();
        let mut live = tasks(&["study +Uni/Exams", "boxes +Personal"]);
        db.save(Some(&mut live), None).unwrap();
        assert_eq!(space_paths(&db), ["Personal", "Uni", "Uni/Exams"]);
        // The last Exams task goes; the space stays.
        let mut live = vec![live[1].clone()];
        db.save(Some(&mut live), None).unwrap();
        assert_eq!(space_paths(&db), ["Personal", "Uni", "Uni/Exams"]);
        db.delete_space("Uni").unwrap();
        assert_eq!(space_paths(&db), ["Personal"]);
    }

    #[test]
    fn renaming_a_space_moves_its_sub_spaces_and_settings() {
        let mut db = Db::in_memory();
        let mut live = tasks(&["study +Uni/Exams", "a +Unicorn"]);
        db.save(Some(&mut live), None).unwrap();
        db.conn
            .execute("UPDATE spaces SET hidden = 1 WHERE path = 'Uni/Exams'", [])
            .unwrap();
        db.rename_space("Uni", "School").unwrap();
        let spaces = db.load_spaces().unwrap();
        let rows: Vec<(&str, bool)> = spaces.iter().map(|s| (s.path.as_str(), s.hidden)).collect();
        assert_eq!(
            rows,
            [
                ("School", false),
                ("School/Exams", true),
                ("Unicorn", false)
            ]
        );
    }

    #[test]
    fn titles_drop_markup() {
        assert_eq!(
            title_of("x 2026-10-03 2026-10-01 Pay rent +home @bank due:2026-06-01 see https://x.y"),
            "Pay rent see https://x.y"
        );
        assert_eq!(
            title_of("(A) 2026-10-01 Call anna @calls at:18:00"),
            "Call anna"
        );
    }

    // ---- the Store on a database ----

    use crate::core::Store;

    fn raws(tasks: &[Task]) -> Vec<&str> {
        tasks.iter().map(|t| t.raw.as_str()).collect()
    }

    #[test]
    fn store_add_complete_archive_undo() {
        let mut s = Store::in_memory_db("2026-10-03");
        s.add_finalized("Pay rent due:2026-10-05 rec:+1m");
        s.add_finalized("Call anna");
        assert!(s.tasks().iter().all(|t| !t.id.is_empty()));
        let rent = s.tasks()[0].id.clone();

        // Completing the recurring task spawns the next one with a new id.
        s.toggle_complete(0);
        assert_eq!(s.tasks().len(), 3);
        assert_eq!(s.tasks()[0].id, rent, "completion keeps the id");
        assert!(s.tasks()[0].done);
        assert_ne!(s.tasks()[1].id, rent);
        assert!(
            s.tasks()[1].raw.contains("due:2026-11-05"),
            "{:?}",
            raws(s.tasks())
        );

        s.archive_completed();
        assert_eq!(s.tasks().len(), 2);
        assert_eq!(s.archive().tasks()[0].id, rent);
        let db = s.db.as_ref().unwrap();
        assert_eq!(db.load(List::Archive).unwrap()[0].id, rent);
        assert_eq!(db.load(List::Live).unwrap().len(), 2);

        // Undo brings it back to the live list, out of the archive.
        s.undo();
        assert_eq!(s.tasks().len(), 3);
        assert!(s.archive().tasks().is_empty());
        let db = s.db.as_ref().unwrap();
        assert!(db.load(List::Archive).unwrap().is_empty());
        assert_eq!(db.load(List::Live).unwrap().len(), 3);
    }

    #[test]
    fn store_edits_keep_ids_and_unarchive_round_trips() {
        let mut s = Store::in_memory_db("2026-10-03");
        s.add_finalized("Buy milk");
        let id = s.tasks()[0].id.clone();
        s.edit_line(0, "Buy oat milk +shop");
        s.set_priority_at(0, Some('A'));
        s.add_project(0, "home");
        assert_eq!(s.tasks()[0].id, id);
        s.toggle_complete(0);
        s.archive_completed();
        assert!(s.tasks().is_empty());
        s.unarchive(0);
        assert_eq!(s.tasks()[0].id, id);
        assert!(!s.tasks()[0].done);
        let live = s.db.as_ref().unwrap().load(List::Live).unwrap();
        assert_eq!(live[0].id, id);
        assert_eq!(live[0].raw, s.tasks()[0].raw);
    }

    #[test]
    fn the_trash_lives_in_the_database() {
        let dir = std::env::temp_dir().join(format!("tasq-trash-test-{}", new_ulid()));
        let path = dir.join("tasq.db");
        let mut s = Store::open_db(path.clone(), "2026-10-03".into()).unwrap();
        s.add_finalized("one");
        s.add_finalized("two");
        s.delete(0);
        let t = s.trash();
        assert_eq!(t.len(), 1);
        assert!(t[0].raw.ends_with("one"));
        assert_eq!(t[0].deleted_on, "2026-10-03");
        // Kept across opens, and gone after thirty days.
        let mut later = Store::open_db(path.clone(), "2026-11-03".into()).unwrap();
        assert_eq!(later.trash().len(), 1);
        later.trash_purge_old();
        assert!(later.trash().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn store_import_and_external_reload() {
        let dir = std::env::temp_dir().join(format!("tasq-store-test-{}", new_ulid()));
        let path = dir.join("tasq.db");
        let mut a = Store::open_db(path.clone(), "2026-10-03".into()).unwrap();
        a.import(
            todo::parse_file("(A) one\ntwo\n"),
            todo::parse_file("x 2026-10-01 2026-09-30 old\n"),
        )
        .unwrap();
        let mut b = Store::open_db(path.clone(), "2026-10-03".into()).unwrap();
        assert_eq!(raws(b.tasks()), ["(A) one", "two"]);
        assert_eq!(b.archive().len(), 1);
        b.add_finalized("three");
        // `a` sees b's write and refuses to act on stale state.
        assert_eq!(a.reconcile(), crate::core::Reconcile::Reloaded);
        assert_eq!(a.tasks().len(), 3);
        assert_eq!(a.reconcile(), crate::core::Reconcile::Unchanged);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn store_keeps_renames_and_deletes_spaces() {
        let mut s = Store::in_memory_db("2026-10-03");
        s.add_finalized("study +Uni/Exams");
        s.add_finalized("pay tuition +Uni");
        let paths =
            |s: &Store| -> Vec<String> { s.space_tree().into_iter().map(|r| r.path).collect() };
        assert_eq!(paths(&s), ["Uni", "Uni/Exams"]);

        // Renaming a space moves its sub-spaces and their tasks.
        assert!(matches!(
            s.rename_project("Uni", "School"),
            crate::core::RenameOutcome::Done { renamed: 2 }
        ));
        assert_eq!(
            raws(s.tasks()),
            [
                "2026-10-03 study +School/Exams",
                "2026-10-03 pay tuition +School"
            ]
        );
        assert_eq!(paths(&s), ["School", "School/Exams"]);

        // Its last task gone, Exams stays, empty, until deleted.
        s.delete(0);
        assert_eq!(paths(&s), ["School", "School/Exams"]);
        assert_eq!(s.space_tree()[1].count, 0);
        assert!(matches!(
            s.delete_space("School"),
            crate::core::DeleteSpaceOutcome::InUse(1)
        ));
        assert!(matches!(
            s.delete_space("School/Exams"),
            crate::core::DeleteSpaceOutcome::Deleted
        ));
        assert_eq!(paths(&s), ["School"]);

        // An empty space can be renamed too.
        s.delete(0);
        assert!(matches!(
            s.rename_project("School", "Uni"),
            crate::core::RenameOutcome::Done { renamed: 0 }
        ));
        assert_eq!(paths(&s), ["Uni"]);
    }

    #[test]
    fn a_space_keeps_its_colour_and_its_sub_spaces_inherit_it() {
        use crate::core::spaces::SpaceColor;
        let mut s = Store::in_memory_db("2026-10-03");
        s.add_finalized("study +Uni/Exams");
        s.set_space_color("Uni", Some(SpaceColor::Slot(5)));
        assert_eq!(s.space_color("Uni"), SpaceColor::Slot(5));
        assert_eq!(s.space_color("Uni/Exams"), SpaceColor::Slot(5));
        s.set_space_color("Uni/Exams", Some(SpaceColor::Rgb(1, 2, 3)));
        assert_eq!(s.space_color("Uni/Exams"), SpaceColor::Rgb(1, 2, 3));
        // Renaming keeps the choice; resetting goes back to automatic.
        s.rename_project("Uni", "School");
        assert_eq!(s.space_color("School"), SpaceColor::Slot(5));
        s.set_space_color("School", None);
        assert_ne!(
            s.known_spaces()
                .iter()
                .find(|k| k.path == "School")
                .and_then(|k| k.color.clone()),
            Some("5".to_string())
        );
    }

    #[test]
    fn undoing_a_rename_takes_the_space_name_back() {
        let mut s = Store::in_memory_db("2026-10-03");
        s.add_finalized("study +Uni/Exams");
        s.set_space_hidden("Uni", true);
        s.rename_project("Uni", "School");
        s.undo();
        let paths: Vec<(String, bool)> = s
            .known_spaces()
            .iter()
            .map(|k| (k.path.clone(), k.hidden))
            .collect();
        assert_eq!(
            paths,
            [("Uni".to_string(), true), ("Uni/Exams".to_string(), false)]
        );
        assert!(s.tasks()[0].raw.ends_with("+Uni/Exams"));
    }
}
