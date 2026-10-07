//! Dragging blocks in the day and week views: grab a block to move it to
//! another time or day, or its bottom edge to make it longer or shorter.
//! While it moves, a ghost shows where it would land; letting go makes the
//! change (asking first, for a repeating task).

use chrono::NaiveDate;

use super::App;
use super::calendar::SeriesOp;
use crate::core::calendar::Occurrence;

/// Minutes a dragged block snaps to.
const SNAP: u32 = 15;

/// A day column of the time grid as it was drawn: where it is and how
/// its rows map to the time of day.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CalCol {
    pub x: u16,
    pub w: u16,
    pub date: NaiveDate,
    /// The grid's first row and height.
    pub y: u16,
    pub h: u16,
    /// Minutes after midnight at the first row.
    pub top: u32,
    /// Rows per hour.
    pub scale: f64,
}

impl CalCol {
    /// The time at row `y`, snapped.
    pub fn minutes_at(&self, y: u16) -> u32 {
        let rows = f64::from(y.saturating_sub(self.y));
        let m = f64::from(self.top) + rows * 60.0 / self.scale;
        snap(m.max(0.0) as u32)
    }

    /// The row (from the grid's top) of `m` minutes after midnight.
    pub fn row_of(&self, m: u32) -> i64 {
        ((f64::from(m) - f64::from(self.top)) * self.scale / 60.0).floor() as i64
    }
}

fn snap(m: u32) -> u32 {
    ((m + SNAP / 2) / SNAP * SNAP).min(24 * 60)
}

/// A block being dragged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalDrag {
    pub occ: Occurrence,
    /// The bottom edge: changing its length.
    pub resize: bool,
    /// Minutes between the block's start and where it was grabbed.
    pub grab: i64,
    /// Where it would land: day, start, length.
    pub target: Option<(NaiveDate, u32, u32)>,
    /// It was already selected when pressed: let go without moving, it
    /// opens.
    pub was_selected: bool,
}

impl App {
    /// The day column under `x`, as last drawn.
    fn cal_col_at(&self, x: u16) -> Option<CalCol> {
        self.cal_cols
            .borrow()
            .iter()
            .find(|c| x >= c.x && x < c.x + c.w)
            .copied()
    }

    /// A press on a block (`resize`: on its bottom edge): it's selected,
    /// and dragging starts.
    pub fn cal_press(&mut self, occ: Occurrence, resize: bool, x: u16, y: u16) {
        let was_selected = self.cal_select_occ(&occ);
        let grab = match (self.cal_col_at(x), occ.start) {
            (Some(col), Some(s)) => i64::from(col.minutes_at(y)) - i64::from(s),
            _ => 0,
        };
        self.cal_drag = Some(CalDrag {
            occ,
            resize,
            grab,
            target: None,
            was_selected,
        });
    }

    /// Select the occurrence `occ` (its day, and it in the day). Returns
    /// whether it was selected already.
    pub fn cal_select_occ(&mut self, occ: &Occurrence) -> bool {
        let same = |a: &Occurrence, b: &Occurrence| {
            a.abs == b.abs && a.origin == b.origin && a.projected == b.projected
        };
        let was = self.cal_selected().is_some_and(|s| same(&s, occ));
        if let Some(c) = self.calendar.as_mut()
            && c.date != occ.date
            && !(was && occ.span > 1)
        {
            c.date = occ.date;
        }
        let items = self.cal_day_items();
        if let Some(i) = items.iter().position(|o| same(o, occ))
            && let Some(c) = self.calendar.as_mut()
        {
            c.selected = i;
        }
        was
    }

    /// The pointer dragged to `(x, y)`: where the block would land.
    pub fn cal_drag_to(&mut self, x: u16, y: u16) {
        let Some(drag) = self.cal_drag.as_ref() else {
            return;
        };
        let occ = &drag.occ;
        let Some(start) = occ.start else {
            return;
        };
        let target = if drag.resize {
            let Some(col) = self
                .cal_cols
                .borrow()
                .iter()
                .find(|c| c.date == occ.date)
                .copied()
            else {
                return;
            };
            let end = col.minutes_at(y.saturating_add(1)).max(start + SNAP);
            (occ.date, start, end - start)
        } else {
            let Some(col) = self.cal_col_at(x) else {
                return;
            };
            let at = i64::from(col.minutes_at(y)) - drag.grab;
            let latest = i64::from(24 * 60 - occ.minutes.min(24 * 60));
            let s = snap(at.clamp(0, latest.max(0)) as u32);
            (col.date, s, occ.minutes)
        };
        let same = target == (occ.date, start, occ.minutes);
        if let Some(d) = self.cal_drag.as_mut() {
            d.target = (!same).then_some(target);
        }
    }

    /// Let go: the block moves (or takes its new length) there.
    pub fn cal_drop(&mut self) {
        let Some(drag) = self.cal_drag.take() else {
            return;
        };
        let Some((date, start, minutes)) = drag.target else {
            // A click, not a drag, on the selected block: edit it.
            if drag.was_selected {
                self.cal_edit();
            }
            return;
        };
        let op = if drag.resize {
            SeriesOp::Resize(minutes)
        } else {
            SeriesOp::MoveTo {
                date,
                start: Some(start),
            }
        };
        self.cal_apply(drag.occ, op);
    }

    /// A click on the grid off any block: that day is selected.
    pub fn cal_click_day(&mut self, x: u16, y: u16) -> bool {
        let Some(col) = self.cal_col_at(x) else {
            return false;
        };
        if y < col.y || y >= col.y + col.h {
            return false;
        }
        if let Some(c) = self.calendar.as_mut()
            && c.date != col.date
        {
            c.date = col.date;
            c.selected = 0;
        }
        true
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::app::CalView;
    use crate::app::test_support::build_app;

    #[test]
    fn dragging_a_block_moves_it_and_its_edge_resizes_it() {
        let mut app = build_app("Study plan:2026-05-06 at:09:00 dur:1h\n");
        app.open_cal(CalView::Week);
        let d = |s: &str| NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
        // Two columns, two rows an hour from 08:00.
        let col = |x: u16, date: &str| CalCol {
            x,
            w: 10,
            date: d(date),
            y: 10,
            h: 30,
            top: 8 * 60,
            scale: 2.0,
        };
        *app.cal_cols.borrow_mut() = vec![col(0, "2026-05-06"), col(10, "2026-05-07")];
        let occ = app.cal_selected().unwrap();
        // Grabbed at 09:00 (row 12), dropped a day on, an hour later.
        app.cal_press(occ.clone(), false, 2, 12);
        app.cal_drag_to(12, 14);
        assert_eq!(
            app.cal_drag.as_ref().unwrap().target,
            Some((d("2026-05-07"), 10 * 60, 60))
        );
        app.cal_drop();
        let raw = &app.tasks()[0].raw;
        assert!(
            raw.contains("plan:2026-05-07") && raw.contains("at:10:00"),
            "{raw}"
        );

        // Its bottom edge pulled down an hour: two hours long.
        let occ = app.cal_selected().unwrap();
        app.cal_press(occ, true, 12, 15);
        app.cal_drag_to(12, 17);
        app.cal_drop();
        assert!(
            app.tasks()[0].raw.contains("dur:2h"),
            "{}",
            app.tasks()[0].raw
        );
    }

    #[test]
    fn a_dragged_block_shows_a_ghost_where_it_would_land() {
        use ratatui::{Terminal, backend::TestBackend};
        let mut app = build_app("Study plan:2026-05-06 at:09:00 dur:1h\n");
        app.frozen_now = Some(u32::MAX);
        app.open_cal(CalView::Day);
        let mut term = Terminal::new(TestBackend::new(100, 40)).unwrap();
        let text = |term: &Terminal<TestBackend>| -> Vec<String> {
            let buf = term.backend().buffer();
            (0..buf.area.height)
                .map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect())
                .collect()
        };
        term.draw(|f| crate::ui::draw(f, &app)).unwrap();
        let rows = text(&term);
        let (y, x) = rows
            .iter()
            .enumerate()
            .find_map(|(y, r)| {
                r.find("Study")
                    .map(|i| (y as u16, r[..i].chars().count() as u16))
            })
            .unwrap();
        app.click(x, y);
        assert!(app.cal_drag.is_some(), "pressed on the block");
        app.cal_drag_to(x, y + 6);
        term.draw(|f| crate::ui::draw(f, &app)).unwrap();
        let rows = text(&term);
        if std::env::var("SHOW").is_ok() {
            println!("{}", rows.join("\n"));
        }
        let (_, start, _) = app.cal_drag.as_ref().unwrap().target.unwrap();
        let label = format!("{:02}:{:02}–", start / 60, start % 60);
        assert!(
            rows.iter().any(|r| r.contains(&label)),
            "{label}\n{}",
            rows.join("\n")
        );
    }
}
