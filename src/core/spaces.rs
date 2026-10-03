//! Spaces: the part of your life a task belongs to, as a tree.
//!
//! A space is a task's `+project`; a `/` nests it, so `+Uni/Exams` is the
//! Exams space inside Uni. That keeps every line plain todo.txt (other
//! tools see an ordinary project) while tasq shows a tree: a task in
//! Uni/Exams also belongs to Uni, and opening Uni shows both.
//!
//! With a database, every space a task has used is also a row of its own
//! ([`Space`]), so a space outlives its last task and can carry settings.

use crate::todo::Task;

/// Separator between a space and its sub-space in a `+project` tag.
pub const SEP: char = '/';

/// Whether a task with these `+project`s lives in `space` or one of its
/// sub-spaces.
pub fn in_space(projects: &[String], space: &str) -> bool {
    projects.iter().any(|p| is_within(p, space))
}

/// Whether the space `path` is `space` itself or nested inside it.
pub fn is_within(path: &str, space: &str) -> bool {
    path == space
        || path
            .strip_prefix(space)
            .is_some_and(|rest| rest.starts_with(SEP))
}

/// How a space path reads: `Uni/Exams` → `Uni › Exams`.
pub fn display(path: &str) -> String {
    path.split(SEP).collect::<Vec<_>>().join(" › ")
}

/// The last part of a space path: `Uni/Exams` → `Exams`.
pub fn leaf(path: &str) -> &str {
    path.rsplit(SEP).next().unwrap_or(path)
}

/// A space the database keeps, whether or not any task uses it now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Space {
    /// Full path, e.g. `Uni/Exams`.
    pub path: String,
    pub hidden: bool,
}

/// A path and every space above it: `Uni/Exams` → `Uni`, `Uni/Exams`.
pub fn with_ancestors(path: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut prefix = String::new();
    for part in path.split(SEP).filter(|s| !s.is_empty()) {
        if !prefix.is_empty() {
            prefix.push(SEP);
        }
        prefix.push_str(part);
        out.push(prefix.clone());
    }
    out
}

/// The path `path` takes when the space `from` is renamed to `to`, or
/// `None` when it isn't `from` or inside it: renaming `Uni` to `School`
/// moves `Uni/Exams` to `School/Exams`.
pub fn renamed(path: &str, from: &str, to: &str) -> Option<String> {
    is_within(path, from).then(|| format!("{to}{}", &path[from.len()..]))
}

/// One row of the space tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpaceRow {
    /// Full path, e.g. `Uni/Exams`.
    pub path: String,
    /// 0 for a top-level space.
    pub depth: usize,
    /// Tasks in it or any of its sub-spaces (each counted once).
    pub count: usize,
    /// Hidden, itself or through a space above it.
    pub hidden: bool,
}

/// The hidden space `path` is in — itself or the nearest one above it —
/// if any.
pub fn hidden_by<'a>(path: &str, known: &'a [Space]) -> Option<&'a str> {
    known
        .iter()
        .filter(|s| s.hidden && is_within(path, &s.path))
        .max_by_key(|s| s.path.len())
        .map(|s| s.path.as_str())
}

/// Whether a task with these `+project`s is out of sight: it lives in a
/// hidden space, and the space being looked at (`open`) isn't that space or
/// one inside it. Opening a hidden space shows its tasks.
pub fn hidden_from_view(projects: &[String], known: &[Space], open: Option<&str>) -> bool {
    known
        .iter()
        .filter(|s| s.hidden)
        .any(|h| in_space(projects, &h.path) && !open.is_some_and(|o| is_within(o, &h.path)))
}

/// Every space the open tasks use plus the `known` ones (kept in the
/// database, possibly empty), with the parents of nested ones, as a tree in
/// display order: siblings by open-task count (most first), then by name.
/// Completed tasks don't count, like everywhere in the sidebar.
pub fn tree(tasks: &[Task], known: &[Space]) -> Vec<SpaceRow> {
    let open: Vec<&Task> = tasks.iter().filter(|t| !t.done).collect();
    let mut paths: Vec<String> = Vec::new();
    let used = open.iter().flat_map(|t| t.projects.iter());
    for p in used.chain(known.iter().map(|s| &s.path)) {
        for path in with_ancestors(p) {
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
    }
    let count = |path: &str| open.iter().filter(|t| in_space(&t.projects, path)).count();
    let mut out = Vec::new();
    push_children(&paths, None, 0, &count, &mut out);
    for row in &mut out {
        row.hidden = hidden_by(&row.path, known).is_some();
    }
    out
}

fn push_children(
    paths: &[String],
    parent: Option<&str>,
    depth: usize,
    count: &dyn Fn(&str) -> usize,
    out: &mut Vec<SpaceRow>,
) {
    let mut children: Vec<(&String, usize)> = paths
        .iter()
        .filter(|p| match parent {
            None => !p.contains(SEP),
            Some(parent) => p
                .strip_prefix(parent)
                .and_then(|rest| rest.strip_prefix(SEP))
                .is_some_and(|rest| !rest.is_empty() && !rest.contains(SEP)),
        })
        .map(|p| (p, count(p)))
        .collect();
    children.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    for (path, n) in children {
        out.push(SpaceRow {
            path: path.clone(),
            depth,
            count: n,
            hidden: false,
        });
        push_children(paths, Some(path), depth + 1, count, out);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::todo::parse_file;

    #[test]
    fn membership_includes_sub_spaces() {
        let p = vec!["Uni/Exams".to_string()];
        assert!(in_space(&p, "Uni"));
        assert!(in_space(&p, "Uni/Exams"));
        assert!(!in_space(&p, "Uni/Labs"));
        assert!(
            !in_space(&["University".to_string()], "Uni"),
            "not a prefix match"
        );
        assert_eq!(display("Uni/Exams"), "Uni › Exams");
        assert_eq!(leaf("Uni/Exams"), "Exams");
    }

    #[test]
    fn tree_nests_and_counts_each_task_once() {
        let tasks = parse_file(
            "study +Uni/Exams\n\
             lab report +Uni/Labs\n\
             pay tuition +Uni\n\
             mock exam +Uni/Exams\n\
             boxes +Personal/Moving\n\
             x 2026-10-01 2026-09-30 old +Uni/Exams\n\
             x 2026-10-01 2026-09-30 gone +Archived\n\
             loose task\n",
        );
        let rows: Vec<(String, usize, usize)> = tree(&tasks, &[])
            .into_iter()
            .map(|r| (r.path, r.depth, r.count))
            .collect();
        assert_eq!(
            rows,
            [
                ("Uni".to_string(), 0, 4),
                ("Uni/Exams".to_string(), 1, 2),
                ("Uni/Labs".to_string(), 1, 1),
                ("Personal".to_string(), 0, 1),
                ("Personal/Moving".to_string(), 1, 1),
            ]
        );
    }

    #[test]
    fn known_spaces_show_up_empty() {
        let tasks = parse_file("study +Uni/Exams\n");
        let known = [Space {
            path: "Personal/Moving".to_string(),
            hidden: false,
        }];
        let rows: Vec<(String, usize)> = tree(&tasks, &known)
            .into_iter()
            .map(|r| (r.path, r.count))
            .collect();
        assert_eq!(
            rows,
            [
                ("Uni".to_string(), 1),
                ("Uni/Exams".to_string(), 1),
                ("Personal".to_string(), 0),
                ("Personal/Moving".to_string(), 0),
            ]
        );
    }

    #[test]
    fn renaming_moves_sub_spaces() {
        assert_eq!(with_ancestors("Uni/Exams"), ["Uni", "Uni/Exams"]);
        assert_eq!(renamed("Uni", "Uni", "School").as_deref(), Some("School"));
        assert_eq!(
            renamed("Uni/Exams", "Uni", "School").as_deref(),
            Some("School/Exams")
        );
        assert_eq!(renamed("University", "Uni", "School"), None);
    }

    #[test]
    fn hiding_a_space_hides_its_sub_spaces_unless_opened() {
        let known = [
            Space {
                path: "Uni".to_string(),
                hidden: true,
            },
            Space {
                path: "Personal/Moving".to_string(),
                hidden: true,
            },
        ];
        let exams = vec!["Uni/Exams".to_string()];
        assert!(hidden_from_view(&exams, &known, None));
        assert!(hidden_from_view(&exams, &known, Some("Personal")));
        assert!(!hidden_from_view(&exams, &known, Some("Uni")));
        assert!(!hidden_from_view(&exams, &known, Some("Uni/Exams")));
        assert!(!hidden_from_view(&["Personal".to_string()], &known, None));
        assert_eq!(hidden_by("Uni/Exams", &known), Some("Uni"));
        assert_eq!(hidden_by("Personal", &known), None);

        let tasks =
            crate::todo::parse_file("study +Uni/Exams\nboxes +Personal/Moving\nbills +Personal\n");
        let rows: Vec<(String, bool)> = tree(&tasks, &known)
            .into_iter()
            .map(|r| (r.path, r.hidden))
            .collect();
        assert_eq!(
            rows,
            [
                ("Personal".to_string(), false),
                ("Personal/Moving".to_string(), true),
                ("Uni".to_string(), true),
                ("Uni/Exams".to_string(), true),
            ]
        );
    }
}
