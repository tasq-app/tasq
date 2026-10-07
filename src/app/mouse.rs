//! The mouse: every frame notes where the things you can click are, and a
//! click (or the wheel) does what the matching key would.

use std::cell::RefCell;

use ratatui::layout::Rect;

use super::{App, HomeSel, NavItem};
use crate::action::Action;

/// Something on screen a click can land on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hit {
    /// A sidebar row.
    Nav(NavItem),
    /// A task row in the list, by its place in the list.
    Row(usize),
    /// That row's checkbox.
    Check(usize),
    /// Home's "Add a task" bar.
    HomeCapture,
    /// A Home tile.
    HomeTile(usize),
    /// An item in a Home tile.
    HomeItem(usize, usize),
    /// A checklist item in the inspector.
    CheckItem(usize),
    /// A note card in the inspector.
    NoteCard(usize),
    /// The edge between the list and the inspector, to drag.
    DetailsEdge,
    /// The "+ filter" chip.
    AddFilter,
    /// An active filter's chip: a click takes it off.
    ClearFilter(FilterPart),
    /// A colour in a space's colour picker.
    Swatch(usize),
    /// A block in the calendar's time grid, and whether it's its bottom
    /// edge (to drag its length).
    CalBlock(Box<crate::core::calendar::Occurrence>, bool),
    /// An all-day item in the calendar (a band across days, a chip): a
    /// click selects it, a second one edits it.
    CalItem(Box<crate::core::calendar::Occurrence>),
    /// A day in the month: a click selects it, a second one opens it.
    CalDay(chrono::NaiveDate),
    /// A result in the Search screen.
    SearchRow(usize),
    /// An option in the "+ filter" popover.
    FilterRow(usize),
    /// The Notes screen's search box.
    NotesSearch,
    /// A note in the Notes screen's list.
    NoteRow(usize),
}

/// Which of the active filters a chip is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterPart {
    Space,
    Tag,
    Search,
    Preset,
    /// The "clear all" chip.
    All,
}

/// Where the clickable things were drawn this frame.
#[derive(Debug, Default)]
pub struct Hits(RefCell<Vec<(Rect, Hit)>>);

impl Hits {
    pub fn clear(&self) {
        self.0.borrow_mut().clear();
    }

    pub fn add(&self, r: Rect, hit: Hit) {
        if r.width > 0 && r.height > 0 {
            self.0.borrow_mut().push((r, hit));
        }
    }

    /// The topmost thing at `(x, y)`.
    pub fn at(&self, x: u16, y: u16) -> Option<Hit> {
        self.0
            .borrow()
            .iter()
            .rev()
            .find(|(r, _)| r.contains((x, y).into()))
            .map(|(_, h)| h.clone())
    }
}

impl App {
    /// The details pane a few columns wider (or narrower), saved.
    pub fn resize_details(&mut self, wider: bool) {
        use super::prefs::{DETAILS_MAX, DETAILS_MIN};
        let w = &mut self.prefs.details_w;
        *w = if wider {
            (*w + 4).min(DETAILS_MAX)
        } else {
            w.saturating_sub(4).max(DETAILS_MIN)
        };
        self.prefs.layout.right = true;
        self.save_prefs();
    }

    /// Dragging the details pane's edge to column `x`.
    pub fn drag_details_to(&mut self, x: u16) {
        use super::prefs::{DETAILS_MAX, DETAILS_MIN};
        let screen = self.screen_w.get();
        self.prefs.details_w = screen
            .saturating_sub(x)
            .clamp(DETAILS_MIN, DETAILS_MAX.min(screen / 2).max(DETAILS_MIN));
    }

    /// A left click at `(x, y)`: what it hit, done. Some clicks are actions
    /// the caller applies.
    pub fn click(&mut self, x: u16, y: u16) -> Option<Action> {
        // The colour picker is on top: a swatch picks, anywhere else closes.
        if self.color_pick.is_some() {
            match self.hits.at(x, y) {
                Some(Hit::Swatch(i)) => self.color_pick_click(i),
                _ => self.color_pick = None,
            }
            return None;
        }
        // The Search screen and the filter popover float on top: a click on
        // one of their rows takes it, anywhere else closes them.
        if matches!(self.mode, super::Mode::SearchAll | super::Mode::Filters) {
            match self.hits.at(x, y) {
                Some(Hit::SearchRow(i)) => {
                    self.search_all.cursor = i;
                    self.search_all_go();
                }
                Some(Hit::FilterRow(i)) => {
                    self.filter_pop.cursor = i;
                    self.filter_pop_pick();
                }
                _ if self.mode == super::Mode::SearchAll => self.close_search_all(),
                _ => self.close_filters(),
            }
            return None;
        }
        let Some(hit) = self.hits.at(x, y) else {
            // Off any block in the calendar's grid: that day.
            if self.calendar.is_some() {
                self.cal_click_day(x, y);
            }
            return None;
        };
        self.sidebar_focus = false;
        match hit {
            Hit::Nav(item) => {
                self.sidebar_open(&item);
                // The sidebar takes the keyboard: `c`, `a`, `r`… now act
                // on the space clicked.
                if let Some(i) = self.sidebar_rows().iter().position(|r| r.item == item) {
                    self.sidebar_cursor = i;
                }
                self.sidebar_focus = true;
                if item == NavItem::Search {
                    return Some(Action::SearchAll);
                }
            }
            Hit::Row(i) => {
                self.inspector_focus = false;
                self.cursor = i;
            }
            Hit::Check(i) => {
                self.cursor = i;
                return Some(Action::ToggleComplete);
            }
            Hit::HomeCapture => return Some(Action::BeginAdd),
            Hit::HomeTile(tile) => {
                self.home_sel = Some(HomeSel { tile, item: None });
            }
            Hit::HomeItem(tile, i) => {
                if let Some(item) = self.home_items(tile).into_iter().nth(i) {
                    self.home_go(tile, &item);
                }
            }
            Hit::CheckItem(i) => {
                self.inspector_focus = true;
                self.inspector_cursor = i;
                self.inspector_toggle();
            }
            Hit::NoteCard(i) => {
                self.inspector_focus = true;
                let notes_from = self
                    .inspector_rows()
                    .iter()
                    .position(|r| matches!(r, super::InspectorRow::Note(_)))
                    .unwrap_or(0);
                self.inspector_cursor = notes_from + i;
                self.inspector_open_note();
            }
            Hit::DetailsEdge => self.resizing = true,
            Hit::AddFilter => return Some(Action::OpenFilters),
            Hit::ClearFilter(part) => self.clear_filter_part(part),
            Hit::Swatch(i) => self.color_pick_click(i),
            Hit::SearchRow(_) | Hit::FilterRow(_) => {}
            Hit::NotesSearch => {
                if let Some(s) = self.notes_screen.as_mut() {
                    s.searching = true;
                }
            }
            // A note: selected; clicked again, read.
            Hit::NoteRow(i) => {
                let again = self.notes_screen.as_ref().is_some_and(|s| s.cursor == i);
                if let Some(s) = self.notes_screen.as_mut() {
                    s.cursor = i;
                    s.scroll = 0;
                    s.searching = false;
                }
                if again {
                    self.notes_screen_read(true);
                }
            }
            Hit::CalBlock(occ, edge) => self.cal_press(*occ, edge, x, y),
            Hit::CalItem(occ) => {
                if self.cal_select_occ(&occ) {
                    self.cal_edit();
                }
            }
            Hit::CalDay(date) => {
                let again = self.calendar.as_ref().is_some_and(|c| c.date == date);
                if let Some(c) = self.calendar.as_mut() {
                    c.date = date;
                    c.selected = 0;
                }
                if again {
                    self.cal_edit();
                }
            }
        }
        None
    }

    /// The wheel: up and down through whatever's under the pointer.
    pub fn wheel(&mut self, down: bool) -> Option<Action> {
        match self.mode {
            super::Mode::SearchAll => {
                self.search_all_move(down);
                return None;
            }
            super::Mode::Filters => {
                self.filter_pop_move(down);
                return None;
            }
            _ => {}
        }
        if self.notes_screen.is_some() {
            self.notes_screen_scroll_by(if down { 2 } else { -2 });
            return None;
        }
        Some(if down {
            Action::CursorDown
        } else {
            Action::CursorUp
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::app::NavItem;
    use crate::app::test_support::build_app;
    use ratatui::{Terminal, backend::TestBackend};

    /// Where `needle` was drawn: its first cell.
    fn find(term: &Terminal<TestBackend>, needle: &str) -> (u16, u16) {
        find_before(term, needle, u16::MAX)
    }

    /// Like [`find`], left of column `max_x` (the sidebar).
    fn find_before(term: &Terminal<TestBackend>, needle: &str, max_x: u16) -> (u16, u16) {
        let buf = term.backend().buffer();
        (0..buf.area.height)
            .find_map(|y| {
                let row: String = (0..buf.area.width.min(max_x))
                    .map(|x| buf[(x, y)].symbol())
                    .collect();
                row.find(needle)
                    .map(|i| (row[..i].chars().count() as u16, y))
            })
            .unwrap()
    }

    #[test]
    fn a_click_on_a_space_gives_the_sidebar_the_keyboard() {
        let mut app = build_app("study +Uni/Exams\nrent +Errands\n");
        let mut term = Terminal::new(TestBackend::new(120, 30)).unwrap();
        term.draw(|f| crate::ui::draw(f, &app)).unwrap();
        let (x, y) = find_before(&term, "Errands", 24);
        app.click(x, y);
        assert!(app.sidebar_focus);
        assert_eq!(
            app.sidebar_current(),
            Some(NavItem::Space("Errands".into()))
        );
        // `c` now opens the colour picker for it, not the @ prompt.
        app.open_color_pick();
        assert_eq!(app.color_pick.as_ref().unwrap().path, "Errands");
    }

    #[test]
    fn notes_and_search_results_answer_a_click() {
        let dir = std::env::temp_dir().join(format!(
            "tasq-notes-click-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let folder = dir.join("tasks").join("abc");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("a.md"), "# Alpha\n").unwrap();
        std::fs::write(folder.join("b.md"), "# Beta\n").unwrap();
        let cfg = crate::config::Config {
            notes_dir: Some(dir.to_string_lossy().into_owned()),
            ..Default::default()
        };
        let mut app = crate::app::test_support::build_app_with_config("study notes:abc/\n", cfg);
        app.open_notes_screen();
        let mut term = Terminal::new(TestBackend::new(120, 30)).unwrap();
        term.draw(|f| crate::ui::draw(f, &app)).unwrap();
        // The search box starts a search.
        let (x, y) = find(&term, "⌕ search notes");
        app.click(x, y);
        assert!(app.notes_screen.as_ref().unwrap().searching);
        app.notes_screen.as_mut().unwrap().searching = false;
        // A note: selected, then read on a second click.
        term.draw(|f| crate::ui::draw(f, &app)).unwrap();
        let other = if app.current_note_entry().unwrap().title == "Alpha" {
            "Beta"
        } else {
            "Alpha"
        };
        let (x, y) = find(&term, other);
        app.click(x, y);
        assert_eq!(app.current_note_entry().unwrap().title, other);
        app.click(x, y);
        assert!(app.notes_screen.as_ref().unwrap().reading);
        let _ = std::fs::remove_dir_all(&dir);

        // The Search screen: a click on a result goes there.
        let mut app = build_app("water plants\nbuy milk\n");
        app.mode = crate::app::Mode::SearchAll;
        for c in "milk".chars() {
            app.search_all_type(c);
        }
        term.draw(|f| crate::ui::draw(f, &app)).unwrap();
        let (x, y) = find(&term, "buy milk");
        app.click(x, y);
        assert_eq!(app.mode, crate::app::Mode::Normal);
        assert_eq!(app.cur_abs(), Some(1));
    }

    #[test]
    fn all_day_items_in_the_calendar_answer_a_click() {
        use crate::app::{CalView, Mode};
        // Today is 2026-05-06, a wednesday.
        let raw = "Trip event:1 plan:2026-05-07 end:2026-05-09\nGym plan:2026-05-06 at:07:00\nCall plan:2026-05-07 at:10:00\n";
        for view in [CalView::Week, CalView::Day] {
            let mut app = build_app(raw);
            app.open_cal(view);
            if view == CalView::Day {
                // Something else selected first: the call.
                app.cal_move(1);
                app.cal_select(true);
            }
            let mut term = Terminal::new(TestBackend::new(140, 40)).unwrap();
            term.draw(|f| crate::ui::draw(f, &app)).unwrap();
            let (x, y) = find(&term, "Trip");
            // First click: selected (with its day).
            app.click(x, y);
            assert_eq!(app.cal_selected().unwrap().abs, 0, "{view:?}");
            assert_eq!(app.mode, Mode::Normal);
            // Keys now act on it; a second click edits it.
            term.draw(|f| crate::ui::draw(f, &app)).unwrap();
            app.click(x, y);
            assert_eq!(app.mode, Mode::Insert, "{view:?}");
            assert_eq!(app.selection.editing(), Some(0), "{view:?}");
        }

        // The month: a click on a day selects it.
        let mut app = build_app(raw);
        app.open_cal(CalView::Month);
        let mut term = Terminal::new(TestBackend::new(140, 40)).unwrap();
        term.draw(|f| crate::ui::draw(f, &app)).unwrap();
        let (x, y) = find(&term, " 20 ");
        app.click(x, y);
        assert_eq!(
            app.calendar.as_ref().unwrap().date.to_string(),
            "2026-05-20"
        );
    }
}
