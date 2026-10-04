use super::App;
use super::types::{Sort, View};
use crate::core::filter::{self, ListDueBucket};
use crate::core::spaces;
use crate::todo::Task;

/// One entry per visible row, parallel to `visible_cache`. Renderers detect
/// group transitions by comparing successive entries; under `Sort::File` every
/// row is `None` so the renderer skips headers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupKey {
    None,
    ArchiveDate(String),
    /// `Some('A'..='Z')` for a graded priority, `None` for unprioritized.
    ListPriority(Option<char>),
    ListDue(ListDueBucket),
    /// Upcoming view: a day of the coming week (`YYYY-MM-DD`), or `None`
    /// for Later.
    Day(Option<String>),
    /// Today, by part of the day: late, morning, afternoon, evening, any
    /// time (see [`TodaySlot`]).
    Slot(TodaySlot),
}

/// Where a task sits in Today.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TodaySlot {
    /// Planned or due before today, still open.
    Late,
    Morning,
    Afternoon,
    Evening,
    /// No time of day.
    AnyTime,
}

impl TodaySlot {
    pub fn label(self) -> &'static str {
        match self {
            TodaySlot::Late => "OVERDUE",
            TodaySlot::Morning => "MORNING",
            TodaySlot::Afternoon => "AFTERNOON",
            TodaySlot::Evening => "EVENING",
            TodaySlot::AnyTime => "ANY TIME",
        }
    }

    fn of(t: &Task, today: &str) -> (Self, u32) {
        let late = !t.done && t.date().is_some_and(|d| d < today);
        let at = crate::todo::find_kv(&t.clean_raw, "at")
            .and_then(|v| crate::core::calendar::parse_time(&v));
        let slot = match (late, at) {
            (true, _) => TodaySlot::Late,
            (false, Some(m)) if m < 12 * 60 => TodaySlot::Morning,
            (false, Some(m)) if m < 18 * 60 => TodaySlot::Afternoon,
            (false, Some(_)) => TodaySlot::Evening,
            (false, None) => TodaySlot::AnyTime,
        };
        (slot, at.unwrap_or(u32::MAX))
    }
}

impl App {
    /// Indices into the active view's task source after filter + sort, in
    /// display order. The source is `archive().tasks()` in Archive view,
    /// `tasks()` otherwise. Reads the cache populated by `recompute_visible`.
    pub fn visible_indices(&self) -> &[usize] {
        &self.visible_cache
    }

    /// Group key per row, parallel to `visible_indices()`.
    pub fn visible_groups(&self) -> &[GroupKey] {
        &self.visible_groups
    }

    /// Recompute the cached visible-index list and parallel group keys. Call
    /// after any mutation that affects filter/sort/view/tasks/archive.
    pub fn recompute_visible(&mut self) {
        match self.view {
            View::List => self.rebuild_list_cache(),
            View::Archive => self.rebuild_archive_cache(),
        }
    }

    fn rebuild_list_cache(&mut self) {
        let tasks = self.store.tasks();
        let today = self.store.today();
        // Resolved once, not per task — a `due:` term can drive a
        // business-day walk in `threshold::shift`.
        let needle = (!self.filter.search.is_empty())
            .then(|| filter::resolve_needle(&self.filter.search, today));

        let scope = self.prefs.scope;
        let known = self.store.known_spaces();
        let open = self.filter.project.as_deref();
        // Today also keeps what you ticked off today, for the progress.
        let is_today = scope == super::types::Scope::Today;
        let mut idxs: Vec<usize> = (0..tasks.len())
            .filter(|&i| filter::in_scope(&tasks[i], scope, today))
            .filter(|&i| !spaces::hidden_from_view(&tasks[i].projects, known, open))
            .filter(|&i| {
                let t = &tasks[i];
                !(is_today && t.done && t.done_date.as_deref() != Some(today))
            })
            .filter(|&i| {
                filter::list_predicate(
                    &tasks[i],
                    self.prefs.show_done || is_today,
                    self.prefs.show_future,
                    today,
                    &self.filter,
                    needle.as_ref(),
                )
            })
            .collect();

        // Today reads as the day: late first, then morning to evening by
        // time, then what has no time.
        if is_today {
            filter::sort_by_prefs(&mut idxs, tasks, self.prefs.sort);
            idxs.sort_by_key(|&i| TodaySlot::of(&tasks[i], today));
            let groups: Vec<GroupKey> = idxs
                .iter()
                .map(|&i| GroupKey::Slot(TodaySlot::of(&tasks[i], today).0))
                .collect();
            float_starred_within_groups(&mut idxs, &groups, tasks);
            self.visible_groups = groups;
            self.visible_cache = idxs;
            return;
        }

        // Upcoming reads as a calendar: by day, then as usual within it.
        if scope == super::types::Scope::Upcoming {
            filter::sort_by_prefs(&mut idxs, tasks, self.prefs.sort);
            idxs.sort_by(|&a, &b| {
                let key = |i: usize| tasks[i].date().unwrap_or("9999").to_string();
                key(a).cmp(&key(b))
            });
            let groups: Vec<GroupKey> = idxs
                .iter()
                .map(|&i| GroupKey::Day(filter::upcoming_day(&tasks[i], today)))
                .collect();
            float_starred_within_groups(&mut idxs, &groups, tasks);
            self.visible_groups = groups;
            self.visible_cache = idxs;
            return;
        }

        filter::sort_by_prefs(&mut idxs, tasks, self.prefs.sort);

        let week_start = &self.week_start;

        let groups: Vec<GroupKey> = match self.prefs.sort {
            Sort::File => vec![GroupKey::None; idxs.len()],
            Sort::Priority => idxs
                .iter()
                .map(|&i| GroupKey::ListPriority(tasks[i].priority))
                .collect(),
            Sort::Due => idxs
                .iter()
                .map(|&i| GroupKey::ListDue(filter::due_bucket(&tasks[i], today, week_start)))
                .collect(),
        };
        // Starred tasks float to the top of their own group (priority
        // bucket, or due bucket), keeping the sort order among themselves
        // and among the rest. Plain file order is left untouched.
        if self.prefs.sort != Sort::File {
            float_starred_within_groups(&mut idxs, &groups, tasks);
        }
        self.visible_groups = groups;
        self.visible_cache = idxs;
    }

    fn rebuild_archive_cache(&mut self) {
        let archive = self.store.archive().tasks();
        let mut idxs: Vec<usize> = (0..archive.len()).collect();
        idxs.sort_by(|&a, &b| {
            archive[b]
                .done_date
                .as_deref()
                .unwrap_or("")
                .cmp(archive[a].done_date.as_deref().unwrap_or(""))
        });
        let groups: Vec<GroupKey> = idxs
            .iter()
            .map(|&i| {
                let date = archive[i]
                    .done_date
                    .clone()
                    .unwrap_or_else(|| "unknown".into());
                GroupKey::ArchiveDate(date)
            })
            .collect();
        self.visible_cache = idxs;
        self.visible_groups = groups;
    }

    pub fn cur_abs(&self) -> Option<usize> {
        self.visible_cache.get(self.cursor).copied()
    }

    pub fn clamp_cursor(&mut self) {
        let len = self.visible_cache.len();
        if len == 0 {
            self.cursor = 0;
        } else if self.cursor >= len {
            self.cursor = len - 1;
        }
    }

    /// Move the cursor to wherever `abs` lives in the current visible list.
    /// Falls back to clamping if `abs` was filtered out.
    pub(super) fn follow_cursor(&mut self, abs: usize) {
        if let Some(pos) = self.visible_cache.iter().position(|&i| i == abs) {
            self.cursor = pos;
        } else {
            self.clamp_cursor();
        }
    }
}

/// Stable-partition each run of equal `groups` entries so starred tasks
/// come first within it. `groups` runs are contiguous because the list was
/// just sorted by the key the groups are derived from.
fn float_starred_within_groups(idxs: &mut [usize], groups: &[GroupKey], tasks: &[Task]) {
    let mut start = 0;
    while start < idxs.len() {
        let end = start
            + groups[start..]
                .iter()
                .take_while(|g| **g == groups[start])
                .count();
        idxs[start..end].sort_by_key(|&i| !tasks[i].starred);
        start = end;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_support::build_app;
    use crate::core::filter::ListDueBucket;

    fn visible_bodies(app: &App) -> Vec<String> {
        app.visible_indices()
            .iter()
            .map(|&i| crate::todo::body_only(&app.tasks()[i].raw))
            .collect()
    }

    #[test]
    fn starred_tasks_float_to_the_top_of_their_priority_group() {
        let mut app = build_app(
            "(A) a1 due:2026-05-01\n(A) a2 star:1 due:2026-06-01\n(B) b1\n(B) b2 star:1\nnone1\nnone2 star:1\n",
        );
        app.prefs.sort = Sort::Priority;
        app.recompute_visible();
        assert_eq!(
            visible_bodies(&app),
            ["a2", "a1", "b2", "b1", "none2", "none1"],
            "starred first within A, B and unprioritized — never above a higher priority"
        );
    }

    #[test]
    fn starred_tasks_float_within_due_buckets_but_not_in_file_order() {
        let mut app = build_app("x1 due:2026-05-01\nx2 star:1 due:2026-05-03\nlater star:1\n");
        app.prefs.sort = Sort::File;
        app.recompute_visible();
        assert_eq!(visible_bodies(&app), ["x1", "x2", "later"]);
    }

    #[test]
    fn search_matches_subsequence() {
        let mut app = build_app("2026-05-01 Call dentist\n2026-05-01 buy milk\n");
        app.filter.search = "cade".into();
        app.recompute_visible();
        assert_eq!(app.visible_indices().len(), 1);
    }

    #[test]
    fn search_matches_body_not_dates() {
        let mut app = build_app("2026-05-01 buy milk\n2026-04-01 something else\n");
        app.filter.search = "2026".into();
        app.recompute_visible();
        assert_eq!(app.visible_indices().len(), 0);
    }

    #[test]
    fn search_due_range_matches_by_due_field_not_literal_text() {
        let mut app =
            build_app("in range due:2026-05-10\nout of range due:2026-05-20\nno due date here\n");
        app.filter.search = "due:+1w".into();
        app.recompute_visible();
        assert_eq!(app.visible_indices().len(), 1);
    }

    #[test]
    fn visible_cache_updates_after_mutation() {
        let mut app = build_app("a\nb\nc\n");
        assert_eq!(app.visible_indices().len(), 3);
        app.draft_set("d".into());
        app.add_from_draft();
        assert_eq!(app.visible_indices().len(), 4);
    }

    #[test]
    fn list_cursor_survives_archive_roundtrip() {
        let mut app = build_app("a\nb\nc\nd\ne\n");
        app.cursor = 3;
        app.set_view(View::Archive);
        app.set_view(View::List);
        assert_eq!(app.cursor, 3, "cursor lost on List → Archive → List");
    }

    #[test]
    fn archive_indices_point_into_archive_tasks() {
        let mut app = build_app("a\n");
        let path = app.archive().path().to_path_buf();
        app.store.archive = crate::app::Archive::for_test(
            crate::todo::parse_file(
                "x 2026-05-01 2026-04-01 first\nx 2026-05-02 2026-04-02 second\n",
            ),
            String::new(),
            path,
        );
        app.set_view(View::Archive);
        let idxs = app.visible_indices();
        assert_eq!(idxs.len(), 2);
        for &i in idxs {
            assert!(app.archive().tasks().get(i).is_some());
        }
    }

    #[test]
    fn list_groups_are_none_under_sort_file() {
        let mut app = build_app("(A) a\n(B) b\nc\n");
        app.prefs.sort = Sort::File;
        app.recompute_visible();
        let groups = app.visible_groups();
        assert_eq!(groups.len(), 3);
        for g in groups {
            assert!(matches!(g, GroupKey::None));
        }
    }

    #[test]
    fn list_groups_track_priority_under_sort_priority() {
        let mut app = build_app("(A) a\n(B) b\nc\n(A) a2\n");
        app.prefs.sort = Sort::Priority;
        app.recompute_visible();
        let groups = app.visible_groups();
        assert_eq!(groups.len(), 4);
        assert_eq!(groups[0], GroupKey::ListPriority(Some('A')));
        assert_eq!(groups[1], GroupKey::ListPriority(Some('A')));
        assert_eq!(groups[2], GroupKey::ListPriority(Some('B')));
        assert_eq!(groups[3], GroupKey::ListPriority(None));
    }

    #[test]
    fn list_groups_bucket_due_dates_under_sort_due() {
        let raw = "a due:2026-05-04\n\
                   b due:2026-05-06\n\
                   c due:2026-05-08\n\
                   d due:2026-05-15\n\
                   e due:2026-05-25\n\
                   f\n";
        let mut app = build_app(raw);
        app.prefs.sort = Sort::Due;
        app.recompute_visible();
        let groups = app.visible_groups();
        assert_eq!(groups.len(), 6);
        assert_eq!(groups[0], GroupKey::ListDue(ListDueBucket::Overdue));
        assert_eq!(groups[1], GroupKey::ListDue(ListDueBucket::Today));
        assert_eq!(groups[2], GroupKey::ListDue(ListDueBucket::ThisWeek));
        assert_eq!(groups[3], GroupKey::ListDue(ListDueBucket::NextWeek));
        assert_eq!(groups[4], GroupKey::ListDue(ListDueBucket::Later));
        assert_eq!(groups[5], GroupKey::ListDue(ListDueBucket::NoDue));
    }

    #[test]
    fn future_absolute_threshold_hidden_by_default() {
        let mut app = build_app("future task t:2030-01-01\nvisible task\n");
        assert_eq!(app.visible_indices().len(), 1);
        assert_eq!(app.tasks()[app.visible_indices()[0]].raw, "visible task");
        app.prefs.show_future = true;
        app.recompute_visible();
        assert_eq!(app.visible_indices().len(), 2);
    }

    #[test]
    fn relative_threshold_anchors_on_due() {
        let mut app = build_app("Pay rent due:2026-05-15 t:-3d\n");
        assert_eq!(app.visible_indices().len(), 0);
        app.prefs.show_future = true;
        app.recompute_visible();
        assert_eq!(app.visible_indices().len(), 1);
    }

    #[test]
    fn refresh_today_unhides_tasks_when_date_advances() {
        let mut app = build_app("future task t:2026-05-07\nvisible task\n");
        assert_eq!(app.visible_indices().len(), 1);
        let changed = app.refresh_today("2026-05-07".into());
        assert!(changed);
        assert_eq!(app.today(), "2026-05-07");
        assert_eq!(app.visible_indices().len(), 2);
    }

    #[test]
    fn refresh_today_is_noop_when_date_unchanged() {
        let mut app = build_app("a\n");
        let changed = app.refresh_today("2026-05-06".into());
        assert!(!changed);
        assert_eq!(app.today(), "2026-05-06");
    }

    #[test]
    fn archive_visible_groups_are_done_date_desc() {
        let mut app = build_app("a\n");
        let path = app.archive().path().to_path_buf();
        app.store.archive = crate::app::Archive::for_test(
            crate::todo::parse_file(
                "x 2026-04-01 2026-03-01 older\nx 2026-05-02 2026-04-02 newer\n",
            ),
            String::new(),
            path,
        );
        app.set_view(View::Archive);
        let groups = app.visible_groups();
        assert_eq!(groups.len(), 2);
        let first = match &groups[0] {
            GroupKey::ArchiveDate(d) => d.as_str(),
            _ => panic!("expected ArchiveDate"),
        };
        let second = match &groups[1] {
            GroupKey::ArchiveDate(d) => d.as_str(),
            _ => panic!("expected ArchiveDate"),
        };
        assert_eq!(first, "2026-05-02");
        assert_eq!(second, "2026-04-01");
    }
}
