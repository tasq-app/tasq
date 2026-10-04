//! The Trash screen: what you deleted in the last thirty days, newest
//! first; `r` puts a task back, `D` forgets it, `E` empties the trash.

use super::App;
use super::types::{Mode, Scope};
use crate::core::TrashItem;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrashScreen {
    pub cursor: usize,
    /// `E` was pressed once: the next `E` empties the trash.
    pub confirm_empty: bool,
}

impl App {
    pub fn open_trash(&mut self) {
        self.home = false;
        self.notes_screen = None;
        self.calendar = None;
        self.inspector_focus = false;
        self.mode = Mode::Normal;
        self.trash_screen = Some(TrashScreen::default());
    }

    pub fn close_trash(&mut self) {
        self.trash_screen = None;
        self.set_scope(Scope::Today);
    }

    /// Forget what was deleted more than thirty days ago (at startup).
    pub fn purge_old_trash(&mut self) {
        self.store.trash_purge_old();
    }

    pub fn trash_items(&self) -> Vec<TrashItem> {
        self.store.trash()
    }

    fn trash_current(&self) -> Option<TrashItem> {
        let cursor = self.trash_screen.as_ref()?.cursor;
        self.trash_items().into_iter().nth(cursor)
    }

    pub fn trash_move(&mut self, forward: bool) {
        let n = self.trash_items().len();
        if let Some(s) = self.trash_screen.as_mut() {
            s.cursor = if forward {
                (s.cursor + 1).min(n.saturating_sub(1))
            } else {
                s.cursor.saturating_sub(1)
            };
            s.confirm_empty = false;
        }
    }

    fn trash_clamp(&mut self) {
        let n = self.trash_items().len();
        if let Some(s) = self.trash_screen.as_mut() {
            s.cursor = s.cursor.min(n.saturating_sub(1));
        }
    }

    /// `r`: put the selected task back in the list.
    pub fn trash_restore_current(&mut self) {
        let Some(item) = self.trash_current() else {
            return;
        };
        match self.store.trash_restore(&item) {
            Ok(abs) => {
                let title = self.task_title(abs);
                self.toast(super::ToastKind::Done, "Restored", title, "restored");
                self.recompute_visible();
                self.trash_clamp();
            }
            Err(e) => self.flash(format!("restore failed: {e}")),
        }
    }

    /// `D`: forget the selected task for good.
    pub fn trash_forget_current(&mut self) {
        let Some(item) = self.trash_current() else {
            return;
        };
        match self.store.trash_forget(&item) {
            Ok(()) => {
                self.flash("deleted for good");
                self.trash_clamp();
            }
            Err(e) => self.flash(format!("delete failed: {e}")),
        }
    }

    /// `E` twice: empty the trash.
    pub fn trash_empty_confirmed(&mut self) {
        let Some(s) = self.trash_screen.as_mut() else {
            return;
        };
        if !s.confirm_empty {
            s.confirm_empty = true;
            self.flash("press E again to empty the trash");
            return;
        }
        s.confirm_empty = false;
        s.cursor = 0;
        match self.store.trash_empty() {
            Ok(()) => self.flash("trash emptied"),
            Err(e) => self.flash(format!("empty failed: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::app::test_support::build_app;

    #[test]
    fn a_deleted_task_comes_back_from_the_trash() {
        let mut app = build_app("keep\nlose me\n");
        app.delete(1);
        assert_eq!(app.tasks().len(), 1);
        app.open_trash();
        assert_eq!(app.trash_items()[0].raw, "lose me");
        app.trash_restore_current();
        assert_eq!(app.tasks().len(), 2);
        assert!(app.trash_items().iter().all(|i| i.raw != "lose me"));
    }
}
