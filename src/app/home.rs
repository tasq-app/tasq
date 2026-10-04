//! Home: the day at a glance when tasq opens — what's on today, the week
//! ahead, your routines, your spaces, the notes you touched last and what's
//! waiting in the inbox.

use chrono::{Days, NaiveDate, Timelike};

use super::App;
use super::types::{Mode, Preset, Scope};
use crate::core::{calendar, filter, spaces};
use crate::todo;

/// One of the coming days, for the week heatmap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeatDay {
    pub date: NaiveDate,
    /// Things planned or due that day.
    pub count: usize,
}

/// A note you changed lately.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentNote {
    pub path: std::path::PathBuf,
    pub title: String,
    /// "2h ago", "yesterday", "thu"…
    pub when: String,
}

/// A note's `# ` heading, else its file name.
pub fn note_title(path: &std::path::Path, body: &str) -> String {
    body.lines()
        .find_map(|l| l.strip_prefix("# "))
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("note")
                .replace(['-', '_'], " ")
        })
}

/// When, in a few words: "just now", "5m ago", "2h ago", "yesterday",
/// "thu", "28 sep".
pub fn ago(at: chrono::DateTime<chrono::Utc>, today: NaiveDate) -> String {
    let mins = (chrono::Utc::now() - at).num_minutes().max(0);
    let day = at.with_timezone(&chrono::Local).date_naive();
    if mins < 1 {
        "just now".to_string()
    } else if mins < 60 {
        format!("{mins}m ago")
    } else if day == today {
        format!("{}h ago", mins / 60)
    } else if Some(day) == today.pred_opt() {
        "yesterday".to_string()
    } else if (today - day).num_days() < 7 {
        day.format("%a").to_string().to_lowercase()
    } else {
        day.format("%-d %b").to_string().to_lowercase()
    }
}

impl App {
    pub fn open_home(&mut self) {
        self.calendar = None;
        self.notes_screen = None;
        self.trash_screen = None;
        self.home = true;
        self.mode = Mode::Normal;
        self.inspector_focus = false;
    }

    /// Leave Home for Today.
    pub fn close_home(&mut self) {
        self.home = false;
        self.filter.clear();
        self.set_scope(Scope::Today);
    }

    /// "Good morning, jf".
    pub fn greeting(&self) -> String {
        let hour = chrono::Local::now().hour();
        let part = match hour {
            5..=11 => "Good morning",
            12..=19 => "Good afternoon",
            _ => "Good evening",
        };
        format!("{part}, {}", self.user_name)
    }

    /// Today's tasks as the Today view has them: `(done, total)` and the
    /// open ones, first first.
    pub fn home_today(&self) -> (usize, usize, Vec<usize>) {
        let today = self.store.today();
        let tasks = self.store.tasks();
        let known = self.store.known_spaces();
        let ids: Vec<usize> = tasks
            .iter()
            .enumerate()
            .filter(|(_, t)| {
                filter::in_scope(t, Scope::Today, today)
                    && !spaces::hidden_from_view(&t.projects, known, None)
                    && (!t.done || t.done_date.as_deref() == Some(today))
            })
            .map(|(i, _)| i)
            .collect();
        let done = ids.iter().filter(|&&i| tasks[i].done).count();
        let mut open: Vec<usize> = ids.into_iter().filter(|&i| !tasks[i].done).collect();
        open.sort_by_key(|&i| super::TodaySlot::of(&tasks[i], today));
        (done, done + open.len(), open)
    }

    /// Today and the six days after it, with how much is on each.
    pub fn home_week(&self) -> Vec<HeatDay> {
        let today = self.today_naive();
        let from = today;
        let Some(to) = from.checked_add_days(Days::new(6)) else {
            return Vec::new();
        };
        let occs = calendar::occurrences(self.store.tasks(), from, to, today);
        (0..7)
            .filter_map(|k| from.checked_add_days(Days::new(k)))
            .map(|date| HeatDay {
                date,
                count: occs.iter().filter(|o| o.date == date).count(),
            })
            .collect()
    }

    /// The next thing with a time today: its time, title and how long till.
    pub fn home_next_up(&self) -> Option<(String, String, String)> {
        let today = self.today_naive();
        let now = chrono::Local::now();
        let now_min = now.hour() * 60 + now.minute();
        let occs = calendar::occurrences(self.store.tasks(), today, today, today);
        let o = occs
            .iter()
            .filter(|o| !self.store.tasks()[o.abs].done)
            .filter_map(|o| o.start.map(|s| (s, o)))
            .filter(|(s, _)| *s >= now_min)
            .min_by_key(|(s, _)| *s)?;
        let (start, occ) = o;
        let left = start - now_min;
        let till = if left < 60 {
            format!("in {left}m")
        } else if left.is_multiple_of(60) {
            format!("in {}h", left / 60)
        } else {
            format!("in {}h {}m", left / 60, left % 60)
        };
        let title = todo::body_only(&self.store.tasks()[occ.abs].raw);
        Some((format!("{:02}:{:02}", start / 60, start % 60), title, till))
    }

    /// Top-level spaces with how many open tasks each, busiest first.
    pub fn home_spaces(&self) -> Vec<(String, usize)> {
        let mut out: Vec<(String, usize)> = self
            .store
            .space_tree()
            .into_iter()
            .filter(|s| s.depth == 0 && !s.hidden && s.count > 0)
            .map(|s| (s.path, s.count))
            .collect();
        out.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        out
    }

    /// The notes you changed last.
    pub fn home_recent_notes(&self, limit: usize) -> Vec<RecentNote> {
        let today = self.today_naive();
        crate::note_store::recent(self.notes_dir(), limit)
            .into_iter()
            .map(|(path, at)| {
                let body = crate::note_store::read(&path).unwrap_or_default();
                RecentNote {
                    title: note_title(&path, &body),
                    when: ago(at, today),
                    path,
                }
            })
            .collect()
    }

    /// What's in the inbox, oldest first.
    pub fn inbox(&self) -> Vec<usize> {
        self.store
            .tasks()
            .iter()
            .enumerate()
            .filter(|(_, t)| filter::passes_preset(t, Preset::Inbox))
            .map(|(i, _)| i)
            .collect()
    }

    /// Open the inbox in the list.
    pub fn open_inbox(&mut self) {
        self.sidebar_open(&super::NavItem::Inbox);
    }
}

/// The tiles of Home, in reading order; 0 is the "Add a task" bar.
pub const HOME_TILES: usize = 7;

/// Something on Home you can go to or act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HomeItem {
    Task(usize),
    Day(NaiveDate),
    Space(String),
    Note(std::path::PathBuf),
}

/// Where Home's keyboard selection is: a tile, and maybe an item in it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HomeSel {
    pub tile: usize,
    pub item: Option<usize>,
}

impl App {
    /// The items of `tile` (1 today, 2 week, 3 routines, 4 spaces, 5
    /// notes, 6 inbox), as Home lists them.
    pub fn home_items(&self, tile: usize) -> Vec<HomeItem> {
        match tile {
            1 => self
                .home_today()
                .2
                .into_iter()
                .map(HomeItem::Task)
                .collect(),
            2 => self
                .home_week()
                .into_iter()
                .map(|d| HomeItem::Day(d.date))
                .collect(),
            3 => self
                .routines()
                .into_iter()
                .map(|r| HomeItem::Task(r.abs))
                .collect(),
            4 => self
                .home_spaces()
                .into_iter()
                .map(|(p, _)| HomeItem::Space(p))
                .collect(),
            5 => self
                .home_recent_notes(8)
                .into_iter()
                .map(|n| HomeItem::Note(n.path))
                .collect(),
            6 => self.inbox().into_iter().map(HomeItem::Task).collect(),
            _ => Vec::new(),
        }
    }

    /// `Tab` on Home: the next tile (after the last, the sidebar).
    pub fn home_tab(&mut self, forward: bool) {
        let next = match self.home_sel {
            None if forward => Some(0),
            None => Some(HOME_TILES - 1),
            Some(s) if forward && s.tile + 1 < HOME_TILES => Some(s.tile + 1),
            Some(s) if !forward && s.tile > 0 => Some(s.tile - 1),
            Some(_) => None,
        };
        match next {
            Some(tile) => self.home_sel = Some(HomeSel { tile, item: None }),
            None => {
                self.home_sel = None;
                self.sidebar_toggle_focus();
            }
        }
    }

    /// Arrows between tiles (three across, the bar on top); inside a tile,
    /// up and down move between its items.
    pub fn home_arrow(&mut self, dx: i32, dy: i32) {
        let Some(mut s) = self.home_sel else {
            self.home_sel = Some(HomeSel::default());
            return;
        };
        if let Some(i) = s.item {
            let n = self.home_items(s.tile).len();
            let step = if s.tile == 2 { dx } else { dy };
            let i = (i as i32 + step).clamp(0, n.saturating_sub(1) as i32) as usize;
            s.item = Some(i);
            self.home_sel = Some(s);
            return;
        }
        s.tile = match (s.tile, dx, dy) {
            (0, _, 1) => 1,
            (0, ..) => 0,
            (t, _, -1) if t <= 3 => 0,
            (t, _, -1) => t - 3,
            (t, _, 1) if t <= 3 => t + 3,
            (t, -1, _) if t != 1 && t != 4 => t - 1,
            (t, 1, _) if t != 3 && t != 6 => t + 1,
            (t, ..) => t,
        };
        self.home_sel = Some(s);
    }

    /// `Enter` on Home: into a tile, or do what an item says.
    pub fn home_enter(&mut self) -> Option<crate::action::Action> {
        let s = self.home_sel?;
        if s.tile == 0 {
            return Some(crate::action::Action::BeginAdd);
        }
        match s.item {
            None => {
                if self.home_items(s.tile).is_empty() {
                    if s.tile == 6 {
                        self.open_inbox();
                    }
                } else {
                    self.home_sel = Some(HomeSel {
                        tile: s.tile,
                        item: Some(0),
                    });
                }
                None
            }
            Some(i) => {
                let item = self.home_items(s.tile).into_iter().nth(i)?;
                self.home_go(s.tile, &item);
                None
            }
        }
    }

    /// Go where an item points.
    pub fn home_go(&mut self, tile: usize, item: &HomeItem) {
        self.home_sel = None;
        match item {
            HomeItem::Task(abs) => {
                let raw = self.store.tasks().get(*abs).map(|t| t.raw.clone());
                match tile {
                    6 => self.open_inbox(),
                    1 => self.close_home(),
                    _ => {
                        self.home = false;
                        self.filter.clear();
                        self.set_scope(Scope::All);
                    }
                }
                if let Some(abs) =
                    raw.and_then(|r| self.store.tasks().iter().position(|t| t.raw == r))
                {
                    self.follow_cursor(abs);
                }
            }
            HomeItem::Day(d) => {
                self.home = false;
                self.open_cal(super::CalView::Day);
                if let Some(c) = self.calendar.as_mut() {
                    c.date = *d;
                }
            }
            HomeItem::Space(p) => self.sidebar_open(&super::NavItem::Space(p.clone())),
            HomeItem::Note(path) => {
                self.open_notes_screen();
                let pos = self.note_entries().iter().position(|e| e.path == *path);
                if let (Some(pos), Some(s)) = (pos, self.notes_screen.as_mut()) {
                    s.cursor = pos;
                }
            }
        }
    }

    /// `x` on a task in a Home tile: done (or not).
    pub fn home_toggle(&mut self) {
        let Some(HomeSel {
            tile,
            item: Some(i),
        }) = self.home_sel
        else {
            return;
        };
        if let Some(HomeItem::Task(abs)) = self.home_items(tile).into_iter().nth(i) {
            self.toggle_complete(abs);
            let n = self.home_items(tile).len();
            if let Some(s) = self.home_sel.as_mut() {
                s.item = if n == 0 { None } else { Some(i.min(n - 1)) };
            }
        }
    }

    /// `Esc` on Home: out of an item, then out of the tile.
    pub fn home_back(&mut self) -> bool {
        match self.home_sel {
            Some(HomeSel {
                item: Some(_),
                tile,
            }) => {
                self.home_sel = Some(HomeSel { tile, item: None });
                true
            }
            Some(_) => {
                self.home_sel = None;
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::app::test_support::build_app;

    #[test]
    fn home_sums_up_the_day() {
        // Today is 2026-05-06 (a wednesday) in tests.
        let mut app = build_app(concat!(
            "Teoría +Uni plan:2026-05-06 at:09:00\n",
            "x 2026-05-06 Gym +Health plan:2026-05-06\n",
            "Trabajo +Uni plan:2026-05-08\n",
            "llamar al dentista\n",
        ));
        app.open_home();
        let (done, total, open) = app.home_today();
        assert_eq!((done, total), (1, 2));
        assert_eq!(open, vec![0]);
        let week = app.home_week();
        assert_eq!(week.len(), 7);
        assert_eq!(week[0].count, 2, "today");
        assert_eq!(week[2].count, 1, "friday");
        assert_eq!(app.home_spaces(), vec![("Uni".to_string(), 2)]);
        assert_eq!(app.inbox(), vec![3]);

        app.open_inbox();
        assert!(!app.home);
        assert_eq!(app.visible_indices().len(), 1);
    }
}
