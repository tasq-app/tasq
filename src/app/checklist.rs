//! A task's checklist and the notes linked to it, as the inspector shows
//! them.
//!
//! The checklist is not stored apart: it is the `- [ ]` / `- [x]` lines in
//! the task's notes, so a list you write in a note *is* the task's
//! checklist, and one you build in the inspector is a note you can open.
//! Items added from the inspector go to `checklist.md` in the task's notes
//! folder, which is created (and linked) the first time.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use super::App;
use super::types::Mode;
use crate::core::EditOutcome;
use crate::note;
use crate::todo::Task;

/// Where the inspector puts items you add to a task without notes.
pub const CHECKLIST_FILE: &str = "checklist.md";

/// One checkbox line in one of the task's notes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckItem {
    pub file: PathBuf,
    /// Line number in `file`, from 0.
    pub line: usize,
    pub done: bool,
    pub text: String,
}

/// A note linked to the task, as a card: its title and a first line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteCard {
    pub path: PathBuf,
    pub title: String,
    pub preview: String,
}

/// Everything the inspector shows from a task's notes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskNotes {
    pub items: Vec<CheckItem>,
    pub notes: Vec<NoteCard>,
}

impl TaskNotes {
    /// `(done, total)` of the checklist, if there is one.
    pub fn progress(&self) -> Option<(usize, usize)> {
        (!self.items.is_empty()).then(|| {
            (
                self.items.iter().filter(|i| i.done).count(),
                self.items.len(),
            )
        })
    }
}

/// Read once per frame at most: notes come from disk or the database, and
/// the list asks for every visible row. Cleared after every key and every
/// change from outside.
#[derive(Debug, Default)]
pub struct NotesCache(RefCell<HashMap<String, Rc<TaskNotes>>>);

impl NotesCache {
    pub fn clear(&self) {
        self.0.borrow_mut().clear();
    }
}

/// A checkbox line: `- [ ] text`, `* [x] text`, indented or not.
pub fn parse_item(line: &str) -> Option<(bool, &str)> {
    let rest = line.trim_start();
    let rest = rest
        .strip_prefix("- ")
        .or_else(|| rest.strip_prefix("* "))?;
    let (done, text) = if let Some(t) = rest.strip_prefix("[ ]") {
        (false, t)
    } else if let Some(t) = rest
        .strip_prefix("[x]")
        .or_else(|| rest.strip_prefix("[X]"))
    {
        (true, t)
    } else {
        return None;
    };
    Some((done, text.trim()))
}

/// `body` with the box on line `line` ticked or unticked.
pub fn toggle_line(body: &str, line: usize) -> Option<String> {
    let mut lines: Vec<String> = body.lines().map(str::to_string).collect();
    let l = lines.get_mut(line)?;
    let (done, _) = parse_item(l)?;
    let (from, to) = if done {
        (["[x]", "[X]"], "[ ]")
    } else {
        (["[ ]", "[ ]"], "[x]")
    };
    let at = from.iter().find_map(|f| l.find(f))?;
    l.replace_range(at..at + 3, to);
    Some(rejoin(lines, body))
}

/// `body` without line `line`.
pub fn remove_line(body: &str, line: usize) -> Option<String> {
    let mut lines: Vec<String> = body.lines().map(str::to_string).collect();
    if line >= lines.len() {
        return None;
    }
    lines.remove(line);
    Some(rejoin(lines, body))
}

/// `body` with a new unticked item after its last one (or at the end).
pub fn add_item(body: &str, text: &str) -> String {
    let mut lines: Vec<String> = body.lines().map(str::to_string).collect();
    let item = format!("- [ ] {text}");
    match lines.iter().rposition(|l| parse_item(l).is_some()) {
        Some(last) => lines.insert(last + 1, item),
        None => {
            if lines.last().is_some_and(|l| !l.trim().is_empty()) {
                lines.push(String::new());
            }
            lines.push(item);
        }
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

fn rejoin(lines: Vec<String>, original: &str) -> String {
    let mut out = lines.join("\n");
    if original.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// A note's card: its `# ` heading (else its file name) and the first line
/// of prose.
fn card(path: &Path, body: &str) -> NoteCard {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("note")
        .replace(['-', '_'], " ");
    let title = body
        .lines()
        .find_map(|l| l.strip_prefix("# "))
        .map_or(stem, |t| t.trim().to_string());
    let preview = body
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#') && parse_item(l).is_none())
        .unwrap_or("")
        .to_string();
    NoteCard {
        path: path.to_path_buf(),
        title,
        preview,
    }
}

/// Read a task's notes folder.
pub fn read_task_notes(dir: &Path) -> TaskNotes {
    let mut out = TaskNotes::default();
    for path in note::list_notes(dir) {
        let Ok(body) = crate::note_store::read(&path) else {
            continue;
        };
        for (i, l) in body.lines().enumerate() {
            if let Some((done, text)) = parse_item(l) {
                out.items.push(CheckItem {
                    file: path.clone(),
                    line: i,
                    done,
                    text: text.to_string(),
                });
            }
        }
        // A file that is only a checklist isn't shown again as a note.
        let only_list = body
            .lines()
            .map(str::trim)
            .all(|l| l.is_empty() || l.starts_with('#') || parse_item(l).is_some());
        if !only_list {
            out.notes.push(card(&path, &body));
        }
    }
    out
}

/// What the inspector's cursor is on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InspectorRow {
    Item(usize),
    AddItem,
    Note(usize),
}

impl App {
    /// The checklist and notes of `task` (cached until the next key).
    pub fn task_notes(&self, task: &Task) -> Rc<TaskNotes> {
        let Some(id) = note::notes_id_from_raw(&task.raw) else {
            return Rc::default();
        };
        if let Some(hit) = self.notes_cache.0.borrow().get(&id) {
            return Rc::clone(hit);
        }
        let folder = note::folder_for_task(task, self.notes_dir());
        let notes = Rc::new(read_task_notes(&folder.dir));
        self.notes_cache
            .0
            .borrow_mut()
            .insert(id, Rc::clone(&notes));
        notes
    }

    /// The rows the inspector's cursor walks: items, "add item", notes.
    pub fn inspector_rows(&self) -> Vec<InspectorRow> {
        let Some(t) = self.cur_task() else {
            return Vec::new();
        };
        let n = self.task_notes(t);
        let mut rows: Vec<InspectorRow> = (0..n.items.len()).map(InspectorRow::Item).collect();
        rows.push(InspectorRow::AddItem);
        rows.extend((0..n.notes.len()).map(InspectorRow::Note));
        rows
    }

    pub fn inspector_current(&self) -> Option<InspectorRow> {
        self.inspector_rows().into_iter().nth(self.inspector_cursor)
    }

    /// Give the keyboard to the inspector, opening it if it's closed.
    pub fn inspector_focus_on(&mut self) {
        if self.cur_task().is_none() {
            return;
        }
        if !self.prefs.layout.right {
            self.prefs.layout.right = true;
        }
        self.sidebar_focus = false;
        self.inspector_focus = true;
        self.inspector_cursor = 0;
    }

    pub fn inspector_move(&mut self, forward: bool) {
        let n = self.inspector_rows().len();
        self.inspector_cursor = if forward {
            (self.inspector_cursor + 1).min(n.saturating_sub(1))
        } else {
            self.inspector_cursor.saturating_sub(1)
        };
    }

    /// Tick or untick the item under the cursor.
    pub fn inspector_toggle(&mut self) {
        let Some(InspectorRow::Item(i)) = self.inspector_current() else {
            return;
        };
        let Some(item) = self.current_item(i) else {
            return;
        };
        let done = !item.done;
        if self.rewrite_note(&item.file, |b| toggle_line(b, item.line)) {
            let title = if done { "Checked" } else { "Unchecked" };
            self.toast(
                super::ToastKind::Done,
                title,
                Some(item.text),
                title.to_lowercase(),
            );
            // The last box ticked: the task may be done too (a setting:
            // ask, always, never).
            if done
                && self.prefs.checklist_done != super::AutoDone::Never
                && let Some(t) = self.cur_task().cloned()
                && !t.done
                && self.task_notes(&t).progress().is_some_and(|(d, n)| d == n)
                && let Some(abs) = self.cur_abs()
            {
                if self.prefs.checklist_done == super::AutoDone::Always {
                    self.toggle_complete(abs);
                } else {
                    self.confirm_done = Some(abs);
                }
            }
        }
    }

    /// Remove the item under the cursor from its note.
    pub fn inspector_remove(&mut self) {
        let Some(InspectorRow::Item(i)) = self.inspector_current() else {
            return;
        };
        let Some(item) = self.current_item(i) else {
            return;
        };
        if self.rewrite_note(&item.file, |b| remove_line(b, item.line)) {
            self.inspector_move(false);
            self.inspector_cursor = self.inspector_cursor.min(i);
        }
    }

    fn current_item(&self, i: usize) -> Option<CheckItem> {
        let t = self.cur_task()?;
        self.task_notes(t).items.get(i).cloned()
    }

    /// Open the note under the cursor in the notes window.
    pub fn inspector_open_note(&mut self) {
        let Some(InspectorRow::Note(i)) = self.inspector_current() else {
            return;
        };
        let Some(path) = self
            .cur_task()
            .and_then(|t| self.task_notes(t).notes.get(i).map(|n| n.path.clone()))
        else {
            return;
        };
        self.inspector_focus = false;
        self.open_notes_for_current();
        if let Some(pos) = self.notes_popup.files.iter().position(|p| *p == path) {
            self.notes_popup.cursor = pos;
            self.open_note_editor_preview();
        }
    }

    /// Ask for a new checklist item.
    pub fn begin_add_check_item(&mut self) {
        if self.cur_task().is_none() {
            return;
        }
        self.draft_clear();
        self.mode = Mode::PromptChecklist;
    }

    /// Add `text` to the current task's checklist: after the last item in
    /// whichever note has one, else to `checklist.md`, creating the task's
    /// notes folder (and linking it) if it has none yet.
    pub fn add_check_item(&mut self, text: &str) {
        // A pasted list keeps its bullets out: "- [ ] a", "* b", "• c", "1. d".
        let mut text = text.trim();
        for prefix in ["- [ ] ", "- [x] ", "- [X] ", "- ", "* ", "• "] {
            if let Some(rest) = text.strip_prefix(prefix) {
                text = rest.trim();
                break;
            }
        }
        if let Some((n, rest)) = text.split_once(". ")
            && !n.is_empty()
            && n.chars().all(|c| c.is_ascii_digit())
        {
            text = rest.trim();
        }
        if text.is_empty() {
            return;
        }
        let Some(task) = self.cur_task().cloned() else {
            return;
        };
        let existing = self.task_notes(&task).items.last().map(|i| i.file.clone());
        let folder = note::folder_for_task(&task, self.notes_dir());
        let path = existing.unwrap_or_else(|| folder.dir.join(CHECKLIST_FILE));
        if !crate::note_store::exists(&path) {
            if let Err(e) = crate::note_store::create_dir_all(&folder.dir) {
                self.flash(format!("couldn't add the item: {e}"));
                return;
            }
            if let Err(e) = crate::note_store::write(&path, &note::note_template(&task)) {
                self.flash(format!("couldn't add the item: {e}"));
                return;
            }
        }
        if !folder.existed_in_task
            && let Some(abs) = self.cur_abs()
        {
            match self.store.append_at(abs, &format!("notes:{}/", folder.id)) {
                EditOutcome::Saved { abs } => self.after_mutation(abs),
                EditOutcome::Aborted(r) => {
                    self.handle_reconcile_abort(r);
                    return;
                }
                EditOutcome::Error(e) => {
                    self.flash(format!("couldn't link the checklist: {e}"));
                    return;
                }
                EditOutcome::Empty | EditOutcome::OutOfRange | EditOutcome::TermNotFound => {
                    return;
                }
            }
        }
        if self.rewrite_note(&path, |b| Some(add_item(b, text))) {
            // Keep the cursor on "add item" so the next one is a key away.
            if let Some(pos) = self
                .inspector_rows()
                .iter()
                .position(|r| *r == InspectorRow::AddItem)
            {
                self.inspector_cursor = pos;
            }
        }
    }

    /// Read `path`, change it, write it back, refresh every view of it.
    fn rewrite_note(&mut self, path: &Path, edit: impl FnOnce(&str) -> Option<String>) -> bool {
        let body = match crate::note_store::read(path) {
            Ok(b) => b,
            Err(e) => {
                self.flash(format!("couldn't read the note: {e}"));
                return false;
            }
        };
        let Some(new) = edit(&body) else {
            return false;
        };
        if let Err(e) = crate::note_store::write(path, &new) {
            self.flash(format!("couldn't save the note: {e}"));
            return false;
        }
        self.notes_cache.clear();
        self.reload_note_editors(path);
        true
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn checkbox_lines_parse_tick_and_grow() {
        assert_eq!(parse_item("- [ ] Tema 3"), Some((false, "Tema 3")));
        assert_eq!(parse_item("  * [x] done"), Some((true, "done")));
        assert_eq!(parse_item("- plain"), None);
        let body = "# AII\n\n- [ ] a\n- [x] b\n\nprose\n";
        assert_eq!(
            toggle_line(body, 2).unwrap(),
            "# AII\n\n- [x] a\n- [x] b\n\nprose\n"
        );
        assert_eq!(
            toggle_line(body, 3).unwrap(),
            "# AII\n\n- [ ] a\n- [ ] b\n\nprose\n"
        );
        assert_eq!(toggle_line(body, 5), None);
        assert_eq!(
            add_item(body, "c"),
            "# AII\n\n- [ ] a\n- [x] b\n- [ ] c\n\nprose\n"
        );
        assert_eq!(add_item("# T\n", "a"), "# T\n\n- [ ] a\n");
        assert_eq!(remove_line(body, 2).unwrap(), "# AII\n\n- [x] b\n\nprose\n");
    }

    #[test]
    fn the_inspector_builds_a_checklist_in_the_tasks_notes() {
        use crate::app::test_support::build_app_with_config;
        let dir = std::env::temp_dir().join(format!(
            "tasq-checklist-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let cfg = crate::config::Config {
            notes_dir: Some(dir.to_string_lossy().into_owned()),
            ..Default::default()
        };
        let mut app = build_app_with_config("Teoría AII +Uni\n", cfg);
        app.inspector_focus_on();
        assert_eq!(app.inspector_rows(), vec![InspectorRow::AddItem]);

        app.add_check_item("Tema 3");
        app.add_check_item("Ejercicio 5");
        let t = app.cur_task().unwrap().clone();
        assert!(t.raw.contains("notes:"), "{}", t.raw);
        let n = app.task_notes(&t);
        assert_eq!(n.progress(), Some((0, 2)));
        assert!(n.notes.is_empty(), "a bare checklist isn't a note card");

        app.inspector_cursor = 0;
        app.inspector_toggle();
        assert_eq!(app.task_notes(&t).progress(), Some((1, 2)));
        app.inspector_cursor = 1;
        app.inspector_remove();
        let n = app.task_notes(&t);
        assert_eq!(n.progress(), Some((1, 1)));
        assert_eq!(n.items[0].text, "Tema 3");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ticking_the_last_box_finishes_the_task() {
        use crate::app::test_support::build_app_with_config;
        let dir = std::env::temp_dir().join(format!(
            "tasq-checklist-done-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let cfg = crate::config::Config {
            notes_dir: Some(dir.to_string_lossy().into_owned()),
            ..Default::default()
        };
        let mut app = build_app_with_config("Repasar\n", cfg);
        app.inspector_focus_on();
        app.add_check_item("uno");
        app.add_check_item("dos");
        app.inspector_cursor = 0;
        app.inspector_toggle();
        assert!(!app.tasks()[0].done, "one box left");
        app.inspector_cursor = 1;
        app.inspector_toggle();
        // By default it asks first.
        assert!(!app.tasks()[0].done);
        assert_eq!(app.confirm_done, Some(0));
        // Set to always, it's done straight away.
        app.confirm_done = None;
        app.inspector_toggle(); // untick
        app.prefs.checklist_done = crate::app::AutoDone::Always;
        app.inspector_toggle();
        assert!(app.tasks()[0].done, "all ticked: done");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_note_reads_as_a_card() {
        let c = card(
            Path::new("/n/apuntes-aii.md"),
            "# Apuntes AII\n\nÁrboles AVL: rotaciones\n",
        );
        assert_eq!(c.title, "Apuntes AII");
        assert_eq!(c.preview, "Árboles AVL: rotaciones");
        let c = card(Path::new("/n/apuntes-aii.md"), "- [ ] a\nmore\n");
        assert_eq!(c.title, "apuntes aii");
        assert_eq!(c.preview, "more");
    }
}
