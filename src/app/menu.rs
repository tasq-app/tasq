//! The shortcut menu (`space`): what you can do from where you are, one key
//! each, so the shortcuts can be learnt by looking instead of memorised.
//! Entries ending in `…` open a page of their own.

use super::App;
use super::types::{MenuPage, Mode};
use crate::action::Action;

/// What a menu key does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuDo {
    Act(Action),
    Page(MenuPage),
    /// Give the keyboard to the sidebar.
    FocusSidebar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuEntry {
    pub key: char,
    pub label: &'static str,
    pub does: MenuDo,
}

const fn act(key: char, label: &'static str, a: Action) -> MenuEntry {
    MenuEntry {
        key,
        label,
        does: MenuDo::Act(a),
    }
}

const fn page(key: char, label: &'static str, p: MenuPage) -> MenuEntry {
    MenuEntry {
        key,
        label,
        does: MenuDo::Page(p),
    }
}

/// The entries of a menu page, in reading order (left to right, top to
/// bottom in three columns).
pub fn entries(p: MenuPage) -> Vec<MenuEntry> {
    match p {
        MenuPage::Root => vec![
            act('n', "new task", Action::BeginAdd),
            act('x', "done", Action::ToggleComplete),
            act('e', "edit", Action::BeginEdit),
            act('r', "reschedule", Action::Reschedule),
            act('p', "priority", Action::CyclePriority),
            act('*', "star", Action::ToggleStar),
            page('g', "go to…", MenuPage::Go),
            act('f', "filter…", Action::OpenFilters),
            act('o', "notes", Action::OpenNotes),
            act('/', "search", Action::BeginSearch),
            act(':', "commands", Action::OpenCommandPalette),
            act('u', "undo", Action::Undo),
            act('[', "sidebar", Action::ToggleLeftPane),
            act(']', "details", Action::ToggleRightPane),
            act(',', "settings", Action::OpenSettings),
            act('?', "help", Action::OpenHelp),
        ],
        MenuPage::Go => vec![
            act('h', "home", Action::GoHome),
            act('i', "inbox", Action::GoInbox),
            act('t', "today", Action::ScopeToday),
            act('u', "upcoming", Action::ScopeUpcoming),
            act('a', "all tasks", Action::ScopeAll),
            act('d', "day", Action::CalendarDay),
            act('w', "week", Action::CalendarWeek),
            act('m', "month", Action::CalendarMonth),
            act('r', "archive", Action::ToggleArchiveView),
            MenuEntry {
                key: 's',
                label: "sidebar",
                does: MenuDo::FocusSidebar,
            },
        ],
    }
}

impl App {
    pub fn open_menu(&mut self) {
        self.menu_page = MenuPage::Root;
        self.mode = Mode::Menu;
    }

    pub fn close_menu(&mut self) {
        self.mode = Mode::Normal;
        self.menu_page = MenuPage::Root;
    }

    /// A key pressed in the menu: what it does, if anything. Pages and the
    /// sidebar are handled here; actions go back to the caller to apply.
    pub fn menu_key(&mut self, key: char) -> Option<Action> {
        let entry = entries(self.menu_page).into_iter().find(|e| e.key == key)?;
        match entry.does {
            MenuDo::Page(p) => {
                self.menu_page = p;
                None
            }
            MenuDo::Act(a) => {
                self.close_menu();
                Some(a)
            }
            MenuDo::FocusSidebar => {
                self.close_menu();
                self.sidebar_toggle_focus();
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_support::build_app;

    #[test]
    fn the_menu_walks_pages_and_hands_back_actions() {
        let mut app = build_app("a +x\n");
        app.open_menu();
        assert_eq!(app.menu_key('g'), None);
        assert_eq!(app.menu_page, MenuPage::Go);
        assert_eq!(app.menu_key('w'), Some(Action::CalendarWeek));
        assert_eq!(app.mode, Mode::Normal);
        app.open_menu();
        assert_eq!(app.menu_key('f'), Some(Action::OpenFilters));
        // Every key is unique on its page.
        for p in [MenuPage::Root, MenuPage::Go] {
            let keys: Vec<char> = entries(p).iter().map(|e| e.key).collect();
            let mut dedup = keys.clone();
            dedup.sort_unstable();
            dedup.dedup();
            assert_eq!(keys.len(), dedup.len(), "{p:?}");
        }
    }
}
