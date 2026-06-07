//! Pure filtering, sorting, and grouping helpers over `[Task]`.
//!
//! These take plain data (`&Task` / `&[Task]` + `Filter`/`Sort` + flags) and
//! return decisions or orderings. They hold no view state, so both the TUI's
//! `recompute_visible` (which owns the visible-index cache) and the CLI's
//! `list`/`listpri`/`listproj`/`listcon` commands reuse them.

use std::cmp::Ordering;

use crate::app::{Filter, Sort};
use crate::search::subseq_match_ci;
use crate::threshold;
use crate::todo::{self, Task};

/// Which canonical bucket a List-view row belongs to when the active sort is
/// `Sort::Due`. `NoDue` covers tasks with no `due:` tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListDueBucket {
    Overdue,
    Today,
    Upcoming,
    NoDue,
}

impl ListDueBucket {
    pub fn label(self) -> &'static str {
        match self {
            ListDueBucket::Overdue => "OVERDUE",
            ListDueBucket::Today => "TODAY",
            ListDueBucket::Upcoming => "UPCOMING",
            ListDueBucket::NoDue => "NO DUE DATE",
        }
    }
}

pub fn due_bucket(task: &Task, today: &str) -> ListDueBucket {
    match task.due.as_deref() {
        None => ListDueBucket::NoDue,
        Some(d) => match d.cmp(today) {
            Ordering::Less => ListDueBucket::Overdue,
            Ordering::Equal => ListDueBucket::Today,
            Ordering::Greater => ListDueBucket::Upcoming,
        },
    }
}

pub fn sort_by_prefs(idxs: &mut [usize], tasks: &[Task], sort: Sort) {
    match sort {
        Sort::Priority => idxs.sort_by(cmp_priority(tasks)),
        Sort::Due => idxs.sort_by(cmp_due(tasks)),
        Sort::File => { /* preserve order */ }
    }
}

/// Project / context / search predicate, shared by every view that honors
/// user filters. `needle` matches as a case-insensitive subsequence of the
/// task body — chars must appear in order, gaps allowed.
pub fn passes_user_filter(t: &Task, filter: &Filter, needle: Option<&str>) -> bool {
    if let Some(p) = &filter.project
        && !t.projects.iter().any(|x| x == p)
    {
        return false;
    }
    if let Some(c) = &filter.context
        && !t.contexts.iter().any(|x| x == c)
    {
        return false;
    }
    if let Some(needle) = needle {
        let body = todo::body_after_priority(&t.raw);
        if subseq_match_ci(body, needle).is_none() {
            return false;
        }
    }
    true
}

pub fn list_predicate(
    t: &Task,
    show_done: bool,
    show_future: bool,
    today: &str,
    filter: &Filter,
    needle: Option<&str>,
) -> bool {
    if t.done && !show_done {
        return false;
    }
    if !show_future && is_future_threshold(t, today) {
        return false;
    }
    passes_user_filter(t, filter, needle)
}

/// True when the task carries a `t:` value that resolves to a date strictly
/// after `today`. Malformed values, missing anchors for relative offsets,
/// and arithmetic overflow all leave the task visible — better to surface a
/// task the user might miss than to hide it because of a bad threshold.
pub fn is_future_threshold(t: &Task, today: &str) -> bool {
    let Some(raw) = t.threshold.as_deref() else {
        return false;
    };
    let Some(spec) = threshold::parse_threshold(raw) else {
        return false;
    };
    let Some(date) = threshold::resolve(&spec, t.due.as_deref(), t.created_date.as_deref()) else {
        return false;
    };
    date.format("%Y-%m-%d").to_string().as_str() > today
}

/// Sort by priority asc (None last), tie-broken by due-date asc.
fn cmp_priority(tasks: &[Task]) -> impl Fn(&usize, &usize) -> Ordering + '_ {
    |&a, &b| {
        let ta = &tasks[a];
        let tb = &tasks[b];
        let pa = ta.priority.unwrap_or('Z');
        let pb = tb.priority.unwrap_or('Z');
        pa.cmp(&pb).then_with(|| {
            ta.due
                .as_deref()
                .unwrap_or("z")
                .cmp(tb.due.as_deref().unwrap_or("z"))
        })
    }
}

/// Sort by due-date asc (None last).
fn cmp_due(tasks: &[Task]) -> impl Fn(&usize, &usize) -> Ordering + '_ {
    |&a, &b| {
        tasks[a]
            .due
            .as_deref()
            .unwrap_or("z")
            .cmp(tasks[b].due.as_deref().unwrap_or("z"))
    }
}

/// Order projects/contexts the same way the filter sidebar does:
/// count descending, then name ascending. Used by both the picker and
/// the sidebar so j/k advances visibly down the list.
pub fn ordered_unique<F>(tasks: &[Task], pick: F) -> Vec<(String, usize)>
where
    F: Fn(&Task) -> &Vec<String>,
{
    use std::collections::BTreeMap;
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for t in tasks.iter().filter(|t| !t.done) {
        for v in pick(t) {
            *counts.entry(v.clone()).or_insert(0) += 1;
        }
    }
    let mut out: Vec<(String, usize)> = counts.into_iter().collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    out
}

pub fn unique_values<F>(tasks: &[Task], pick: F) -> Vec<String>
where
    F: Fn(&Task) -> &Vec<String>,
{
    ordered_unique(tasks, pick)
        .into_iter()
        .map(|(n, _)| n)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_values_dedups_and_sorts() {
        let raw = "(A) 2026-05-01 a +work +health\n2026-05-01 b +work\n2026-05-01 c +health\n";
        let tasks = crate::todo::parse_file(raw);
        let projects = unique_values(&tasks, |t| &t.projects);
        assert_eq!(projects, vec!["health".to_string(), "work".to_string()]);
    }
}
