//! The Notes screen (`N`): every note, the one you touched last on top,
//! with a search box, and the selected one rendered beside the list — its
//! checkboxes, and the tasks that link to it underneath.

use std::path::PathBuf;

use super::App;
use super::home::{ago, note_title};
use super::types::{Mode, Scope};
use crate::note;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NotesScreen {
    pub query: String,
    /// The search box has the keyboard.
    pub searching: bool,
    pub cursor: usize,
    /// How far the preview is scrolled.
    pub scroll: u16,
}

/// A note in the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteEntry {
    pub path: PathBuf,
    pub title: String,
    pub when: String,
    pub body: String,
    /// The space of the task it belongs to, for the dot.
    pub space: Option<String>,
}

impl App {
    pub fn open_notes_screen(&mut self) {
        self.home = false;
        self.calendar = None;
        self.inspector_focus = false;
        self.mode = Mode::Normal;
        self.notes_screen = Some(NotesScreen::default());
    }

    /// Back to the list (Today).
    pub fn close_notes_screen(&mut self) {
        self.notes_screen = None;
        self.set_scope(Scope::Today);
    }

    /// Every task note matching the search, newest first.
    pub fn note_entries(&self) -> Vec<NoteEntry> {
        let today = self.today_naive();
        let query = self
            .notes_screen
            .as_ref()
            .map(|s| s.query.trim().to_lowercase())
            .unwrap_or_default();
        crate::note_store::recent(self.notes_dir(), 1000)
            .into_iter()
            .filter_map(|(path, at)| {
                let body = crate::note_store::read(&path).unwrap_or_default();
                let title = note_title(&path, &body);
                if !query.is_empty()
                    && !title.to_lowercase().contains(&query)
                    && !body.to_lowercase().contains(&query)
                {
                    return None;
                }
                let space = self
                    .note_tasks(&path)
                    .into_iter()
                    .find_map(|t| t.projects.first().cloned());
                Some(NoteEntry {
                    when: ago(at, today),
                    path,
                    title,
                    body,
                    space,
                })
            })
            .collect()
    }

    /// The tasks (open or archived) whose notes folder holds `path`.
    pub fn note_tasks(&self, path: &std::path::Path) -> Vec<&crate::todo::Task> {
        let Some(id) = path
            .parent()
            .and_then(|d| d.file_name())
            .and_then(|n| n.to_str())
        else {
            return Vec::new();
        };
        self.store
            .tasks()
            .iter()
            .chain(self.store.archive().tasks())
            .filter(|t| note::notes_id_from_raw(&t.raw).as_deref() == Some(id))
            .collect()
    }

    pub fn current_note_entry(&self) -> Option<NoteEntry> {
        let cursor = self.notes_screen.as_ref()?.cursor;
        self.note_entries().into_iter().nth(cursor)
    }

    pub fn notes_screen_move(&mut self, forward: bool) {
        let n = self.note_entries().len();
        if let Some(s) = self.notes_screen.as_mut() {
            s.cursor = if forward {
                (s.cursor + 1).min(n.saturating_sub(1))
            } else {
                s.cursor.saturating_sub(1)
            };
            s.scroll = 0;
        }
    }

    pub fn notes_screen_scroll(&mut self, down: bool) {
        if let Some(s) = self.notes_screen.as_mut() {
            s.scroll = if down {
                s.scroll.saturating_add(3)
            } else {
                s.scroll.saturating_sub(3)
            };
        }
    }

    pub fn notes_screen_type(&mut self, c: char) {
        if let Some(s) = self.notes_screen.as_mut() {
            s.query.push(c);
            s.cursor = 0;
            s.scroll = 0;
        }
    }

    pub fn notes_screen_backspace(&mut self) {
        if let Some(s) = self.notes_screen.as_mut() {
            s.query.pop();
            s.cursor = 0;
        }
    }

    /// `e`: open the selected note in `$EDITOR`.
    pub fn notes_screen_edit(&mut self) {
        if let Some(e) = self.current_note_entry() {
            self.queue_editor_path(e.path);
        }
    }

    /// `p`: pin the selected note beside the list.
    pub fn notes_screen_pin(&mut self) {
        let Some(e) = self.current_note_entry() else {
            return;
        };
        self.close_notes_screen();
        self.pin_note_path(e.path);
    }

    /// `Enter`: go to the task the note belongs to.
    pub fn notes_screen_open_task(&mut self) {
        let Some(e) = self.current_note_entry() else {
            return;
        };
        let raw = self
            .note_tasks(&e.path)
            .into_iter()
            .find(|t| !t.done)
            .map(|t| t.raw.clone());
        let Some(raw) = raw else {
            self.flash("its task is done or gone");
            return;
        };
        self.notes_screen = None;
        self.filter.clear();
        self.set_scope(Scope::All);
        if let Some(abs) = self.store.tasks().iter().position(|t| t.raw == raw) {
            self.follow_cursor(abs);
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::app::test_support::build_app_with_config;

    #[test]
    fn the_notes_screen_lists_searches_and_links_back() {
        let dir = std::env::temp_dir().join(format!(
            "tasq-notes-screen-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let folder = dir.join("tasks").join("abc");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("apuntes.md"), "# Apuntes AII\n\nÁrboles AVL\n").unwrap();
        std::fs::write(folder.join("dudas.md"), "# Dudas\n\nrotaciones\n").unwrap();
        let cfg = crate::config::Config {
            notes_dir: Some(dir.to_string_lossy().into_owned()),
            ..Default::default()
        };
        let mut app = build_app_with_config("Teoría AII +Uni/AII notes:abc/\nother\n", cfg);
        app.open_notes_screen();
        let all = app.note_entries();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].space.as_deref(), Some("Uni/AII"));

        for c in "avl".chars() {
            app.notes_screen_type(c);
        }
        let found = app.note_entries();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].title, "Apuntes AII");
        assert_eq!(app.note_tasks(&found[0].path).len(), 1);

        app.notes_screen_open_task();
        assert!(app.notes_screen.is_none());
        assert_eq!(app.cur_abs(), Some(0));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
