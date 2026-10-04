//! Search (`Ctrl-K`, or Search in the sidebar): one box over everything —
//! your tasks (done ones too), your notes, word for word, and your spaces —
//! that narrows as you type; `Enter` goes there.

use std::path::PathBuf;

use super::App;
use super::home::note_title;
use super::types::{Mode, Scope};
use crate::core::spaces;
use crate::todo;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchAll {
    pub query: String,
    pub cursor: usize,
}

/// One result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Found {
    /// An open (or done, not yet archived) task, by its index.
    Task(usize),
    Note(PathBuf),
    Space(String),
}

/// A result as the window lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundRow {
    pub section: &'static str,
    pub label: String,
    /// The line of a note that matched, or a task's space.
    pub detail: String,
    pub done: bool,
    pub found: Found,
}

const MAX_TASKS: usize = 8;
const MAX_NOTES: usize = 6;
const MAX_SPACES: usize = 4;

impl App {
    pub fn open_search_all(&mut self) {
        self.search_all = SearchAll::default();
        self.mode = Mode::SearchAll;
    }

    pub fn close_search_all(&mut self) {
        self.mode = Mode::Normal;
    }

    /// What matches the query, tasks first, then notes, then spaces.
    pub fn search_all_rows(&self) -> Vec<FoundRow> {
        let q = self.search_all.query.trim().to_lowercase();
        if q.is_empty() {
            return Vec::new();
        }
        let mut rows = Vec::new();
        let mut tasks: Vec<(usize, &todo::Task)> = self
            .store
            .tasks()
            .iter()
            .enumerate()
            .filter(|(_, t)| todo::body_only(&t.raw).to_lowercase().contains(&q))
            .collect();
        // Open ones first.
        tasks.sort_by_key(|(_, t)| t.done);
        for (abs, t) in tasks.into_iter().take(MAX_TASKS) {
            rows.push(FoundRow {
                section: "TASKS",
                label: todo::body_only(&t.raw),
                detail: t
                    .projects
                    .first()
                    .map(|p| spaces::display(p))
                    .unwrap_or_default(),
                done: t.done,
                found: Found::Task(abs),
            });
        }
        let mut notes = 0;
        for (path, _) in crate::note_store::recent(self.notes_dir(), 1000) {
            if notes == MAX_NOTES {
                break;
            }
            let body = crate::note_store::read(&path).unwrap_or_default();
            let title = note_title(&path, &body);
            let line = body
                .lines()
                .map(str::trim)
                .find(|l| !l.starts_with("# ") && l.to_lowercase().contains(&q));
            if !title.to_lowercase().contains(&q) && line.is_none() {
                continue;
            }
            notes += 1;
            rows.push(FoundRow {
                section: "NOTES",
                label: title,
                detail: line.unwrap_or("").to_string(),
                done: false,
                found: Found::Note(path),
            });
        }
        for s in self
            .store
            .space_tree()
            .into_iter()
            .filter(|s| spaces::display(&s.path).to_lowercase().contains(&q))
            .take(MAX_SPACES)
        {
            rows.push(FoundRow {
                section: "SPACES",
                label: spaces::display(&s.path),
                detail: format!("{} open", s.count),
                done: false,
                found: Found::Space(s.path),
            });
        }
        rows
    }

    pub fn search_all_type(&mut self, c: char) {
        self.search_all.query.push(c);
        self.search_all.cursor = 0;
    }

    pub fn search_all_backspace(&mut self) {
        self.search_all.query.pop();
        self.search_all.cursor = 0;
    }

    pub fn search_all_move(&mut self, forward: bool) {
        let n = self.search_all_rows().len();
        let c = &mut self.search_all.cursor;
        *c = if forward {
            (*c + 1).min(n.saturating_sub(1))
        } else {
            c.saturating_sub(1)
        };
    }

    /// `Enter`: go to what's picked.
    pub fn search_all_go(&mut self) {
        let Some(row) = self
            .search_all_rows()
            .into_iter()
            .nth(self.search_all.cursor)
        else {
            return;
        };
        let query = self.search_all.query.clone();
        self.close_search_all();
        self.home = false;
        match row.found {
            Found::Task(abs) => {
                let raw = self.store.tasks().get(abs).map(|t| t.raw.clone());
                self.filter.clear();
                if row.done {
                    self.prefs.show_done = true;
                }
                self.set_scope(Scope::All);
                if let Some(abs) =
                    raw.and_then(|r| self.store.tasks().iter().position(|t| t.raw == r))
                {
                    self.follow_cursor(abs);
                }
            }
            Found::Note(path) => {
                self.open_notes_screen();
                // The note opens with what you searched for lit up.
                if let Some(s) = self.notes_screen.as_mut() {
                    s.query = query;
                    s.hit = Some(0);
                }
                let pos = self.note_entries().iter().position(|e| e.path == path);
                if let (Some(pos), Some(s)) = (pos, self.notes_screen.as_mut()) {
                    s.cursor = pos;
                }
            }
            Found::Space(p) => self.sidebar_open(&super::NavItem::Space(p)),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::app::test_support::build_app;

    #[test]
    fn search_finds_tasks_and_spaces_and_goes_there() {
        let mut app = build_app("Teoría AII +Uni/AII\nx 2026-05-01 Teoría vieja\nComprar pan\n");
        app.open_search_all();
        for c in "teor".chars() {
            app.search_all_type(c);
        }
        let rows = app.search_all_rows();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].label, "Teoría AII");
        assert!(rows[1].done, "done ones come after");
        app.search_all.query = "ai".into();
        assert!(
            app.search_all_rows()
                .iter()
                .any(|r| r.found == Found::Space("Uni/AII".into()))
        );
        app.search_all.query = "pan".into();
        app.search_all.cursor = 0;
        app.search_all_go();
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.cur_task().unwrap().raw, "Comprar pan");
    }
}
