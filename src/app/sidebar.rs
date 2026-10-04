//! The sidebar: every place in tasq, Notion-style — the views, the spaces
//! as a tree in their colours, the built-in and saved filters, and you.
//!
//! It opens and closes like an accordion (`[`), and takes the keyboard with
//! `Tab`: `↑`/`↓` move, `Enter` opens, `Tab` or `Esc` go back to the list.

use std::time::{Duration, Instant};

use super::App;
use super::types::{Filter, Mode, Preset, Scope};
use crate::core::{filter, spaces};

/// Full width of the sidebar, in columns.
pub const SIDEBAR_W: u16 = 28;
/// How long it takes to open or close.
pub const SIDEBAR_SLIDE: Duration = Duration::from_millis(160);

/// One place the sidebar can take you.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavItem {
    Home,
    Inbox,
    Today,
    Upcoming,
    All,
    Calendar,
    Notes,
    Search,
    Trash,
    Space(String),
    Preset(Preset),
    /// A saved filter, by its index.
    Saved(usize),
}

/// A row of the sidebar, ready to draw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavRow {
    pub item: NavItem,
    pub label: String,
    /// Nesting of a sub-space.
    pub depth: usize,
    pub count: Option<usize>,
    /// A hidden space, drawn dimmed.
    pub dimmed: bool,
}

impl App {
    /// Every row of the sidebar, in order: views, spaces, filters.
    pub fn sidebar_rows(&self) -> Vec<NavRow> {
        let tasks = self.store.tasks();
        let today = self.store.today();
        let known = self.store.known_spaces();
        let open_in = |scope: Scope| {
            tasks
                .iter()
                .filter(|t| {
                    filter::in_scope(t, scope, today)
                        && !spaces::hidden_from_view(&t.projects, known, None)
                        && filter::list_predicate(
                            t,
                            false,
                            self.prefs.show_future,
                            today,
                            &Filter::default(),
                            None,
                        )
                })
                .count()
        };
        let row = |item: NavItem, label: &str, count: Option<usize>| NavRow {
            item,
            label: label.to_string(),
            depth: 0,
            count,
            dimmed: false,
        };
        let inbox = tasks
            .iter()
            .filter(|t| filter::passes_preset(t, Preset::Inbox))
            .count();
        let mut rows = vec![
            row(NavItem::Home, "Home", None),
            row(NavItem::Inbox, "Inbox", Some(inbox)),
            row(NavItem::Today, "Today", Some(open_in(Scope::Today))),
            row(
                NavItem::Upcoming,
                "Upcoming",
                Some(open_in(Scope::Upcoming)),
            ),
            row(NavItem::All, "All tasks", Some(open_in(Scope::All))),
            row(NavItem::Calendar, "Calendar", None),
            row(NavItem::Notes, "Notes", None),
            row(NavItem::Search, "Search", None),
            row(NavItem::Trash, "Trash", Some(self.store.trash().len())),
        ];
        for s in self.store.space_tree() {
            rows.push(NavRow {
                label: spaces::leaf(&s.path).to_string(),
                item: NavItem::Space(s.path),
                depth: s.depth,
                count: Some(s.count),
                dimmed: s.hidden,
            });
        }
        for p in Preset::ALL {
            let n = tasks
                .iter()
                .filter(|t| !t.done && filter::passes_preset(t, p))
                .count();
            rows.push(row(NavItem::Preset(p), p.label(), Some(n)));
        }
        for (i, f) in self.saved_filters().iter().enumerate() {
            let view = Filter::from_query(&f.query);
            let needle = filter::resolve_needle(&view.search, today);
            let n = tasks
                .iter()
                .filter(|t| !t.done && filter::passes_user_filter(t, &view, Some(&needle)))
                .count();
            rows.push(row(NavItem::Saved(i), &f.name, Some(n)));
        }
        rows
    }

    /// The place you're in, as the sidebar marks it.
    pub fn sidebar_active(&self) -> Option<NavItem> {
        if self.home {
            return Some(NavItem::Home);
        }
        if self.notes_screen.is_some() {
            return Some(NavItem::Notes);
        }
        if self.trash_screen.is_some() {
            return Some(NavItem::Trash);
        }
        if self.calendar.is_some() {
            return Some(NavItem::Calendar);
        }
        if self.filter.preset == Some(Preset::Inbox) {
            return Some(NavItem::Inbox);
        }
        if let Some(i) = self
            .saved_filters()
            .iter()
            .position(|f| Filter::from_query(&f.query).same_as(&self.filter))
            .filter(|_| self.filter.has_any())
        {
            return Some(NavItem::Saved(i));
        }
        if let Some(p) = self.filter.preset {
            return Some(NavItem::Preset(p));
        }
        if let Some(p) = &self.filter.project {
            return Some(NavItem::Space(p.clone()));
        }
        if !self.filter.search.is_empty() {
            return Some(NavItem::Search);
        }
        Some(match self.prefs.scope {
            Scope::Today => NavItem::Today,
            Scope::Upcoming => NavItem::Upcoming,
            Scope::All => NavItem::All,
        })
    }

    /// Give the keyboard to the sidebar (or back to the list), with the
    /// cursor on the place you're in.
    pub fn sidebar_toggle_focus(&mut self) {
        if self.sidebar_focus {
            self.sidebar_focus = false;
            return;
        }
        if !self.prefs.layout.left {
            self.sidebar_set_open(true);
        }
        let rows = self.sidebar_rows();
        let active = self.sidebar_active();
        self.sidebar_cursor = rows
            .iter()
            .position(|r| Some(&r.item) == active.as_ref())
            .unwrap_or(0);
        self.sidebar_focus = true;
    }

    pub fn sidebar_move(&mut self, forward: bool) {
        let n = self.sidebar_rows().len();
        if n == 0 {
            return;
        }
        self.sidebar_cursor = if forward {
            (self.sidebar_cursor + 1).min(n - 1)
        } else {
            self.sidebar_cursor.saturating_sub(1)
        };
    }

    /// The row under the sidebar's cursor.
    pub fn sidebar_current(&self) -> Option<NavItem> {
        self.sidebar_rows()
            .into_iter()
            .nth(self.sidebar_cursor)
            .map(|r| r.item)
    }

    /// Go to `item`. A view clears the space and built-in filters; a space
    /// or a filter shows all your tasks through it. `Search` is opened by
    /// the caller, which owns the search prompt.
    pub fn sidebar_open(&mut self, item: &NavItem) {
        self.calendar = None;
        self.home = false;
        self.notes_screen = None;
        self.trash_screen = None;
        self.mode = Mode::Normal;
        match item {
            NavItem::Trash => self.open_trash(),
            NavItem::Home => self.open_home(),
            NavItem::Notes => self.open_notes_screen(),
            NavItem::Inbox => {
                self.filter.clear();
                self.filter.preset = Some(Preset::Inbox);
                self.set_scope(Scope::All);
            }
            NavItem::Today | NavItem::Upcoming | NavItem::All => {
                self.filter.project = None;
                self.filter.preset = None;
                self.filter.search.clear();
                let scope = match item {
                    NavItem::Today => Scope::Today,
                    NavItem::Upcoming => Scope::Upcoming,
                    _ => Scope::All,
                };
                self.set_scope(scope);
            }
            NavItem::Calendar => self.open_cal(super::CalView::Day),
            NavItem::Search => {}
            NavItem::Space(p) => {
                self.filter.preset = None;
                self.filter.project = Some(p.clone());
                self.set_scope(Scope::All);
            }
            NavItem::Preset(p) => {
                self.filter.project = None;
                self.filter.preset = Some(*p);
                self.set_scope(Scope::All);
            }
            NavItem::Saved(i) => {
                if let Some(q) = self.saved_filters().get(*i).map(|f| f.query.clone()) {
                    self.filter = Filter::from_query(&q);
                    self.set_scope(Scope::All);
                }
            }
        }
    }

    /// Open or close the sidebar, sliding.
    pub fn sidebar_set_open(&mut self, open: bool) {
        if self.prefs.layout.left == open {
            return;
        }
        self.prefs.layout.left = open;
        self.sidebar_anim = Some(Instant::now());
        if !open {
            self.sidebar_focus = false;
        }
    }

    /// `[`: open or close the sidebar.
    pub fn sidebar_toggle(&mut self) {
        let open = !self.prefs.layout.left;
        self.sidebar_set_open(open);
        self.save_prefs();
    }

    /// How wide the sidebar is right now, mid-slide or not.
    pub fn sidebar_width(&self) -> u16 {
        let target = if self.prefs.layout.left { 1.0 } else { 0.0 };
        let p = match self.sidebar_anim {
            Some(t0) if t0.elapsed() < SIDEBAR_SLIDE => {
                let x = t0.elapsed().as_secs_f32() / SIDEBAR_SLIDE.as_secs_f32();
                let eased = 1.0 - (1.0 - x).powi(3);
                if self.prefs.layout.left {
                    eased
                } else {
                    1.0 - eased
                }
            }
            _ => target,
        };
        (f32::from(SIDEBAR_W) * p).round() as u16
    }

    /// Whether the sidebar is still sliding.
    pub fn sidebar_animating(&self) -> bool {
        self.sidebar_anim
            .is_some_and(|t0| t0.elapsed() < SIDEBAR_SLIDE)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::app::test_support::build_app;

    #[test]
    fn the_sidebar_lists_places_and_opens_them() {
        let mut app =
            build_app("(A) a +Uni/Exams plan:2026-05-06\nb +Uni star:1\nc due:2026-05-01\n");
        let rows = app.sidebar_rows();
        let items: Vec<&NavItem> = rows.iter().map(|r| &r.item).collect();
        assert!(items.contains(&&NavItem::Space("Uni/Exams".into())));
        assert_eq!(
            rows.iter()
                .find(|r| r.item == NavItem::Preset(Preset::HighPriority))
                .unwrap()
                .count,
            Some(1)
        );
        app.sidebar_open(&NavItem::Space("Uni".into()));
        assert_eq!(app.sidebar_active(), Some(NavItem::Space("Uni".into())));
        assert_eq!(app.visible_indices().len(), 2);
        app.sidebar_open(&NavItem::Preset(Preset::Starred));
        assert_eq!(app.visible_indices().len(), 1);
        app.sidebar_open(&NavItem::Today);
        assert_eq!(app.sidebar_active(), Some(NavItem::Today));
        assert!(app.filter.preset.is_none());

        app.sidebar_toggle_focus();
        assert!(app.sidebar_focus);
        assert_eq!(app.sidebar_current(), Some(NavItem::Today));
        app.sidebar_move(true);
        assert_eq!(app.sidebar_current(), Some(NavItem::Upcoming));
    }
}
