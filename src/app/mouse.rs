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
        let hit = self.hits.at(x, y)?;
        self.sidebar_focus = false;
        match hit {
            Hit::Nav(item) => {
                self.sidebar_open(&item);
                if item == NavItem::Search {
                    return Some(Action::BeginSearch);
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
        }
        None
    }

    /// The wheel: up and down through whatever's under the pointer.
    pub fn wheel(&mut self, down: bool) -> Option<Action> {
        if let Some(s) = self.notes_screen.as_mut() {
            s.scroll = if down {
                s.scroll.saturating_add(2)
            } else {
                s.scroll.saturating_sub(2)
            };
            return None;
        }
        Some(if down {
            Action::CursorDown
        } else {
            Action::CursorUp
        })
    }
}
