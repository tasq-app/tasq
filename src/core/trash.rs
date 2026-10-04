//! The trash: a deleted task stays here for [`KEEP_DAYS`] days, and can be
//! put back. With the database it's a table; with a todo.txt, a sibling
//! `trash.txt` of `date<TAB>line` rows.

use std::path::PathBuf;

use chrono::{Days, NaiveDate};

use super::{Store, StoreError};
use crate::todo;

/// How long a deleted task is kept.
pub const KEEP_DAYS: u64 = 30;

/// A deleted task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashItem {
    /// The database row id, or the file row itself.
    pub key: String,
    pub raw: String,
    /// `YYYY-MM-DD`.
    pub deleted_on: String,
}

impl Store {
    /// `trash.txt` beside `todo.txt`; beside any other file, `<name>.trash`.
    fn trash_file(&self) -> PathBuf {
        let name = self
            .file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("todo.txt");
        if name == "todo.txt" {
            self.file_path.with_file_name("trash.txt")
        } else {
            self.file_path.with_file_name(format!("{name}.trash"))
        }
    }

    fn read_trash_file(&self) -> Vec<TrashItem> {
        let body = std::fs::read_to_string(self.trash_file()).unwrap_or_default();
        let mut items: Vec<TrashItem> = body
            .lines()
            .filter_map(|l| {
                let (on, raw) = l.split_once('\t')?;
                Some(TrashItem {
                    key: l.to_string(),
                    raw: raw.to_string(),
                    deleted_on: on.to_string(),
                })
            })
            .collect();
        items.reverse();
        items
    }

    fn write_trash_file(&self, items: &[TrashItem]) -> std::io::Result<()> {
        let mut body = String::new();
        for i in items.iter().rev() {
            body.push_str(&i.deleted_on);
            body.push('\t');
            body.push_str(&i.raw);
            body.push('\n');
        }
        todo::write_atomic(&self.trash_file(), &body)
    }

    /// Keep `raws` in the trash. Best effort: a failure here never stops
    /// the delete itself.
    pub(crate) fn trash_put(&mut self, raws: &[String]) {
        let on = self.today.clone();
        if let Some(db) = self.db.as_mut() {
            let _ = db.trash_put(raws, &on);
            return;
        }
        let mut items = self.read_trash_file();
        for raw in raws {
            items.insert(
                0,
                TrashItem {
                    key: String::new(),
                    raw: raw.clone(),
                    deleted_on: on.clone(),
                },
            );
        }
        let _ = self.write_trash_file(&items);
    }

    /// What's in the trash, newest first. A task that's back in the list
    /// (deleted, then undone) isn't shown.
    pub fn trash(&self) -> Vec<TrashItem> {
        let items = match self.db.as_ref() {
            Some(db) => db
                .trash_list()
                .unwrap_or_default()
                .into_iter()
                .map(|(key, raw, deleted_on)| TrashItem {
                    key,
                    raw,
                    deleted_on,
                })
                .collect(),
            None => self.read_trash_file(),
        };
        let live: std::collections::HashSet<&str> =
            self.tasks.iter().map(|t| t.raw.as_str()).collect();
        items
            .into_iter()
            .filter(|i| !live.contains(i.raw.as_str()))
            .collect()
    }

    fn trash_drop(&mut self, key: Option<&str>, before: Option<&str>) -> Result<(), StoreError> {
        if let Some(db) = self.db.as_mut() {
            return db.trash_remove(key, before).map_err(StoreError::Write);
        }
        let items: Vec<TrashItem> = self
            .read_trash_file()
            .into_iter()
            .filter(|i| match (key, before) {
                (Some(k), _) => i.key != k,
                (None, Some(d)) => i.deleted_on.as_str() >= d,
                (None, None) => false,
            })
            .collect();
        self.write_trash_file(&items).map_err(StoreError::Write)
    }

    /// Put a deleted task back in the list.
    pub fn trash_restore(&mut self, item: &TrashItem) -> Result<usize, StoreError> {
        let task = todo::parse_line(&item.raw)
            .map_err(|e| StoreError::Write(std::io::Error::other(e.to_string())))?;
        self.push_history();
        self.tasks.push(task);
        let abs = self.tasks.len() - 1;
        self.persist()?;
        self.trash_drop(Some(&item.key), None)?;
        Ok(abs)
    }

    /// Delete one task for good.
    pub fn trash_forget(&mut self, item: &TrashItem) -> Result<(), StoreError> {
        self.trash_drop(Some(&item.key), None)
    }

    /// Empty the trash.
    pub fn trash_empty(&mut self) -> Result<(), StoreError> {
        self.trash_drop(None, None)
    }

    /// Forget what was deleted more than [`KEEP_DAYS`] days ago.
    pub fn trash_purge_old(&mut self) {
        let Some(cut) = NaiveDate::parse_from_str(&self.today, "%Y-%m-%d")
            .ok()
            .and_then(|d| d.checked_sub_days(Days::new(KEEP_DAYS)))
        else {
            return;
        };
        let cut = cut.format("%Y-%m-%d").to_string();
        if self.db.is_none() && !self.trash_file().exists() {
            return;
        }
        let _ = self.trash_drop(None, Some(&cut));
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::core::test_support::build_store;

    #[test]
    fn deleted_tasks_wait_in_the_trash_and_come_back() {
        let mut store = build_store("a\nb\nc\n");
        let _ = std::fs::remove_file(store.trash_file());
        store.delete(1);
        store.delete_many(&[0]);
        let trash = store.trash();
        assert_eq!(
            trash.iter().map(|i| i.raw.as_str()).collect::<Vec<_>>(),
            vec!["a", "b"]
        );
        let abs = store.trash_restore(&trash[1]).unwrap();
        assert_eq!(store.tasks()[abs].raw, "b");
        assert_eq!(store.trash().len(), 1);

        // Undoing a delete takes it out of the trash's view.
        store.undo();
        store.undo();
        assert!(store.trash().iter().all(|i| i.raw != "a"));

        store.trash_empty().unwrap();
        assert!(store.trash().is_empty());
        let _ = std::fs::remove_file(store.trash_file());
    }
}
