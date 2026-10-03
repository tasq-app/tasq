//! What falls on which day: the occurrences the calendar views draw.
//!
//! A task shows on its planned day, else on its deadline. One with both on
//! different days shows twice: its planned block, and a deadline marker on
//! the due day. A repeating task also shows its future repeats (marked
//! `projected`), up to its `until:` / `times:` end. Lists show only the
//! next occurrence; calendars show them all.

use chrono::NaiveDate;

use crate::recurrence;
use crate::todo::{self, Task};

/// How long a block is drawn when the task has a time but no duration.
pub const DEFAULT_MINUTES: u32 = 30;

/// Upper bound on projected repeats per task, so a daily rule over a long
/// range can't run away.
const MAX_PROJECTED: usize = 400;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Occurrence {
    /// Index of the task in the live list.
    pub abs: usize,
    pub date: NaiveDate,
    /// Start time in minutes after midnight (`at:`); `None` is all-day.
    pub start: Option<u32>,
    /// Duration in minutes (`dur:`, else [`DEFAULT_MINUTES`]).
    pub minutes: u32,
    /// Shown for its deadline rather than its plan.
    pub deadline: bool,
    /// A future repeat, not the task as it stands now.
    pub projected: bool,
    /// Planned or due before today and still open; shown on today.
    pub late: bool,
}

impl Occurrence {
    pub fn end(&self) -> Option<u32> {
        self.start.map(|s| s + self.minutes)
    }
}

fn date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

/// `HH:MM` → minutes after midnight.
pub fn parse_time(s: &str) -> Option<u32> {
    let (h, m) = s.split_once(':')?;
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

/// Every occurrence from `from` to `to` (inclusive), by day, then all-day
/// first, then by start time. Open tasks dated before `today` also show on
/// `today`, marked `late`, when today is in range.
pub fn occurrences(
    tasks: &[Task],
    from: NaiveDate,
    to: NaiveDate,
    today: NaiveDate,
) -> Vec<Occurrence> {
    let mut out = Vec::new();
    for (abs, t) in tasks.iter().enumerate() {
        let body = todo::body_after_priority(&t.clean_raw);
        let start = todo::find_kv(body, "at").and_then(|v| parse_time(&v));
        let minutes = t
            .duration
            .as_deref()
            .and_then(crate::duration::parse_minutes)
            .filter(|m| *m > 0)
            .unwrap_or(DEFAULT_MINUTES);
        let planned = t.planned.as_deref().and_then(date);
        let due = t.due.as_deref().and_then(date);
        // A repeat with no date yet ("gym every day") starts today.
        let undated_repeat = t.rec.is_some() && !t.done;
        let Some(main) = planned.or(due).or(undated_repeat.then_some(today)) else {
            continue;
        };
        let deadline_only = planned.is_none() && due.is_some();
        let occ = |date: NaiveDate, deadline: bool, projected: bool| Occurrence {
            abs,
            date,
            start: if deadline && !deadline_only {
                None
            } else {
                start
            },
            minutes,
            deadline,
            projected,
            late: false,
        };
        let in_range = |d: NaiveDate| d >= from && d <= to;

        if in_range(main) {
            out.push(occ(main, deadline_only, false));
        }
        if let (Some(p), Some(d)) = (planned, due)
            && d != p
            && in_range(d)
        {
            out.push(occ(d, true, false));
        }
        if !t.done && main < today && in_range(today) && (planned.is_some() || due.is_some()) {
            out.push(Occurrence {
                late: true,
                start: None,
                ..occ(today, deadline_only, false)
            });
        }

        // Future repeats.
        let Some(spec) = t.rec.as_deref().and_then(recurrence::parse_rec_spec) else {
            continue;
        };
        if t.done {
            continue;
        }
        let until = t.until.as_deref().and_then(date);
        let mut left = t.times.as_deref().and_then(|v| v.parse::<u32>().ok());
        let mut cur = main;
        for _ in 0..MAX_PROJECTED {
            if let Some(n) = left.as_mut() {
                if *n <= 1 {
                    break;
                }
                *n -= 1;
            }
            let Some(next) = recurrence::advance(cur, &spec) else {
                break;
            };
            if next > to || until.is_some_and(|u| next > u) {
                break;
            }
            if next >= from {
                out.push(occ(next, deadline_only, true));
            }
            cur = next;
        }
    }
    out.sort_by(|a, b| {
        a.date
            .cmp(&b.date)
            .then(a.start.is_some().cmp(&b.start.is_some()))
            .then(a.start.cmp(&b.start))
            .then(a.abs.cmp(&b.abs))
    });
    out
}

/// Gaps of at least `min` minutes between `day_start` and `day_end` that no
/// timed occurrence of `occs` covers.
pub fn free_slots(occs: &[&Occurrence], day_start: u32, day_end: u32, min: u32) -> Vec<(u32, u32)> {
    let mut busy: Vec<(u32, u32)> = occs
        .iter()
        .filter_map(|o| Some((o.start?, o.end()?)))
        .collect();
    busy.sort();
    let mut out = Vec::new();
    let mut at = day_start;
    for (s, e) in busy {
        if s > at && s - at >= min {
            out.push((at, s.min(day_end)));
        }
        at = at.max(e);
        if at >= day_end {
            break;
        }
    }
    if day_end > at && day_end - at >= min {
        out.push((at, day_end));
    }
    out
}

/// Side-by-side lanes for overlapping timed occurrences: `(lane, lanes)`
/// for each, in the order given.
pub fn lanes(occs: &[&Occurrence]) -> Vec<(usize, usize)> {
    let mut lane_end: Vec<u32> = Vec::new();
    let mut lane_of = vec![0usize; occs.len()];
    // Clusters of mutually overlapping blocks share a lane count.
    let mut cluster_of = vec![0usize; occs.len()];
    let mut cluster_lanes: Vec<usize> = Vec::new();
    let mut cluster_end = 0u32;
    let mut order: Vec<usize> = (0..occs.len()).collect();
    order.sort_by_key(|&i| (occs[i].start, occs[i].end()));
    for i in order {
        let (Some(s), Some(e)) = (occs[i].start, occs[i].end()) else {
            continue;
        };
        if cluster_lanes.is_empty() || s >= cluster_end {
            cluster_lanes.push(0);
            lane_end.clear();
            cluster_end = e;
        }
        cluster_end = cluster_end.max(e);
        let lane = match lane_end.iter().position(|&end| end <= s) {
            Some(l) => {
                lane_end[l] = e;
                l
            }
            None => {
                lane_end.push(e);
                lane_end.len() - 1
            }
        };
        lane_of[i] = lane;
        let c = cluster_lanes.len() - 1;
        cluster_of[i] = c;
        cluster_lanes[c] = cluster_lanes[c].max(lane + 1);
    }
    (0..occs.len())
        .map(|i| {
            (
                lane_of[i],
                cluster_lanes
                    .get(cluster_of[i])
                    .copied()
                    .unwrap_or(1)
                    .max(1),
            )
        })
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::todo::parse_file;

    fn d(s: &str) -> NaiveDate {
        date(s).unwrap()
    }

    #[test]
    fn tasks_land_on_their_days_with_repeats_and_deadlines() {
        let tasks = parse_file(
            "Gym plan:2026-10-05 rec:+1w:mon,wed,fri at:07:00 dur:1h\n\
             Essay plan:2026-10-06 due:2026-10-09\n\
             Ryanair due:2026-10-09\n\
             Old plan:2026-10-01\n\
             x 2026-10-02 2026-10-01 Done plan:2026-10-06\n\
             Physio plan:2026-10-06 rec:+1w at:11:30 times:2\n\
             Stretch rec:+1d\n",
        );
        let occ = occurrences(&tasks, d("2026-10-05"), d("2026-10-11"), d("2026-10-06"));
        let on = |day: &str| -> Vec<(usize, Option<u32>, bool, bool, bool)> {
            occ.iter()
                .filter(|o| o.date == d(day))
                .map(|o| (o.abs, o.start, o.deadline, o.projected, o.late))
                .collect()
        };
        assert_eq!(on("2026-10-05"), [(0, Some(420), false, false, false)]);
        // A dateless repeat starts today, then repeats.
        assert!(
            occ.iter()
                .any(|o| o.abs == 6 && o.date == d("2026-10-06") && !o.projected)
        );
        assert!(
            occ.iter()
                .any(|o| o.abs == 6 && o.date == d("2026-10-07") && o.projected)
        );
        assert!(!occ.iter().any(|o| o.abs == 6 && o.date == d("2026-10-05")));
        assert_eq!(
            on("2026-10-06"),
            [
                // Monday's gym wasn't ticked off: late.
                (0, None, false, false, true),
                (1, None, false, false, false),
                (3, None, false, false, true),
                (4, None, false, false, false),
                (6, None, false, false, false),
                (5, Some(690), false, false, false),
            ]
        );
        assert_eq!(
            on("2026-10-07"),
            [
                (6, None, false, true, false),
                (0, Some(420), false, true, false)
            ]
        );
        assert_eq!(
            on("2026-10-09"),
            [
                (1, None, true, false, false),
                (2, None, true, false, false),
                (6, None, false, true, false),
                (0, Some(420), false, true, false),
            ]
        );
        // times:2 — this one and one more.
        assert_eq!(occ.iter().filter(|o| o.abs == 5).count(), 1);
        let occ = occurrences(&tasks, d("2026-10-01"), d("2026-10-31"), d("2026-10-06"));
        assert_eq!(occ.iter().filter(|o| o.abs == 5).count(), 2);
        // 12 Mondays, Wednesdays and Fridays, plus the late one on today.
        assert_eq!(occ.iter().filter(|o| o.abs == 0).count(), 13);
    }

    #[test]
    fn free_slots_and_lanes() {
        let mk = |s: u32, m: u32| Occurrence {
            abs: 0,
            date: d("2026-10-06"),
            start: Some(s),
            minutes: m,
            deadline: false,
            projected: false,
            late: false,
        };
        let a = mk(9 * 60, 120);
        let b = mk(10 * 60, 60);
        let c = mk(16 * 60, 60);
        let occs = [&a, &b, &c];
        assert_eq!(
            free_slots(&occs, 8 * 60, 20 * 60, 30),
            [(480, 540), (660, 960), (1020, 1200)]
        );
        assert_eq!(lanes(&occs), [(0, 2), (1, 2), (0, 1)]);
        assert_eq!(parse_time("07:05"), Some(425));
        assert_eq!(parse_time("24:00"), None);
    }
}
