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
    /// Which search hit in the note `n` / `N` are on.
    pub hit: Option<usize>,
    /// `d` was pressed: `y` deletes the note.
    pub confirm_delete: bool,
    /// The note open in the built-in editor (vim keys, `:w` `:q` `:wq`).
    pub editor: Option<crate::app::NoteEditorState>,
    /// Showing only one task's notes (`o` on a task).
    pub task: Option<TaskNotes>,
    /// Reading the selected note across the whole screen (`Enter`).
    pub reading: bool,
    /// Naming a new note for the task (`a`): what's typed so far.
    pub naming: Option<String>,
    /// How far the note can scroll, measured when it was drawn.
    pub max_scroll: std::cell::Cell<u16>,
}

/// The task whose notes the screen is showing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskNotes {
    pub folder: note::NotesFolder,
    pub title: String,
}

/// The tag marking a task's main note, shown first: `main:apuntes.md`.
pub const MAIN_NOTE_KEY: &str = "main";

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
        self.trash_screen = None;
        self.calendar = None;
        self.inspector_focus = false;
        self.mode = Mode::Normal;
        self.notes_screen = Some(NotesScreen::default());
    }

    /// `o` on a task: the Notes screen showing only that task's notes, its
    /// main note first, where `a` adds another.
    pub fn open_notes_for_task(&mut self, abs: usize) {
        let Some(task) = self.store.tasks().get(abs).cloned() else {
            return;
        };
        let folder = note::folder_for_task(&task, self.notes_dir());
        self.open_notes_screen();
        if let Some(s) = self.notes_screen.as_mut() {
            s.task = Some(TaskNotes {
                folder,
                title: crate::todo::body_only(&task.raw),
            });
        }
    }

    /// The task the screen is filtered to, as it is now (its index moves
    /// as the list changes).
    fn notes_task_abs(&self) -> Option<usize> {
        let id = &self.notes_screen.as_ref()?.task.as_ref()?.folder.id;
        self.store
            .tasks()
            .iter()
            .position(|t| note::notes_id_from_raw(&t.raw).as_deref() == Some(id.as_str()))
    }

    /// The main note of the task whose notes folder holds `path`.
    fn main_note_of(&self, path: &std::path::Path) -> Option<String> {
        self.note_tasks(path)
            .into_iter()
            .find_map(|t| crate::todo::find_kv(&t.raw, MAIN_NOTE_KEY))
    }

    /// `a` on a task's notes: start naming a new one.
    pub fn notes_screen_begin_new(&mut self) {
        let Some(s) = self.notes_screen.as_mut() else {
            return;
        };
        if s.task.is_none() {
            self.flash("o on a task opens its notes, where a adds one");
            return;
        }
        s.naming = Some(String::new());
    }

    /// Enter on the name: the note is made in the task's folder (linking
    /// the folder to the task the first time) and opens in the editor.
    pub fn notes_screen_create(&mut self) {
        let Some(s) = self.notes_screen.as_mut() else {
            return;
        };
        let name = s.naming.take().unwrap_or_default();
        let name = name.trim();
        let Some(tn) = s.task.clone() else {
            return;
        };
        if name.is_empty() {
            return;
        }
        let Some(abs) = self.notes_task_abs().or_else(|| {
            // Not linked yet: the task is found by its title.
            self.store
                .tasks()
                .iter()
                .position(|t| crate::todo::body_only(&t.raw) == tn.title)
        }) else {
            self.flash("its task is gone");
            return;
        };
        let Some(task) = self.store.tasks().get(abs).cloned() else {
            return;
        };
        let file = if name.to_ascii_lowercase().ends_with(".md") {
            name.to_string()
        } else {
            format!("{name}.md")
        };
        let path = tn.folder.dir.join(&file);
        if crate::note_store::read(&path).is_ok() {
            self.flash("there's already a note with that name");
            return;
        }
        if let Err(e) = crate::note_store::create_dir_all(&tn.folder.dir) {
            self.flash(format!("couldn't make the notes folder: {e}"));
            return;
        }
        let title = name.trim_end_matches(".md");
        if let Err(e) = crate::note_store::write(&path, &format!("# {title}\n\n")) {
            self.flash(format!("couldn't write the note: {e}"));
            return;
        }
        if note::notes_id_from_raw(&task.raw).is_none() {
            match self
                .store
                .append_at(abs, &format!("notes:{}/", tn.folder.id))
            {
                crate::core::EditOutcome::Saved { abs } => self.after_mutation(abs),
                crate::core::EditOutcome::Aborted(r) => self.handle_reconcile_abort(r),
                crate::core::EditOutcome::Error(e) => self.flash(format!("note link failed: {e}")),
                _ => {}
            }
        }
        self.notes_cache.clear();
        let i = self
            .note_entries()
            .iter()
            .position(|e| e.path == path)
            .unwrap_or(0);
        if let Some(s) = self.notes_screen.as_mut() {
            s.cursor = i;
        }
        self.notes_screen_open_editor();
        // Ready to write, under the title.
        if let Some(ed) = self.notes_screen.as_mut().and_then(|s| s.editor.as_mut()) {
            for _ in 0..ed.lines().len() {
                ed.move_down();
            }
            ed.enter_insert();
        }
    }

    /// `*`: make the selected note its task's main note, shown first (again
    /// to unset it).
    pub fn notes_screen_toggle_main(&mut self) {
        let Some(e) = self.current_note_entry() else {
            return;
        };
        let Some(file) = e
            .path
            .file_name()
            .and_then(|f| f.to_str())
            .map(str::to_string)
        else {
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
        let Some(abs) = self.store.tasks().iter().position(|t| t.raw == raw) else {
            return;
        };
        let was = crate::todo::find_kv(&raw, MAIN_NOTE_KEY).as_deref() == Some(file.as_str());
        let mut words: Vec<String> = raw
            .split_whitespace()
            .filter(|w| !w.starts_with(&format!("{MAIN_NOTE_KEY}:")))
            .map(str::to_string)
            .collect();
        if !was {
            words.push(format!("{MAIN_NOTE_KEY}:{file}"));
        }
        match self.store.edit_line(abs, &words.join(" ")) {
            crate::core::EditOutcome::Saved { abs } => {
                self.after_mutation(abs);
                self.flash(if was { "no main note" } else { "main note" });
            }
            crate::core::EditOutcome::Aborted(r) => self.handle_reconcile_abort(r),
            crate::core::EditOutcome::Error(e) => self.flash(format!("couldn't save: {e}")),
            _ => {}
        }
        // The cursor stays on the same note wherever it moved.
        let i = self.note_entries().iter().position(|n| n.path == e.path);
        if let (Some(s), Some(i)) = (self.notes_screen.as_mut(), i) {
            s.cursor = i;
        }
    }

    /// Whether `path` is its task's main note.
    pub fn is_main_note(&self, path: &std::path::Path) -> bool {
        let file = path.file_name().and_then(|f| f.to_str());
        file.is_some() && self.main_note_of(path).as_deref() == file
    }

    /// `Enter`: read the selected note across the whole screen.
    pub fn notes_screen_read(&mut self, on: bool) {
        if let Some(s) = self.notes_screen.as_mut() {
            s.reading = on;
            s.scroll = 0;
        }
    }

    /// Scroll the note by `rows` (negative: up), within what it has.
    pub fn notes_screen_scroll_by(&mut self, rows: i32) {
        if let Some(s) = self.notes_screen.as_mut() {
            let max = i32::from(s.max_scroll.get());
            s.scroll = (i32::from(s.scroll) + rows).clamp(0, max.max(0)) as u16;
            s.hit = None;
        }
    }

    /// `gg` / `G`: the top or the end of the note.
    pub fn notes_screen_scroll_edge(&mut self, end: bool) {
        if let Some(s) = self.notes_screen.as_mut() {
            s.scroll = if end { s.max_scroll.get() } else { 0 };
            s.hit = None;
        }
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
        let only = self
            .notes_screen
            .as_ref()
            .and_then(|s| s.task.as_ref())
            .map(|t| t.folder.dir.clone());
        let mut entries: Vec<NoteEntry> = crate::note_store::recent(self.notes_dir(), 1000)
            .into_iter()
            .filter(|(path, _)| {
                only.as_ref()
                    .is_none_or(|d| path.parent() == Some(d.as_path()))
            })
            .filter_map(|(path, at)| {
                // A note goes with its task: deleted, it's out of sight
                // (and back if the task comes back from the trash).
                if self.note_tasks(&path).is_empty() {
                    return None;
                }
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
            .collect();
        // Showing a task's notes: its main note first.
        if only.is_some()
            && let Some(i) = entries.iter().position(|e| self.is_main_note(&e.path))
        {
            let main = entries.remove(i);
            entries.insert(0, main);
        }
        entries
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
            s.hit = None;
        }
    }

    pub fn notes_screen_scroll(&mut self, down: bool) {
        self.notes_screen_scroll_by(if down { 3 } else { -3 });
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

    /// `Enter` / `e`: open the selected note in the built-in editor.
    pub fn notes_screen_open_editor(&mut self) {
        let Some(e) = self.current_note_entry() else {
            return;
        };
        let editor = crate::app::NoteEditorState::load(e.path, crate::app::NoteEditorMode::Normal);
        if let Some(s) = self.notes_screen.as_mut() {
            s.editor = Some(editor);
        }
    }

    /// `d` then `y`: delete the selected note for good.
    pub fn notes_screen_delete(&mut self) {
        if let Some(s) = self.notes_screen.as_mut() {
            s.confirm_delete = false;
        }
        let Some(e) = self.current_note_entry() else {
            return;
        };
        match crate::note_store::remove(&e.path) {
            Ok(()) => {
                self.toast(
                    super::ToastKind::Info,
                    "Note deleted",
                    Some(e.title),
                    "note deleted",
                );
                let n = self.note_entries().len();
                if let Some(s) = self.notes_screen.as_mut() {
                    s.cursor = s.cursor.min(n.saturating_sub(1));
                }
            }
            Err(err) => self.flash(format!("couldn't delete the note: {err}")),
        }
    }

    /// `n` / `N`: the next (or previous) search hit in the note.
    pub fn notes_screen_next_hit(&mut self, forward: bool) {
        if let Some(s) = self.notes_screen.as_mut()
            && !s.query.trim().is_empty()
        {
            s.hit = Some(match (s.hit, forward) {
                (None, _) => 0,
                (Some(h), true) => h + 1,
                (Some(h), false) => h.saturating_sub(1),
            });
        }
    }

    /// `E`: open the selected note in `$EDITOR`.
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
    fn dragging_over_a_note_selects_and_copies_only_its_text() {
        use ratatui::{Terminal, backend::TestBackend};
        let dir = std::env::temp_dir().join(format!(
            "tasq-notes-mouse-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let folder = dir.join("tasks").join("abc");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("a.md"), "# Title\n\nalpha beta gamma\n").unwrap();
        let cfg = crate::config::Config {
            notes_dir: Some(dir.to_string_lossy().into_owned()),
            ..Default::default()
        };
        let mut app = build_app_with_config("Teoría AII +Uni/AII notes:abc/\n", cfg);
        app.open_notes_screen();
        app.notes_screen_open_editor();
        let mut term = Terminal::new(TestBackend::new(120, 30)).unwrap();
        term.draw(|f| crate::ui::draw(f, &app)).unwrap();
        // Where "beta" was drawn.
        let buf = term.backend().buffer().clone();
        let (bx, by) = (0..buf.area.height)
            .find_map(|y| {
                let row: String = (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect();
                row.find("alpha beta")
                    .map(|i| (row[..i].chars().count() as u16 + 6, y))
            })
            .unwrap();
        // Outside the text: not the editor's.
        assert!(!app.editor_mouse_down(0, by));
        assert!(app.editor_mouse_down(bx, by));
        // Dragged way past the right edge: stops at the end of the text.
        assert!(app.editor_mouse_drag(119, by));
        assert!(app.editor_mouse_up());
        assert_eq!(app.take_note_clipboard().as_deref(), Some("beta gamma"));
        let _ = std::fs::remove_dir_all(&dir);
    }

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

    #[test]
    fn o_shows_only_the_tasks_notes_and_a_adds_one_that_can_be_the_main() {
        let dir = std::env::temp_dir().join(format!(
            "tasq-notes-task-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        for (id, file) in [("abc", "apuntes.md"), ("xyz", "other.md")] {
            let folder = dir.join("tasks").join(id);
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(folder.join(file), "# Note\n\nbody\n").unwrap();
        }
        let cfg = crate::config::Config {
            notes_dir: Some(dir.to_string_lossy().into_owned()),
            ..Default::default()
        };
        let mut app = build_app_with_config("Teoría AII notes:abc/\nOther notes:xyz/\n", cfg);
        app.open_notes_for_task(0);
        let only = app.note_entries();
        assert_eq!(only.len(), 1, "just the task's notes");
        assert!(only[0].path.ends_with("abc/apuntes.md"));

        // `a`, a name, Enter: a new note in the task's folder, in the editor.
        app.notes_screen_begin_new();
        app.notes_screen.as_mut().unwrap().naming = Some("dudas".into());
        app.notes_screen_create();
        let s = app.notes_screen.as_ref().unwrap();
        assert!(s.editor.is_some(), "opens to write in");
        assert!(dir.join("tasks/abc/dudas.md").exists());
        app.notes_screen.as_mut().unwrap().editor = None;
        assert_eq!(app.note_entries().len(), 2);

        // `*` on it: the main note, listed first; `*` again unsets it.
        app.notes_screen_toggle_main();
        assert!(app.store.tasks()[0].raw.contains("main:dudas.md"));
        let first = app.note_entries()[0].path.clone();
        assert!(first.ends_with("dudas.md"));
        assert!(app.is_main_note(&first));
        app.notes_screen_toggle_main();
        assert!(!app.store.tasks()[0].raw.contains("main:"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reading_scrolls_within_the_note_and_jumps_to_its_ends() {
        let mut app = crate::app::test_support::build_app("a\n");
        app.open_notes_screen();
        app.notes_screen_read(true);
        app.notes_screen.as_ref().unwrap().max_scroll.set(10);
        app.notes_screen_scroll_by(4);
        app.notes_screen_scroll_by(-10);
        assert_eq!(app.notes_screen.as_ref().unwrap().scroll, 0);
        app.notes_screen_scroll_by(50);
        assert_eq!(app.notes_screen.as_ref().unwrap().scroll, 10);
        app.notes_screen_scroll_edge(false);
        assert_eq!(app.notes_screen.as_ref().unwrap().scroll, 0);
        app.notes_screen_scroll_edge(true);
        assert_eq!(app.notes_screen.as_ref().unwrap().scroll, 10);
    }
}
