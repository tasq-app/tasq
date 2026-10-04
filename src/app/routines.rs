//! Routines: your repeating tasks, with how many times in a row you've done
//! them and the last seven days at a glance. Worked out from the done
//! copies a repeat leaves behind, so a streak grows the moment you tick one.

use chrono::{Datelike, Days, NaiveDate, Weekday};

use super::App;
use crate::recurrence::{self, RecSpec, RecUnit};
use crate::todo::{self, Task};

/// One of the last seven days of a routine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DayMark {
    Done,
    /// It was due that day and wasn't done.
    Missed,
    /// Due today, not done yet.
    Pending,
    /// Not a day it repeats on.
    Off,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Routine {
    pub title: String,
    /// Index of the open occurrence in the task list.
    pub abs: usize,
    /// Done in a row, up to the open occurrence.
    pub streak: usize,
    /// Repeats every day (or every weekday): the streak reads in days.
    pub daily: bool,
    /// The last seven days, oldest first; the last is today.
    pub week: [DayMark; 7],
}

fn date(s: Option<&str>) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s?, "%Y-%m-%d").ok()
}

/// What makes two occurrences the same routine: the title and the rule.
fn key(t: &Task) -> Option<(String, String)> {
    Some((todo::body_only(&t.raw), t.rec.clone()?))
}

fn scheduled(t: &Task) -> Option<NaiveDate> {
    date(t.planned.as_deref()).or_else(|| date(t.due.as_deref()))
}

/// Whether a rule asks for `d` at all.
fn repeats_on(spec: &RecSpec, d: NaiveDate) -> bool {
    let weekend = matches!(d.weekday(), Weekday::Sat | Weekday::Sun);
    match spec.unit {
        RecUnit::Day => spec.n == 1,
        RecUnit::BusinessDay => spec.n == 1 && !weekend,
        RecUnit::Week => spec.n == 1 && spec.days & recurrence::day_bit(d.weekday()) != 0,
        RecUnit::Month | RecUnit::Year => false,
    }
}

/// The routine of the open repeating task `open`, given every done
/// occurrence (`(scheduled, done on)`) of it.
pub fn routine(
    open: &Task,
    abs: usize,
    spec: &RecSpec,
    done: &[(Option<NaiveDate>, NaiveDate)],
    today: NaiveDate,
) -> Routine {
    // Walk back from the open one: each done occurrence whose next date is
    // the one after it extends the streak.
    let mut streak = 0;
    let mut used = vec![false; done.len()];
    let mut next = scheduled(open);
    if next.is_some_and(|n| n < today) {
        // Overdue: the chain is already broken.
        next = None;
    }
    while let Some(n) = next {
        let found = done.iter().enumerate().find(|(i, (sched, on))| {
            !used[*i]
                && (sched.and_then(|s| recurrence::advance(s, spec)) == Some(n)
                    || recurrence::advance(*on, spec) == Some(n))
        });
        let Some((i, (sched, on))) = found else {
            break;
        };
        used[i] = true;
        streak += 1;
        next = Some(sched.unwrap_or(*on));
    }

    // Before its first occurrence a routine wasn't there to miss.
    let start = done
        .iter()
        .map(|(s, on)| s.unwrap_or(*on).min(*on))
        .chain(scheduled(open))
        .min();
    let mut week = [DayMark::Off; 7];
    for (k, mark) in week.iter_mut().enumerate() {
        let Some(d) = today.checked_sub_days(Days::new(6 - k as u64)) else {
            continue;
        };
        *mark = if done.iter().any(|(_, on)| *on == d) {
            DayMark::Done
        } else if start.is_some_and(|s| d >= s)
            && (repeats_on(spec, d) || scheduled(open) == Some(d))
        {
            if d == today {
                DayMark::Pending
            } else {
                DayMark::Missed
            }
        } else {
            DayMark::Off
        };
    }
    let daily = matches!(spec.unit, RecUnit::Day | RecUnit::BusinessDay) && spec.n == 1;
    Routine {
        title: todo::body_only(&open.raw),
        abs,
        streak,
        daily,
        week,
    }
}

impl App {
    /// Every open repeating task as a routine, longest streak first.
    pub fn routines(&self) -> Vec<Routine> {
        let Some(today) = date(Some(self.store.today())) else {
            return Vec::new();
        };
        let all_done: Vec<&Task> = self
            .store
            .tasks()
            .iter()
            .chain(self.store.archive().tasks())
            .filter(|t| t.done)
            .collect();
        let mut out: Vec<Routine> = self
            .store
            .tasks()
            .iter()
            .enumerate()
            .filter(|(_, t)| !t.done)
            .filter_map(|(abs, t)| {
                let spec = recurrence::parse_rec_spec(t.rec.as_deref()?)?;
                let k = key(t)?;
                let done: Vec<(Option<NaiveDate>, NaiveDate)> = all_done
                    .iter()
                    .filter(|d| key(d).as_ref() == Some(&k))
                    .filter_map(|d| Some((scheduled(d), date(d.done_date.as_deref())?)))
                    .collect();
                Some(routine(t, abs, &spec, &done, today))
            })
            .collect();
        out.sort_by(|a, b| b.streak.cmp(&a.streak).then(a.title.cmp(&b.title)));
        out
    }

    /// The streak of the open task at `abs`, if it is a routine.
    pub fn streak_of(&self, abs: usize) -> Option<usize> {
        self.routines()
            .into_iter()
            .find(|r| r.abs == abs)
            .map(|r| r.streak)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::app::test_support::build_app;

    #[test]
    fn a_streak_counts_the_days_in_a_row() {
        // Today is 2026-05-06 in tests.
        let mut app = build_app(concat!(
            "x 2026-05-03 Gym plan:2026-05-03 rec:1d\n",
            "x 2026-05-04 Gym plan:2026-05-04 rec:1d\n",
            "x 2026-05-05 Gym plan:2026-05-05 rec:1d\n",
            "Gym plan:2026-05-06 rec:1d\n",
        ));
        let r = &app.routines()[0];
        assert_eq!(r.title, "Gym");
        assert_eq!(r.streak, 3);
        assert!(r.daily);
        use DayMark::*;
        assert_eq!(r.week, [Off, Off, Off, Done, Done, Done, Pending]);
        // Ticking today's makes it four, at once.
        app.toggle_complete(3);
        let r = &app.routines()[0];
        assert_eq!(r.streak, 4);
        assert_eq!(r.week[6], Done);
    }

    #[test]
    fn a_missed_day_breaks_the_streak() {
        let app = build_app(concat!(
            "x 2026-05-02 Gym plan:2026-05-02 rec:1d\n",
            "x 2026-05-04 Gym plan:2026-05-04 rec:1d\n",
            "Gym plan:2026-05-05 rec:1d\n",
        ));
        let r = &app.routines()[0];
        assert_eq!(r.streak, 0, "yesterday's is still open");
        assert_eq!(r.week[5], DayMark::Missed);
    }
}
