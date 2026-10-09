//! Google Calendar: tasq's dated tasks in a "tasq" calendar, each in its
//! space's colour.
//!
//! tasq talks to Google straight from your computer: no server of ours in
//! between. [`desired`] says what each task looks like as an event;
//! [`sync::sync`] sends only what changed since last time.

pub mod auth;
pub mod colors;
pub mod event;
pub mod google;
pub mod http;
pub mod sync;

use std::collections::BTreeMap;

use serde_json::Value;

use crate::todo::Task;

/// A space's colour by name, if it has one.
pub type SpaceRgb<'a> = &'a dyn Fn(&str) -> Option<(u8, u8, u8)>;

/// Every task that belongs in the calendar, as its event body, by task id.
/// `space_rgb` gives a space's colour (the theme's, as drawn); the event
/// takes the nearest of Google's. Tasks without an id (a todo.txt opened
/// directly) can't be followed from one sync to the next, so they stay out.
pub fn desired(tasks: &[Task], space_rgb: SpaceRgb, time_zone: &str) -> BTreeMap<String, Value> {
    tasks
        .iter()
        .filter(|t| !t.id.is_empty())
        .filter_map(|t| {
            let color = t
                .projects
                .first()
                .and_then(|p| space_rgb(p))
                .map(colors::nearest);
            let cx = event::Context {
                time_zone,
                color_id: color,
            };
            Some((t.id.clone(), event::body(t, &cx)?))
        })
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::todo::parse_line;

    #[test]
    fn dated_tasks_go_coloured_by_their_space() {
        let mut tasks: Vec<Task> = [
            "Exam +Uni/Exams plan:2026-10-12 at:09:00",
            "Rent +Home plan:2026-11-01",
            "Someday +Home",
            "x 2026-10-01 Old +Uni plan:2026-10-01",
        ]
        .iter()
        .map(|l| parse_line(l).unwrap())
        .collect();
        for (i, t) in tasks.iter_mut().enumerate() {
            t.id = format!("t{i}");
        }
        let rgb = |p: &str| -> Option<(u8, u8, u8)> {
            Some(if p.starts_with("Uni") {
                (0x8a, 0xad, 0xf4)
            } else {
                (0xa6, 0xda, 0x95)
            })
        };
        let d = desired(&tasks, &rgb, "Europe/Madrid");
        assert_eq!(d.keys().collect::<Vec<_>>(), ["t0", "t1"]);
        assert_eq!(d["t0"]["colorId"], "1");
        assert_eq!(d["t1"]["colorId"], "2");
    }
}
