//! Deleting a space, asked first: `d` on it in the sidebar (its tasks stay,
//! without it), or by itself when an edit leaves a space with no tasks —
//! a mistyped `+Practicasf` fixed on its task shouldn't stay behind.

use std::collections::BTreeSet;

use super::App;
use super::types::Mode;
use crate::core::spaces;

/// A space waiting for "delete it?".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpaceAsk {
    pub path: String,
    /// Tasks that have it (they stay, without it).
    pub tasks: usize,
    /// Asked because the last change left it empty.
    pub emptied: bool,
}

impl App {
    /// Every space a task in the list has, and the spaces above them.
    pub(crate) fn spaces_used(&self) -> BTreeSet<String> {
        self.store
            .tasks()
            .iter()
            .flat_map(|t| t.projects.iter())
            .flat_map(|p| spaces::with_ancestors(p))
            .collect()
    }

    /// After a change: a space the database keeps that a task had until
    /// now and none has any more is offered for deleting.
    pub(crate) fn notice_emptied_space(&mut self) {
        let now = self.spaces_used();
        let Some(before) = self.spaces_in_use.replace(now.clone()) else {
            return;
        };
        let kept: BTreeSet<&str> = self
            .store
            .known_spaces()
            .iter()
            .map(|s| s.path.as_str())
            .collect();
        // The deepest one: "Uni/Practicasf" emptied, not "Uni".
        let gone = before
            .iter()
            .filter(|p| !now.contains(*p) && kept.contains(p.as_str()))
            .max_by_key(|p| p.len());
        if let Some(path) = gone
            && self.space_ask.is_none()
        {
            self.space_ask = Some(SpaceAsk {
                path: path.clone(),
                tasks: 0,
                emptied: true,
            });
        }
    }

    /// `d` on a space: ask before deleting it.
    pub fn begin_delete_space(&mut self) {
        let Some(path) = self.filter.project.clone() else {
            return;
        };
        let tasks = self
            .store
            .tasks()
            .iter()
            .filter(|t| spaces::in_space(&t.projects, &path))
            .count();
        self.space_ask = Some(SpaceAsk {
            path,
            tasks,
            emptied: false,
        });
    }

    /// `y`: the space goes, taken off its tasks (which stay).
    pub fn confirm_delete_space(&mut self) {
        let Some(ask) = self.space_ask.take() else {
            return;
        };
        let name = spaces::display(&ask.path);
        match self.store.remove_space(&ask.path) {
            Ok(n) => {
                if n > 0 {
                    let tasks = if n == 1 { "task" } else { "tasks" };
                    self.flash(format!("space {name} deleted · {n} {tasks} without it"));
                } else {
                    self.flash(format!("space {name} deleted"));
                }
                if self
                    .filter
                    .project
                    .as_deref()
                    .is_some_and(|p| spaces::is_within(p, &ask.path))
                {
                    self.filter.project = None;
                }
                self.mode = Mode::Normal;
                self.cursor = 0;
                self.recompute_visible();
                self.spaces_in_use = Some(self.spaces_used());
                let last = self.sidebar_rows().len().saturating_sub(1);
                self.sidebar_cursor = self.sidebar_cursor.min(last);
            }
            Err(crate::core::DeleteSpaceOutcome::Aborted(r)) => self.handle_reconcile_abort(r),
            Err(crate::core::DeleteSpaceOutcome::Error(e)) => {
                self.flash(format!("delete failed: {e}"));
            }
            Err(_) => {}
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::app::test_support::build_app;

    #[test]
    fn deleting_a_space_asks_and_leaves_its_tasks_without_it() {
        let mut app = build_app("study +Uni/Labs\nwrite +Uni/Labs @desk\nrent +Home\n");
        app.filter.project = Some("Uni".into());
        app.begin_delete_space();
        let ask = app.space_ask.clone().unwrap();
        assert_eq!((ask.path.as_str(), ask.tasks), ("Uni", 2));
        app.confirm_delete_space();
        let raws: Vec<&str> = app.tasks().iter().map(|t| t.raw.as_str()).collect();
        assert_eq!(raws, ["study", "write @desk", "rent +Home"]);
        assert_eq!(app.filter.project, None);
        // One undo brings it all back.
        app.undo();
        assert_eq!(app.tasks()[0].raw, "study +Uni/Labs");
    }

    #[test]
    fn a_space_left_empty_by_an_edit_is_offered_for_deleting() {
        let mut app = build_app("");
        app.store = crate::core::Store::in_memory_db("2026-05-06");
        app.store.add_finalized("practice +Practicasf");
        app.store.add_finalized("other +Practicas");
        app.spaces_in_use = Some(app.spaces_used());
        // The typo fixed on its only task.
        let raw = app.tasks()[0].raw.replace("+Practicasf", "+Practicas");
        app.store.edit_line(0, &raw);
        app.after_mutation(0);
        let ask = app.space_ask.clone().unwrap();
        assert_eq!((ask.path.as_str(), ask.emptied), ("Practicasf", true));
        app.confirm_delete_space();
        let kept: Vec<&str> = app
            .store
            .known_spaces()
            .iter()
            .map(|s| s.path.as_str())
            .collect();
        assert_eq!(kept, ["Practicas"]);
        // Nothing else asks: Practicas still has its tasks.
        app.store.edit_line(1, "other +Practicas @lab");
        app.after_mutation(1);
        assert!(app.space_ask.is_none());
    }
}
