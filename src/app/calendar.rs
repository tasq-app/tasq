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
    /// The month grid's first row (a Monday) once the arrows have scrolled
    /// it a week at a time; `None`: the selected day's month, whole.
    pub month_top: Option<NaiveDate>,
    /// How many weeks the scrolled grid shows (kept from the month it
    /// started at, so the grid doesn't change height as it scrolls).
    pub month_rows: u16,
    /// The day and week grids' first hour once scrolled (`Ctrl-e`/`Ctrl-y`,
    /// `Ctrl-d`/`Ctrl-u`, the wheel); `None`: [`default_top`], moved just
    /// enough to show the selected block.
    pub hour_top: Option<u32>,
}

/// Rows an hour gets in the day and week grids: the same for every hour,
/// however many hours the day has planned.
pub const ROWS_PER_HOUR: u16 = 2;
/// The grids' first hour when nothing earlier is planned.
pub const DAY_FROM: u32 = 7;

/// Whole hours a grid `height` rows tall shows.
pub fn grid_hours(height: u16) -> u32 {
    (u32::from(height) / u32::from(ROWS_PER_HOUR)).max(1)
}

/// `top` kept so a grid of `visible` hours doesn't run past midnight.
pub fn clamp_top(top: u32, visible: u32) -> u32 {
    top.min(24u32.saturating_sub(visible))
}

/// The first hour a grid starts at unscrolled: [`DAY_FROM`], or the hour
/// of anything that starts earlier.
pub fn default_top(starts: impl Iterator<Item = u32>) -> u32 {
    starts
        .map(|s| s / 60)
        .min()
        .map_or(DAY_FROM, |h| h.min(DAY_FROM))
}

/// The first hour that shows minutes `start..end` in a grid of `visible`
/// hours, moving from `top` as little as possible. A block longer than the
/// grid shows its start.
pub fn reveal(top: u32, visible: u32, start: u32, end: u32) -> u32 {
    let end = end.max(start + 1);
    let t = if start < top * 60 {
        start / 60
    } else if end > (top + visible) * 60 {
        end.div_ceil(60).saturating_sub(visible).min(start / 60)
    } else {
        top
    };
    clamp_top(t, visible)
}

impl CalScreen {
    /// Whether the view is a grid of hours (the day, the week's blocks).
    pub fn has_hour_grid(&self) -> bool {
        match self.view {
            CalView::Day => true,
            CalView::Week => self.week_style == CalStyle::Blocks,
            CalView::Month => false,
        }
    }

    /// The month grid: its first Monday and how many weeks. Scrolled by
    /// the arrows while the selected day stays in it; else the selected
    /// day's month.
    pub fn month_window(&self) -> (NaiveDate, u16) {
        if let Some(top) = self.month_top {
            let end = top + Days::new(u64::from(self.month_rows) * 7);
            if top <= self.date && self.date < end {
                return (top, self.month_rows);
            }
        }
        let (first, last) = month_bounds(self.date);
        let top = week_start(first);
        (top, ((last - top).num_days() / 7 + 1) as u16)
    }
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
                month_top: None,
                month_rows: 0,
                hour_top: None,
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
        self.cal_view_occurrences(c.date, c.date)
    }

    /// What's selected on the day: a task's occurrence, or another
    /// calendar's event.
    pub fn cal_selected_any(&self) -> Option<Occurrence> {
        let c = self.calendar.as_ref()?;
        self.cal_day_items().into_iter().nth(c.selected)
    }

    /// The selected task's occurrence: what every task action is about.
    /// Another calendar's event is read-only, so none of them touch it.
    pub fn cal_selected(&self) -> Option<Occurrence> {
        self.cal_selected_any().filter(|o| o.ext.is_none())
    }

    /// Enter on another calendar's event: its details. Whether one was.
    pub fn cal_open_ext(&mut self) -> bool {
        let Some(x) = self.cal_selected_any().and_then(|o| o.ext) else {
            return false;
        };
        let found =
            self.ext_state.events.iter().find(|e| {
                e.account == x.account && e.calendar == x.calendar_id && e.event.id == x.id
            });
        if let Some(e) = found {
            self.ext_detail = Some(crate::app::ExtDetail {
                event: e.clone(),
                calendar: x.calendar.clone(),
            });
        }
        true
    }

    /// A task action on another calendar's event: say why nothing happens.
    fn cal_read_only(&mut self) -> bool {
        let ext = self.cal_selected_any().is_some_and(|o| o.ext.is_some());
        if ext {
            self.flash("from another calendar · read-only in tasq");
        }
        ext
    }

    fn cal_clamp(&mut self) {
        let n = self.cal_day_items().len();
        if let Some(c) = self.calendar.as_mut() {
            c.selected = c.selected.min(n.saturating_sub(1));
        }
        self.cal_reveal();
    }

    /// Move the selected day by `days`. In the month, going past the
    /// grid's top or bottom row scrolls it a week, not a month.
    pub fn cal_move(&mut self, days: i64) {
        if let Some(c) = self.calendar.as_mut()
            && let Some(d) = c.date.checked_add_signed(chrono::Duration::days(days))
        {
            let (mut top, rows) = c.month_window();
            let week = week_start(d);
            let last_row = top + Days::new(u64::from(rows.saturating_sub(1)) * 7);
            if week < top {
                top = week;
            } else if week > last_row {
                top = week - Days::new(u64::from(rows.saturating_sub(1)) * 7);
            }
            c.date = d;
            c.selected = 0;
            if c.view == CalView::Month {
                c.month_top = Some(top);
                c.month_rows = rows;
            }
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
        // A whole month again.
        c.month_top = None;
        self.cal_clamp();
    }

    pub fn cal_today(&mut self) {
        let today = self.today_naive();
        if let Some(c) = self.calendar.as_mut() {
            c.date = today;
            c.selected = 0;
            c.month_top = None;
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
        self.cal_reveal();
    }

    /// The hour grid as last drawn: its first hour and how many it shows.
    fn cal_grid(&self) -> Option<(u32, u32)> {
        let cols = self.cal_cols.borrow();
        let c = cols.first()?;
        Some((c.top / 60, grid_hours(c.h)))
    }

    /// Scroll the hour grid `hours` later (or earlier, negative), the
    /// selection staying where it is. Nothing outside an hour grid.
    pub fn cal_scroll(&mut self, hours: i32) {
        let Some((top, visible)) = self.cal_grid() else {
            return;
        };
        if let Some(c) = self.calendar.as_mut()
            && c.has_hour_grid()
        {
            let t = (i64::from(top) + i64::from(hours)).max(0) as u32;
            c.hour_top = Some(clamp_top(t, visible));
        }
    }

    /// `Ctrl-d` / `Ctrl-u`: half the grid's hours later or earlier.
    pub fn cal_scroll_half(&mut self, down: bool) {
        let half = self.cal_grid().map_or(1, |(_, v)| (v / 2).max(1)) as i32;
        self.cal_scroll(if down { half } else { -half });
    }

    /// After the selection moved: scroll a scrolled grid just enough to
    /// show it. (Unscrolled, the drawing already follows the selection.)
    fn cal_reveal(&mut self) {
        let sel = self.cal_selected_any();
        let grid = self.cal_grid();
        if let Some(c) = self.calendar.as_mut()
            && let Some(top) = c.hour_top
            && let Some(o) = sel
            && let (Some(s), Some(e)) = (o.start, o.end())
        {
            let visible = grid.map_or(24, |(_, v)| v);
            c.hour_top = Some(reveal(top, visible, s, e));
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
        if self.cal_open_ext() {
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
        if self.cal_read_only() {
            return;
        }
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
        if self.cal_read_only() {
            return;
        }
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
        self.cal_reveal();
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::app::test_support::build_app;

    #[test]
    fn the_hour_window_fits_every_height_and_moves_as_little_as_it_can() {
        // Every height shows whole hours, two rows each: 33 rows used to
        // round 15 hours down to 14 and scroll a grid that fit.
        assert_eq!(grid_hours(33), 16);
        assert_eq!(grid_hours(31), 15);
        assert_eq!(grid_hours(1), 1);
        assert_eq!(clamp_top(20, 10), 14);
        assert_eq!(default_top([8 * 60 + 30, 15 * 60].into_iter()), DAY_FROM);
        assert_eq!(default_top([6 * 60].into_iter()), 6);
        assert_eq!(default_top(std::iter::empty()), DAY_FROM);
        // Already in view: no move.
        assert_eq!(reveal(7, 15, 15 * 60 + 30, 17 * 60 + 20), 7);
        // Below: just enough to show its end.
        assert_eq!(reveal(7, 10, 23 * 60, 24 * 60), 14);
        assert_eq!(reveal(7, 10, 17 * 60 + 30, 18 * 60 + 30), 9);
        // Above: its start at the top.
        assert_eq!(reveal(7, 10, 30, 90), 0);
        // Longer than the grid: its start shows.
        assert_eq!(reveal(0, 2, 10 * 60, 15 * 60), 10);
    }

    fn draw(app: &App) -> String {
        use ratatui::{Terminal, backend::TestBackend};
        // Short: about ten hours of grid.
        let mut term = Terminal::new(TestBackend::new(140, 28)).unwrap();
        term.draw(|f| crate::ui::draw(f, app)).unwrap();
        let buf = term.backend().buffer();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn a_task_at_midnight_scrolls_into_view_instead_of_shrinking_the_hours() {
        // Today is 2026-05-06, a wednesday.
        let mut app = build_app(
            "Class plan:2026-05-06 at:09:00 dur:1h\n\
             Late plan:2026-05-06 at:23:00 dur:1h\n",
        );
        app.frozen_now = Some(24 * 60);
        for view in [CalView::Week, CalView::Day] {
            app.open_cal(view);
            app.calendar.as_mut().unwrap().hour_top = None;
            app.calendar.as_mut().unwrap().selected = 0;
            let text = draw(&app);
            // Two rows an hour, from 07; the late one is counted below.
            assert!(text.contains("07"), "{view:?}\n{text}");
            assert!(text.contains("↓1"), "{view:?}\n{text}");
            assert!(!text.contains("Late"), "{view:?}\n{text}");
            // Ctrl-d twice reaches it; the selection doesn't move.
            app.cal_scroll_half(true);
            draw(&app);
            app.cal_scroll_half(true);
            let text = draw(&app);
            assert!(text.contains("Late"), "{view:?}\n{text}");
            assert!(text.contains("↑1"), "{view:?}\n{text}");
            assert_eq!(app.cal_selected().unwrap().abs, 0);
            // Selecting the class again brings it back into view.
            app.cal_select(true);
            app.cal_select(false);
            let text = draw(&app);
            assert!(text.contains("Class"), "{view:?}\n{text}");
            // Ctrl-y one hour; never above midnight.
            app.cal_scroll(-30);
            draw(&app);
            assert_eq!(app.calendar.as_ref().unwrap().hour_top, Some(0));
        }
    }

    #[test]
    fn the_week_starts_at_the_same_hour_whichever_day_is_selected() {
        // The bug: 07 with an 08:30 block selected, 08 with a 15:30 one.
        let mut app = build_app(
            "PGPI plan:2026-05-06 at:08:30 dur:110m\n\
             Examen plan:2026-05-07 at:15:30 dur:110m\n",
        );
        app.open_cal(CalView::Week);
        let first_hour = |app: &App| {
            draw(app);
            app.cal_cols.borrow()[0].top / 60
        };
        let wed = first_hour(&app);
        app.cal_move(1);
        assert_eq!(app.cal_selected().unwrap().abs, 1);
        assert_eq!(first_hour(&app), wed);
        assert_eq!(wed, DAY_FROM);
    }

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
    fn arrows_scroll_the_month_a_week_at_a_time() {
        // Today is 2026-05-06; May's grid is the five weeks from Apr 27.
        let mut app = build_app("a plan:2026-05-20\n");
        app.open_cal(CalView::Month);
        let window = |app: &App| {
            let (top, rows) = app.calendar.as_ref().unwrap().month_window();
            (top.to_string(), rows)
        };
        assert_eq!(window(&app), ("2026-04-27".into(), 5));
        // Inside the grid: it stays.
        app.cal_move(21);
        assert_eq!(window(&app), ("2026-04-27".into(), 5));
        // Down off the last row: one week on, not June whole.
        app.cal_move(7);
        assert_eq!(
            app.calendar.as_ref().unwrap().date.to_string(),
            "2026-06-03"
        );
        assert_eq!(window(&app), ("2026-05-04".into(), 5));
        // Back up inside it, then off the top: one week back.
        app.cal_move(-28);
        assert_eq!(window(&app), ("2026-05-04".into(), 5));
        app.cal_move(-7);
        assert_eq!(window(&app), ("2026-04-27".into(), 5));
        // `>` goes to the next month whole.
        app.cal_move(28);
        app.cal_page(true);
        assert_eq!(
            app.calendar.as_ref().unwrap().date.to_string(),
            "2026-06-27"
        );
        assert_eq!(window(&app), ("2026-06-01".into(), 5));
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
