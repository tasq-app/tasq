//! The "+ filter" popover (`f`): a search box over everything you can
//! filter by — spaces in their colours, tags, deadlines, priority, your
//! saved views — each with how many open tasks it has. Picking one adds it
//! as a chip over the list (or takes it off, if it's on); combinations can
//! be saved as a view for the sidebar.

use super::App;
use super::types::{Filter, Mode, Preset};
use crate::core::{filter, spaces};
use crate::search::subseq_match_ci;

/// Deadline filters: label and the `due:` search term behind it.
pub const DUE_TERMS: [(&str, &str); 3] = [
    ("due today", "due:+0d"),
    ("due within a week", "due:+1w"),
    ("due within a month", "due:+1m"),
];

/// What a popover row does when picked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PopPick {
    Space(String),
    Tag(String),
    Due(&'static str),
    Preset(Preset),
    Saved(usize),
    Clear,
    Save,
}

/// One row of the popover, under a section heading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PopRow {
    pub section: &'static str,
    pub label: String,
    pub count: Option<usize>,
    /// Already one of the active filters.
    pub on: bool,
    pub pick: PopPick,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FilterPop {
    pub query: String,
    pub cursor: usize,
}

impl App {
    /// `f`: open the popover.
    pub fn open_filters(&mut self) {
        self.filter_pop = FilterPop::default();
        self.mode = Mode::Filters;
    }

    pub fn close_filters(&mut self) {
        self.filter_pop = FilterPop::default();
        self.mode = Mode::Normal;
    }

    /// The rows matching what's typed, section by section.
    pub fn filter_rows(&self) -> Vec<PopRow> {
        let tasks = self.store.tasks();
        let today = self.store.today();
        let open_through = |f: &Filter| {
            let needle = filter::resolve_needle(&f.search, today);
            tasks
                .iter()
                .filter(|t| !t.done && filter::passes_user_filter(t, f, Some(&needle)))
                .count()
        };
        let mut rows: Vec<PopRow> = Vec::new();
        let mut push = |section, label: String, count, on, pick| {
            rows.push(PopRow {
                section,
                label,
                count,
                on,
                pick,
            });
        };

        for s in self.store.space_tree() {
            let on = self.filter.project.as_deref() == Some(s.path.as_str());
            push(
                "SPACES",
                spaces::display(&s.path),
                Some(s.count),
                on,
                PopPick::Space(s.path),
            );
        }
        for (c, n) in filter::ordered_unique(tasks, |t| &t.contexts) {
            let on = self.filter.context.as_deref() == Some(c.as_str());
            push("TAGS", c.clone(), Some(n), on, PopPick::Tag(c));
        }
        for (label, term) in DUE_TERMS {
            let f = Filter {
                search: term.to_string(),
                ..Filter::default()
            };
            let on = self.filter.search == term;
            push(
                "WHEN",
                label.to_string(),
                Some(open_through(&f)),
                on,
                PopPick::Due(term),
            );
        }
        let over = Filter {
            preset: Some(Preset::Overdue),
            ..Filter::default()
        };
        push(
            "WHEN",
            "overdue".to_string(),
            Some(open_through(&over)),
            self.filter.preset == Some(Preset::Overdue),
            PopPick::Preset(Preset::Overdue),
        );
        for p in [Preset::HighPriority, Preset::Starred] {
            let f = Filter {
                preset: Some(p),
                ..Filter::default()
            };
            push(
                "PRIORITY",
                p.label().to_lowercase(),
                Some(open_through(&f)),
                self.filter.preset == Some(p),
                PopPick::Preset(p),
            );
        }
        for (i, v) in self.saved_filters().iter().enumerate() {
            let f = Filter::from_query(&v.query);
            let on = self.filter.has_any() && f.same_as(&self.filter);
            push(
                "VIEWS",
                v.name.clone(),
                Some(open_through(&f)),
                on,
                PopPick::Saved(i),
            );
        }
        if self.filter.has_any() {
            push(
                "SAVE",
                format!("Save \"{}\" as a view…", self.filter_summary()),
                None,
                false,
                PopPick::Save,
            );
            push(
                "SAVE",
                "Clear all filters".to_string(),
                None,
                false,
                PopPick::Clear,
            );
        }

        let q = self.filter_pop.query.trim();
        if !q.is_empty() {
            rows.retain(|r| {
                matches!(r.pick, PopPick::Save | PopPick::Clear)
                    || subseq_match_ci(&r.label, q).is_some()
            });
        }
        rows
    }

    /// The active filters in a few words: `Uni › Exams + @lab`.
    pub fn filter_summary(&self) -> String {
        let f = &self.filter;
        let mut parts: Vec<String> = Vec::new();
        if let Some(p) = &f.project {
            parts.push(spaces::display(p));
        }
        if let Some(c) = &f.context {
            parts.push(format!("@{c}"));
        }
        if let Some((label, _)) = DUE_TERMS.iter().find(|(_, t)| *t == f.search) {
            parts.push((*label).to_string());
        } else if !f.search.is_empty() {
            parts.push(f.search.clone());
        }
        if let Some(p) = f.preset {
            parts.push(p.label().to_lowercase());
        }
        parts.join(" + ")
    }

    pub fn filter_pop_type(&mut self, c: char) {
        self.filter_pop.query.push(c);
        self.filter_pop.cursor = 0;
    }

    /// Backspace: a letter off the search, or with nothing typed, the last
    /// chip off the list.
    pub fn filter_pop_backspace(&mut self) {
        if self.filter_pop.query.pop().is_some() {
            self.filter_pop.cursor = 0;
            return;
        }
        // Chips read space, tag, search, preset: the last one goes first.
        let f = &mut self.filter;
        if f.preset.is_some() {
            f.preset = None;
        } else if !f.search.is_empty() {
            f.search.clear();
        } else if f.context.is_some() {
            f.context = None;
        } else {
            f.project = None;
        }
        self.recompute_visible();
        self.filter_pop.cursor = 0;
    }

    pub fn filter_pop_move(&mut self, forward: bool) {
        let n = self.filter_rows().len();
        self.filter_pop.cursor = if forward {
            (self.filter_pop.cursor + 1).min(n.saturating_sub(1))
        } else {
            self.filter_pop.cursor.saturating_sub(1)
        };
    }

    /// Enter: put the row's filter on (or take it off) and close.
    pub fn filter_pop_pick(&mut self) {
        let Some(row) = self.filter_rows().into_iter().nth(self.filter_pop.cursor) else {
            return;
        };
        self.close_filters();
        let f = &mut self.filter;
        match row.pick {
            PopPick::Space(p) => f.project = if row.on { None } else { Some(p) },
            PopPick::Tag(c) => f.context = if row.on { None } else { Some(c) },
            PopPick::Due(term) => {
                f.search = if row.on {
                    String::new()
                } else {
                    term.to_string()
                };
            }
            PopPick::Preset(p) => f.preset = if row.on { None } else { Some(p) },
            PopPick::Saved(i) => {
                if let Some(q) = self.saved_filters().get(i).map(|v| v.query.clone()) {
                    self.filter = Filter::from_query(&q);
                }
            }
            PopPick::Clear => f.clear(),
            PopPick::Save => {
                self.draft_clear();
                self.mode = Mode::PromptSaveFilter;
                return;
            }
        }
        self.calendar = None;
        self.cursor = 0;
        self.recompute_visible();
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::app::test_support::build_app;

    #[test]
    fn the_popover_searches_and_toggles_filters() {
        let mut app = build_app(
            "a +Uni/Exams @lab due:2026-05-06\nb +Uni/Exams\nc +Work/Examples\nd +Home @lab\n",
        );
        app.open_filters();
        for c in "ex".chars() {
            app.filter_pop_type(c);
        }
        let rows = app.filter_rows();
        let labels: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
        assert!(labels.contains(&"Uni › Exams"), "{labels:?}");
        assert!(labels.contains(&"Work › Examples"), "{labels:?}");
        assert!(!labels.contains(&"Home"), "{labels:?}");
        let exams = rows.iter().position(|r| r.label == "Uni › Exams").unwrap();
        assert_eq!(rows[exams].count, Some(2));

        app.filter_pop.cursor = exams;
        app.filter_pop_pick();
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.filter.project.as_deref(), Some("Uni/Exams"));
        assert_eq!(app.visible_indices().len(), 2);

        // A second filter combines with the first.
        app.open_filters();
        let lab = app
            .filter_rows()
            .iter()
            .position(|r| r.label == "lab")
            .unwrap();
        app.filter_pop.cursor = lab;
        app.filter_pop_pick();
        assert_eq!(app.visible_indices().len(), 1);
        assert_eq!(app.filter_summary(), "Uni › Exams + @lab");

        // Due today counts the one due today.
        app.open_filters();
        let today = app
            .filter_rows()
            .into_iter()
            .find(|r| r.label == "due today")
            .unwrap();
        assert_eq!(today.count, Some(1));

        // Backspace with nothing typed takes the last chip off.
        app.filter_pop_backspace();
        assert_eq!(app.filter.context, None);
        assert_eq!(app.filter.project.as_deref(), Some("Uni/Exams"));
        app.filter_pop_backspace();
        assert!(!app.filter.has_any());
    }

    #[test]
    fn a_filter_round_trips_through_a_saved_query() {
        let f = Filter {
            project: Some("Uni/Exams".into()),
            context: Some("lab".into()),
            search: "due:+1w".into(),
            preset: Some(Preset::Starred),
        };
        let q = f.to_query();
        assert_eq!(q, "+Uni/Exams @lab is:starred due:+1w");
        assert!(Filter::from_query(&q).same_as(&f));
        assert_eq!(Filter::from_query("errand stuff").search, "errand stuff");
    }
}
