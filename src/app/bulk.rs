use super::App;
use super::types::Mode;
use crate::core::{BulkCompleteOutcome, BulkDeleteOutcome, BulkEventOutcome};

impl App {
    /// Bulk-complete every task in the selection that isn't already done.
    /// Clears the selection and exits Visual mode on success. Recurring tasks
    /// also spawn their next instance (handled by the store).
    pub fn complete_selected(&mut self) {
        if self.selection.is_empty() {
            return;
        }
        let indices: Vec<usize> = self.selection.iter().collect();
        match self.store.complete_many(&indices) {
            BulkCompleteOutcome::Done { completed, spawned } => {
                self.selection.clear();
                self.mode = Mode::Normal;
                self.flash(if spawned > 0 {
                    format!("completed {completed} (+{spawned} recurring)")
                } else {
                    format!("completed {completed}")
                });
                self.recompute_visible();
                self.clamp_cursor();
            }
            BulkCompleteOutcome::NothingToComplete => {
                self.flash("nothing to complete");
                self.selection.clear();
                self.mode = Mode::Normal;
            }
            BulkCompleteOutcome::Aborted(r) => self.handle_reconcile_abort(r),
            BulkCompleteOutcome::Error(e) => self.flash(format!("complete failed: {e}")),
        }
    }

    /// `E` on a selection: all of it events (classes, meetings: never
    /// overdue, they move on once over), or all tasks again when every one
    /// already is an event.
    pub fn toggle_event_selected(&mut self) {
        if self.selection.is_empty() {
            return;
        }
        let indices: Vec<usize> = self.selection.iter().collect();
        let on = !indices
            .iter()
            .all(|&i| self.store.tasks().get(i).is_some_and(|t| t.event));
        match self.store.set_event_many(&indices, on) {
            BulkEventOutcome::Done { changed } => {
                self.selection.clear();
                self.mode = Mode::Normal;
                self.flash(if on {
                    format!("{changed} now events · never overdue, they move on once over")
                } else {
                    format!("{changed} tasks again")
                });
                self.recompute_visible();
                self.clamp_cursor();
                self.gcal_touch(false);
                self.roll_past_events();
            }
            BulkEventOutcome::Nothing => {
                self.selection.clear();
                self.mode = Mode::Normal;
            }
            BulkEventOutcome::Aborted(r) => self.handle_reconcile_abort(r),
            BulkEventOutcome::Error(e) => self.flash(format!("write failed: {e}")),
        }
    }

    /// Bulk-delete every task in the selection.
    pub fn delete_selected(&mut self) {
        if self.selection.is_empty() {
            return;
        }
        let indices: Vec<usize> = self.selection.iter().collect();
        match self.store.delete_many(&indices) {
            BulkDeleteOutcome::Done { deleted } => {
                self.selection.clear();
                self.mode = Mode::Normal;
                self.flash(format!("deleted {deleted}"));
                self.recompute_visible();
                self.clamp_cursor();
            }
            BulkDeleteOutcome::Nothing => {
                self.selection.clear();
                self.mode = Mode::Normal;
            }
            BulkDeleteOutcome::Aborted(r) => self.handle_reconcile_abort(r),
            BulkDeleteOutcome::Error(e) => self.flash(format!("write failed: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_support::build_app;

    #[test]
    fn a_selection_becomes_events_and_back() {
        let mut app = build_app(
            "Algebra plan:2026-05-04 at:09:00 rec:1w\nPhysics plan:2026-05-05 at:11:00 rec:1w\nLaundry\n",
        );
        app.selection.toggle(0);
        app.selection.toggle(1);
        app.mode = Mode::Visual;
        app.toggle_event_selected();
        assert!(app.tasks()[0].event && app.tasks()[1].event);
        assert!(!app.tasks()[2].event);
        assert_eq!(app.mode, Mode::Normal);
        // Past ones move on to their next week at once (today is 2026-05-06).
        assert_eq!(app.tasks()[0].planned.as_deref(), Some("2026-05-11"));
        // Undo: first the move, then the marking.
        app.undo();
        app.undo();
        assert!(!app.tasks()[0].event && !app.tasks()[1].event);
        // All already events: the same key makes them tasks again.
        app.selection.toggle(0);
        app.selection.toggle(1);
        app.toggle_event_selected();
        app.selection.toggle(0);
        app.selection.toggle(1);
        app.toggle_event_selected();
        assert!(!app.tasks()[0].event && !app.tasks()[1].event);
    }

    #[test]
    fn complete_selected_clears_selection_and_flashes() {
        let mut app = build_app("a\nb\nc\n");
        app.selection.toggle(0);
        app.selection.toggle(2);
        app.mode = Mode::Visual;
        app.complete_selected();
        assert!(app.tasks()[0].done);
        assert!(!app.tasks()[1].done);
        assert!(app.tasks()[2].done);
        assert!(app.selection.is_empty());
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.flash_active(), Some("completed 2"));
    }

    #[test]
    fn complete_selected_reports_recurring_spawns() {
        let mut app = build_app("a\nPay rent due:2026-04-15 rec:+1m\nb\nWater plants rec:1w\n");
        app.refresh_today("2026-05-09".into());
        app.selection.toggle(1);
        app.selection.toggle(3);
        app.mode = Mode::Visual;
        app.complete_selected();
        assert_eq!(app.tasks().len(), 6);
        assert_eq!(app.flash_active(), Some("completed 2 (+2 recurring)"));
    }

    #[test]
    fn delete_selected_removes_all_in_selection() {
        let mut app = build_app("a\nb\nc\nd\n");
        app.selection.toggle(1);
        app.selection.toggle(3);
        app.mode = Mode::Visual;
        app.delete_selected();
        assert_eq!(app.tasks().len(), 2);
        assert_eq!(app.tasks()[0].raw, "a");
        assert_eq!(app.tasks()[1].raw, "c");
        assert!(app.selection.is_empty());
        assert_eq!(app.mode, Mode::Normal);
    }
}
