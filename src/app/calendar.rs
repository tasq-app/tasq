//! The calendar screen: day, week and month views over the same tasks.
//!
//! It replaces the list while open (`App::calendar` is `Some`), but dialogs
//! — adding, editing — open on top of it and close back into it.

use chrono::{Datelike, Days, NaiveDate};

use super::App;
use super::types::Mode;
use crate::core::calendar::{self, Occurrence};
use crate::core::spaces;

/// Which calendar view is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalView {
    Day,
    Week,
    Month,
}

impl CalView {
    pub fn label(self) -> &'static str {
        match self {
            CalView::Day => "DAY",
            CalView::Week => "WEEK",
            CalView::Month => "MONTH",
        }
    }
}

/// The two looks of the week and month views (`v` switches).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CalStyle {
    /// Week: time blocks in seven columns. Month: counts, the day below.
    #[default]
    Blocks,
    /// Week: an agenda by day. Month: titles in each day.
    List,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalScreen {
    pub view: CalView,
    /// The selected day.
    pub date: NaiveDate,
    /// Index of the selected occurrence within the selected day.
    pub selected: usize,
    pub week_style: CalStyle,
    pub month_style: CalStyle,
}

/// A change made to one occurrence in the calendar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeriesOp {
    /// Open the edit dialog (`true`: in Insert mode).
    Edit(bool),
    Delete,
    /// Days later (negative: earlier).
    ShiftDay(i64),
    /// Minutes later (negative: earlier).
    ShiftTime(i32),
    /// Dropped on a day (and a time, for a timed block).
    MoveTo {
        date: NaiveDate,
        start: Option<u32>,
    },
    /// A new length, in minutes.
    Resize(u32),
}

/// A change to a repeating task waiting for "only this one, or this and
/// the ones after?".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeriesAsk {
    pub occ: Occurrence,
    pub op: SeriesOp,
}

/// Monday of `d`'s week.
pub fn week_start(d: NaiveDate) -> NaiveDate {
    d - Days::new(u64::from(d.weekday().num_days_from_monday()))
}

/// First and last day of `d`'s month.
pub fn month_bounds(d: NaiveDate) -> (NaiveDate, NaiveDate) {
    let first = d.with_day(1).unwrap_or(d);
    let next = first
        .checked_add_months(chrono::Months::new(1))
        .unwrap_or(first);
    (first, next.pred_opt().unwrap_or(first))
}

fn shift_date(raw: &str, key: &str, days: i64) -> Option<String> {
    let mut changed = false;
    let out: Vec<String> = raw
        .split_whitespace()
        .map(|tok| {
            if !changed
                && let Some(v) = tok.strip_prefix(key).and_then(|r| r.strip_prefix(':'))
                && let Ok(d) = NaiveDate::parse_from_str(v, "%Y-%m-%d")
                && let Some(n) = d.checked_add_signed(chrono::Duration::days(days))
            {
                changed = true;
                return format!("{key}:{}", n.format("%Y-%m-%d"));
            }
            tok.to_string()
        })
        .collect();
    changed.then(|| out.join(" "))
}

/// `raw` with its `at:` set to `minutes` after midnight (added if missing).
fn set_time(raw: &str, minutes: u32) -> String {
    let at = format!("at:{:02}:{:02}", minutes / 60, minutes % 60);
    let mut found = false;
    let mut out: Vec<String> = raw
        .split_whitespace()
        .map(|tok| {
            if !found && tok.starts_with("at:") {
                found = true;
                return at.clone();
            }
            tok.to_string()
        })
        .collect();
    if !found {
        out.push(at);
    }
    out.join(" ")
}

impl App {
    /// Minutes after midnight now, for the day view's now line (`None`
    /// when frozen to nothing in tests).
    pub fn now_minutes(&self) -> Option<u32> {
        if let Some(m) = self.frozen_now {
            return (m < 24 * 60).then_some(m);
        }
        use chrono::Timelike;
        let now = chrono::Local::now();
        Some(now.hour() * 60 + now.minute())
    }

    pub fn today_naive(&self) -> NaiveDate {
        NaiveDate::parse_from_str(self.store.today(), "%Y-%m-%d").unwrap_or_default()
    }

    /// The task an action is about: the one selected in the calendar when
    /// it's open, else the list's.
    pub fn calendar_or_list_abs(&self) -> Option<usize> {
        if self.calendar.is_some() {
            return self.cal_selected().map(|o| o.abs);
        }
        self.cur_abs()
    }

    /// Open the calendar on `view`, at today (or where it was).
    pub fn open_cal(&mut self, view: CalView) {
        self.home = false;
        self.notes_screen = None;
        self.trash_screen = None;
        let today = self.today_naive();
        let state = match self.calendar.take() {
            Some(mut s) => {
                s.view = view;
                s
            }
            None => CalScreen {
                view,
                date: today,
                selected: 0,
                week_style: CalStyle::default(),
                month_style: CalStyle::default(),
            },
        };
        self.calendar = Some(state);
        self.mode = Mode::Normal;
        self.cal_clamp();
    }

    pub fn close_calendar(&mut self) {
        self.calendar = None;
    }

    /// Occurrences from `from` to `to`, with the space filter and hidden
    /// spaces applied like the list.
    pub fn cal_occurrences(&self, from: NaiveDate, to: NaiveDate) -> Vec<Occurrence> {
        let tasks = self.store.tasks();
        let known = self.store.known_spaces();
        let open = self.filter.project.as_deref();
        calendar::occurrences(tasks, from, to, self.today_naive())
            .into_iter()
            .filter(|o| {
                let t = &tasks[o.abs];
                open.is_none_or(|p| spaces::in_space(&t.projects, p))
                    && !spaces::hidden_from_view(&t.projects, known, open)
            })
            .collect()
    }

    /// The selected day's occurrences: all-day first, then by time.
    pub fn cal_day_items(&self) -> Vec<Occurrence> {
        let Some(c) = &self.calendar else {
            return Vec::new();
        };
        self.cal_occurrences(c.date, c.date)
    }

    pub fn cal_selected(&self) -> Option<Occurrence> {
        let c = self.calendar.as_ref()?;
        self.cal_day_items().into_iter().nth(c.selected)
    }

    fn cal_clamp(&mut self) {
        let n = self.cal_day_items().len();
        if let Some(c) = self.calendar.as_mut() {
            c.selected = c.selected.min(n.saturating_sub(1));
        }
    }

    /// Move the selected day by `days`.
    pub fn cal_move(&mut self, days: i64) {
        if let Some(c) = self.calendar.as_mut()
            && let Some(d) = c.date.checked_add_signed(chrono::Duration::days(days))
        {
            c.date = d;
            c.selected = 0;
        }
        self.cal_clamp();
    }

    /// Back or forward one step of the view: a day, a week or a month.
    pub fn cal_page(&mut self, forward: bool) {
        let Some(c) = self.calendar.as_mut() else {
            return;
        };
        let d = c.date;
        c.date = match (c.view, forward) {
            (CalView::Month, true) => d.checked_add_months(chrono::Months::new(1)),
            (CalView::Month, false) => d.checked_sub_months(chrono::Months::new(1)),
            (CalView::Week, true) => d.checked_add_days(Days::new(7)),
            (CalView::Week, false) => d.checked_sub_days(Days::new(7)),
            (CalView::Day, true) => d.succ_opt(),
            (CalView::Day, false) => d.pred_opt(),
        }
        .unwrap_or(d);
        c.selected = 0;
        self.cal_clamp();
    }

    pub fn cal_today(&mut self) {
        let today = self.today_naive();
        if let Some(c) = self.calendar.as_mut() {
            c.date = today;
            c.selected = 0;
        }
        self.cal_clamp();
    }

    /// Select the next (or previous) occurrence of the selected day.
    pub fn cal_select(&mut self, forward: bool) {
        let n = self.cal_day_items().len();
        if let Some(c) = self.calendar.as_mut()
            && n > 0
        {
            c.selected = if forward {
                (c.selected + 1).min(n - 1)
            } else {
                c.selected.saturating_sub(1)
            };
        }
    }

    /// `v`: switch the week or month view between its two looks.
    pub fn cal_toggle_style(&mut self) {
        if let Some(c) = self.calendar.as_mut() {
            let flip = |s: CalStyle| match s {
                CalStyle::Blocks => CalStyle::List,
                CalStyle::List => CalStyle::Blocks,
            };
            match c.view {
                CalView::Week => c.week_style = flip(c.week_style),
                CalView::Month => c.month_style = flip(c.month_style),
                CalView::Day => {}
            }
        }
    }

    /// Enter: edit the selected task (a repeat asks which ones); in the
    /// month, open the selected day.
    pub fn cal_edit(&mut self) {
        if self
            .calendar
            .as_ref()
            .is_some_and(|c| c.view == CalView::Month)
        {
            self.open_cal(CalView::Day);
            return;
        }
        if let Some(occ) = self.cal_selected() {
            self.cal_apply(occ, SeriesOp::Edit(false));
        }
    }

    /// `i`: edit the selected task in Insert mode (a repeat asks which).
    pub fn cal_edit_insert(&mut self) {
        if let Some(occ) = self.cal_selected() {
            self.cal_apply(occ, SeriesOp::Edit(true));
        }
    }

    /// `D` / Delete: delete the selected task (a repeat asks which ones).
    pub fn cal_delete(&mut self) {
        if let Some(occ) = self.cal_selected() {
            self.cal_apply(occ, SeriesOp::Delete);
        }
    }

    /// Make `op` on the occurrence `occ`: straight away for a task that
    /// doesn't repeat; a repeating one first asks whether it's only this
    /// one or this and the ones after.
    pub fn cal_apply(&mut self, occ: Occurrence, op: SeriesOp) {
        let Some(t) = self.store.tasks().get(occ.abs) else {
            return;
        };
        if t.rec.is_some() && !t.done {
            self.series_ask = Some(SeriesAsk { occ, op });
            return;
        }
        self.cal_do(occ.abs, &occ, op);
    }

    /// The answer to [`SeriesAsk`]: `only_this` takes the occurrence out of
    /// its series (a task of its own), else the series is cut there and the
    /// change made to the part from it on.
    pub fn series_answer(&mut self, only_this: bool) {
        use crate::core::series;
        let Some(ask) = self.series_ask.take() else {
            return;
        };
        let occ = ask.occ;
        let Some(t) = self.store.tasks().get(occ.abs).cloned() else {
            return;
        };
        let (on, current) = (occ.origin, !occ.projected);
        if ask.op == SeriesOp::Delete {
            let edit = |s: &mut Self, raw: String| match s.store.edit_line(occ.abs, &raw) {
                crate::core::EditOutcome::Saved { abs } => s.after_mutation(abs),
                crate::core::EditOutcome::Aborted(r) => s.handle_reconcile_abort(r),
                crate::core::EditOutcome::Error(e) => s.flash(format!("couldn't save: {e}")),
                _ => {}
            };
            match (only_this, current) {
                (true, true) => match series::advance_one(&t) {
                    Some(raw) => edit(self, raw),
                    None => self.delete(occ.abs),
                },
                (true, false) => edit(self, series::add_skip(&t.raw, on)),
                (false, true) => self.delete(occ.abs),
                (false, false) => edit(self, series::split_at(&t, on).0),
            }
            if !(only_this && current) || series::advance_one(&t).is_some() {
                self.flash(if only_this {
                    "that one's gone · the rest stay"
                } else {
                    "gone from here on"
                });
            }
            self.recompute_visible();
            self.cal_clamp();
            return;
        }
        let target = match (only_this, current) {
            (false, true) => Some(occ.abs),
            (true, true) => match series::advance_one(&t) {
                Some(rest) => self.split_out(occ.abs, &rest, &series::one_off(&t, on)),
                None => {
                    self.cal_rewrite(occ.abs, series::one_off(&t, on));
                    Some(occ.abs)
                }
            },
            (true, false) => self.split_out(
                occ.abs,
                &series::add_skip(&t.raw, on),
                &series::one_off(&t, on),
            ),
            (false, false) => {
                let (old, new) = series::split_at(&t, on);
                self.split_out(occ.abs, &old, &new)
            }
        };
        if let Some(target) = target {
            // The new task stands where the occurrence was.
            let occ = Occurrence {
                abs: target,
                projected: false,
                ..occ
            };
            self.cal_do(target, &occ, ask.op);
        }
    }

    /// Rewrite task `abs` as `keep` and add `add` after it; the added
    /// task's index.
    fn split_out(&mut self, abs: usize, keep: &str, add: &str) -> Option<usize> {
        use crate::core::EditOutcome;
        match self.store.rewrite_and_add(abs, keep, add) {
            EditOutcome::Saved { abs } => {
                self.recompute_visible();
                Some(abs)
            }
            EditOutcome::Aborted(r) => {
                self.handle_reconcile_abort(r);
                None
            }
            EditOutcome::Error(e) => {
                self.flash(format!("couldn't save: {e}"));
                None
            }
            _ => None,
        }
    }

    /// Make `op` on task `abs`, whose occurrence `occ` it's about.
    fn cal_do(&mut self, abs: usize, occ: &Occurrence, op: SeriesOp) {
        let Some(raw) = self.task_raw(abs) else {
            return;
        };
        // Moving a day: the deadline for a deadline, else the plan (and the
        // end, for something lasting several days).
        let moved = |raw: &str, days: i64| -> String {
            if occ.deadline {
                shift_date(raw, "due", days).unwrap_or_else(|| raw.to_string())
            } else {
                let r = shift_date(raw, "plan", days).unwrap_or_else(|| raw.to_string());
                shift_date(&r, crate::core::series::END_KEY, days).unwrap_or(r)
            }
        };
        match op {
            SeriesOp::Edit(insert) => {
                self.begin_live_edit(abs, insert);
                self.mode = Mode::Insert;
            }
            SeriesOp::Delete => {
                self.delete(abs);
                self.cal_clamp();
            }
            SeriesOp::ShiftTime(minutes) => {
                let start = match occ.start {
                    Some(s) => (s as i32 + minutes).clamp(0, 24 * 60 - 30) as u32,
                    None => 9 * 60,
                };
                self.cal_rewrite(abs, set_time(&raw, start));
            }
            SeriesOp::ShiftDay(days) => {
                self.cal_rewrite(abs, moved(&raw, days));
                self.cal_move(days);
                self.cal_select_task(abs);
            }
            SeriesOp::MoveTo { date, start } => {
                let days = (date - occ.date).num_days();
                let mut new = moved(&raw, days);
                if let Some(s) = start {
                    new = set_time(&new, s);
                }
                self.cal_rewrite(abs, new);
                if let Some(c) = self.calendar.as_mut() {
                    c.date = date;
                }
                self.cal_select_task(abs);
            }
            SeriesOp::Resize(minutes) => {
                let dur = crate::duration::format_minutes(minutes.max(15));
                self.cal_rewrite(
                    abs,
                    crate::core::series::set_kv(&raw, crate::todo::DURATION_KEY, Some(&dur)),
                );
            }
        }
    }

    /// Select task `abs` on the selected day, if it's there.
    fn cal_select_task(&mut self, abs: usize) {
        let items = self.cal_day_items();
        if let Some(i) = items.iter().position(|o| o.abs == abs)
            && let Some(c) = self.calendar.as_mut()
        {
            c.selected = i;
        }
    }

    /// `r` in the calendar: reschedule the selected task, not the one
    /// under the list's cursor.
    pub fn cal_reschedule(&mut self) {
        if let Some(occ) = self.cal_selected() {
            self.reschedule(occ.abs);
        }
    }

    /// Reschedule task `abs`: its line in the edit dialog with the
    /// calendar open on its planned date (or its deadline, for a task that
    /// only has one).
    pub fn reschedule(&mut self, abs: usize) {
        use super::draft_overlay::CalendarTarget;
        let Some(raw) = self.task_raw(abs) else {
            return;
        };
        let target = match self.tasks().get(abs) {
            Some(t) if t.planned.is_none() && t.due.is_some() => CalendarTarget::Due,
            _ => CalendarTarget::Planned,
        };
        self.selection.enter_edit(abs);
        self.draft_set_insert(raw);
        self.mode = Mode::Insert;
        self.open_calendar(target);
    }

    /// `x`: complete the selected task (the current one of a repeat).
    pub fn cal_complete(&mut self) {
        let Some(occ) = self.cal_selected() else {
            return;
        };
        if occ.projected {
            self.flash("a future repeat · tick off the current one");
            return;
        }
        self.toggle_complete(occ.abs);
        self.cal_clamp();
    }

    /// `n`: a new task planned on the selected day.
    pub fn cal_new(&mut self) {
        let Some(c) = &self.calendar else {
            return;
        };
        let seed = format!(
            "plan:{} {}",
            c.date.format("%Y-%m-%d"),
            self.filter().tag_seed()
        );
        self.mode = Mode::Insert;
        self.draft_set_insert(seed);
        self.selection.exit_edit();
    }

    /// `J` / `K`: move the selected block later or earlier by `minutes`; an
    /// all-day task gets a time, 09:00.
    pub fn cal_shift_time(&mut self, minutes: i32) {
        if let Some(occ) = self.cal_selected() {
            self.cal_apply(occ, SeriesOp::ShiftTime(minutes));
        }
    }

    /// `H` / `L`: move the selected task to the day before or after.
    pub fn cal_shift_day(&mut self, days: i64) {
        if let Some(occ) = self.cal_selected() {
            self.cal_apply(occ, SeriesOp::ShiftDay(days));
        }
    }

    fn cal_rewrite(&mut self, abs: usize, raw: String) {
        use crate::core::EditOutcome;
        match self.store.edit_line(abs, &raw) {
            EditOutcome::Saved { .. } => self.recompute_visible(),
            EditOutcome::Aborted(r) => self.handle_reconcile_abort(r),
            EditOutcome::Error(e) => self.flash(format!("move failed: {e}")),
            _ => {}
        }
        // Keep the same task selected after a time change re-sorts the day.
        let items = self.cal_day_items();
        if let Some(i) = items.iter().position(|o| o.abs == abs)
            && let Some(c) = self.calendar.as_mut()
        {
            c.selected = i;
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::app::test_support::build_app;

    #[test]
    fn r_in_the_calendar_reschedules_the_task_selected_there() {
        let mut app = build_app(
            "Other task\n\
             Teoria AII plan:2026-05-06 at:09:00 dur:2h\n",
        );
        app.cursor = 0; // the list is on "Other task"
        app.open_cal(CalView::Day);
        assert_eq!(app.cal_selected().unwrap().abs, 1);
        app.cal_reschedule();
        assert_eq!(app.selection.editing(), Some(1));
        assert!(
            app.draft.text().contains("Teoria AII"),
            "{}",
            app.draft.text()
        );
    }

    #[test]
    fn day_view_selects_moves_and_reschedules() {
        let mut app = build_app(
            "Teoria AII plan:2026-05-06 at:09:00 dur:2h\n\
             Trabajo TIS plan:2026-05-06 at:16:00 dur:2h\n\
             Gym plan:2026-05-06 rec:+1d at:07:00\n",
        );
        app.open_cal(CalView::Day);
        let titles =
            |app: &App| -> Vec<usize> { app.cal_day_items().iter().map(|o| o.abs).collect() };
        assert_eq!(titles(&app), [2, 0, 1]);
        app.cal_select(true);
        app.cal_shift_time(30);
        assert!(
            app.tasks()[0].raw.contains("at:09:30"),
            "{}",
            app.tasks()[0].raw
        );
        assert_eq!(app.cal_selected().unwrap().abs, 0);
        app.cal_shift_day(1);
        assert!(app.tasks()[0].raw.contains("plan:2026-05-07"));
        assert_eq!(
            app.calendar.as_ref().unwrap().date.to_string(),
            "2026-05-07"
        );
        // Tomorrow's gym is a future repeat: moving it asks first.
        app.cal_select(false);
        assert!(app.cal_selected().unwrap().projected);
        let before = app.tasks()[2].raw.clone();
        app.cal_shift_time(30);
        assert!(app.series_ask.is_some());
        assert_eq!(app.tasks()[2].raw, before);
        app.series_ask = None;
        app.cal_page(false);
        app.cal_today();
        assert_eq!(
            app.calendar.as_ref().unwrap().date.to_string(),
            "2026-05-06"
        );
    }

    #[test]
    fn a_repeat_changes_only_this_one_or_from_here_on() {
        // Today is 2026-05-06, a wednesday.
        let mut app = build_app("Class plan:2026-05-06 at:09:00 rec:+1w event:1\n");
        app.open_cal(CalView::Day);
        // Next week's class, moved an hour later on its own.
        app.cal_move(7);
        app.cal_shift_time(60);
        app.series_answer(true);
        let raws: Vec<String> = app.tasks().iter().map(|t| t.raw.clone()).collect();
        assert_eq!(
            raws,
            [
                "Class plan:2026-05-06 at:09:00 rec:+1w event:1 skip:2026-05-13",
                "Class plan:2026-05-13 at:10:00 event:1",
            ]
        );
        assert_eq!(app.cal_selected().unwrap().abs, 1);

        // The week after, deleted from there on: the series ends before it.
        app.cal_move(7);
        let occ = app
            .cal_day_items()
            .into_iter()
            .find(|o| o.abs == 0)
            .unwrap();
        app.cal_apply(occ, SeriesOp::Delete);
        app.series_answer(false);
        assert!(
            app.tasks()[0].raw.contains("until:2026-05-19"),
            "{}",
            app.tasks()[0].raw
        );
        assert!(app.cal_day_items().is_empty());

        // Today's, deleted on its own: the series moves on to the next.
        app.cal_today();
        let occ = app.cal_day_items()[0].clone();
        app.cal_apply(occ, SeriesOp::Delete);
        app.series_answer(true);
        // Its next date is 2026-05-13 (skipped), then 05-20 (past until).
        assert!(
            app.tasks()
                .iter()
                .all(|t| !t.raw.contains("plan:2026-05-06"))
        );
    }

    #[test]
    fn enter_on_a_month_day_opens_that_day() {
        let mut app = build_app("a plan:2026-05-20\n");
        app.open_cal(CalView::Month);
        app.cal_move(14);
        app.cal_edit();
        let c = app.calendar.as_ref().unwrap();
        assert_eq!(
            (c.view, c.date.to_string()),
            (CalView::Day, "2026-05-20".into())
        );
        assert_eq!(app.cal_selected().unwrap().abs, 0);
    }

    #[test]
    fn weeks_and_months_have_their_bounds() {
        let d = NaiveDate::from_ymd_opt(2026, 10, 8).unwrap();
        assert_eq!(week_start(d).to_string(), "2026-10-05");
        let (a, b) = month_bounds(d);
        assert_eq!(
            (a.to_string(), b.to_string()),
            ("2026-10-01".into(), "2026-10-31".into())
        );
        assert_eq!(set_time("a at:07:00 b", 450), "a at:07:30 b");
        assert_eq!(set_time("a", 540), "a at:09:00");
        assert_eq!(
            shift_date("x plan:2026-10-31", "plan", 1).as_deref(),
            Some("x plan:2026-11-01")
        );
    }
}
