//! Settings and your profile (`,`): sections on the left, their settings
//! as cards on the right. `↑`/`↓` pick a section, `→` or `Enter` go into
//! it, `Enter` (or `space`) changes the setting under the cursor, and it's
//! saved to the config at once.

use super::App;
use super::types::Mode;
use crate::action::Action;
use crate::app::Density;

/// The sections, in order.
pub const SECTIONS: [&str; 7] = [
    "Sync & calendars",
    "Appearance",
    "Lists",
    "Capture",
    "Data & trash",
    "Keys",
    "About",
];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SettingsState {
    pub section: usize,
    pub row: usize,
    /// The cursor is in the section's settings, not on the list of sections.
    pub in_rows: bool,
}

/// What a setting does when you press `Enter` on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetKey {
    Theme,
    Density,
    Icons,
    Hints,
    LineNum,
    Sidebar,
    Details,
    StatusBar,
    StartOn,
    ShowDone,
    ShowFuture,
    Sort,
    WeekStart,
    RecBuilder,
    ChecklistCompletes,
    PhoneCapture,
    Trash,
    Help,
}

/// One setting: a name, its value, and whether `Enter` changes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetRow {
    /// The card it sits in.
    pub card: &'static str,
    pub label: &'static str,
    pub value: String,
    /// `None` for a line that's just information.
    pub key: Option<SetKey>,
    /// Shown dimmed: not there yet.
    pub soon: bool,
}

fn on(b: bool) -> String {
    if b { "on" } else { "off" }.to_string()
}

impl App {
    pub fn open_settings(&mut self) {
        self.settings = SettingsState::default();
        self.mode = Mode::Settings;
    }

    /// The rows of section `s`.
    pub fn settings_rows(&self, s: usize) -> Vec<SetRow> {
        let row = |card, label, value: String, key| SetRow {
            card,
            label,
            value,
            key,
            soon: false,
        };
        let soon = |card, label, value: &str| SetRow {
            card,
            label,
            value: value.to_string(),
            key: None,
            soon: true,
        };
        let p = &self.prefs;
        match s {
            0 => vec![
                soon("Sync", "Sync", "end-to-end encrypted sync · coming soon"),
                row(
                    "Sync",
                    "This device",
                    if self.is_db() {
                        "● local database".to_string()
                    } else {
                        "● plain todo.txt".to_string()
                    },
                    None,
                ),
                soon("Connected calendars", "Google Calendar", "coming soon"),
                soon("Connected calendars", "Apple Calendar", "coming soon"),
                soon("Connected calendars", "Local .ics feed", "coming soon"),
            ],
            1 => vec![
                row(
                    "Look",
                    "Theme",
                    self.theme().name.to_string(),
                    Some(SetKey::Theme),
                ),
                row(
                    "Look",
                    "Density",
                    match p.density {
                        Density::Compact => "compact",
                        Density::Comfortable => "comfortable",
                        Density::Cozy => "cozy",
                    }
                    .to_string(),
                    Some(SetKey::Density),
                ),
                row(
                    "Look",
                    "Icons",
                    if p.nerd_icons { "nerd font" } else { "unicode" }.to_string(),
                    Some(SetKey::Icons),
                ),
                row(
                    "Panels",
                    "Sidebar",
                    on(p.layout.left),
                    Some(SetKey::Sidebar),
                ),
                row(
                    "Panels",
                    "Details",
                    on(p.layout.right),
                    Some(SetKey::Details),
                ),
                row(
                    "Panels",
                    "Status line",
                    on(p.layout.status_bar),
                    Some(SetKey::StatusBar),
                ),
                row("Panels", "Hints", on(p.hints), Some(SetKey::Hints)),
                row(
                    "Panels",
                    "Line numbers",
                    on(p.layout.line_num),
                    Some(SetKey::LineNum),
                ),
            ],
            2 => vec![
                row(
                    "Lists",
                    "Open on",
                    if p.start_home { "home" } else { "the list" }.to_string(),
                    Some(SetKey::StartOn),
                ),
                row(
                    "Lists",
                    "Sort",
                    self.sort_label().to_string(),
                    Some(SetKey::Sort),
                ),
                row(
                    "Lists",
                    "Show done",
                    on(p.show_done),
                    Some(SetKey::ShowDone),
                ),
                row(
                    "Lists",
                    "Show future",
                    on(p.show_future),
                    Some(SetKey::ShowFuture),
                ),
                row(
                    "Lists",
                    "Week starts on",
                    format!("{:?}", p.week_start).to_lowercase(),
                    Some(SetKey::WeekStart),
                ),
            ],
            3 => vec![
                row(
                    "Capture",
                    "Natural language",
                    "type prose: \"gym every mon 7am in personal\"".to_string(),
                    None,
                ),
                row(
                    "Capture",
                    "Checklist → done",
                    on(p.checklist_completes),
                    Some(SetKey::ChecklistCompletes),
                ),
                row(
                    "Capture",
                    "Repeat builder",
                    on(p.recurrence_builder),
                    Some(SetKey::RecBuilder),
                ),
                row(
                    "Capture",
                    "Phone capture",
                    "show the QR".to_string(),
                    Some(SetKey::PhoneCapture),
                ),
                soon(
                    "Notifications",
                    "Reminders",
                    "desktop notifications · coming soon",
                ),
            ],
            4 => vec![
                row(
                    "Data",
                    if self.is_db() {
                        "Database"
                    } else {
                        "Todo file"
                    },
                    self.file_path.display().to_string(),
                    None,
                ),
                row(
                    "Data",
                    "Notes",
                    self.notes_dir().display().to_string(),
                    None,
                ),
                row(
                    "Data",
                    "Config",
                    self.config_path
                        .as_ref()
                        .map_or_else(|| "(unavailable)".to_string(), |p| p.display().to_string()),
                    None,
                ),
                row("Data", "Export", "tasq export > todo.txt".to_string(), None),
                row(
                    "Trash",
                    "Trash",
                    format!(
                        "{} deleted · kept {} days · open",
                        self.store.trash().len(),
                        crate::core::KEEP_DAYS
                    ),
                    Some(SetKey::Trash),
                ),
            ],
            5 => vec![
                row(
                    "Keys",
                    "Every key",
                    "open the help".to_string(),
                    Some(SetKey::Help),
                ),
                row(
                    "Keys",
                    "The menu",
                    "␣ shows what you can do".to_string(),
                    None,
                ),
                row(
                    "Keys",
                    "Your own",
                    "[keys] in config.toml".to_string(),
                    None,
                ),
            ],
            _ => vec![
                row("About", "Version", self.version_label.clone(), None),
                row(
                    "About",
                    "Updates",
                    self.update_available().map_or_else(
                        || "up to date".to_string(),
                        |t| format!("{t} · tasq update"),
                    ),
                    None,
                ),
                row(
                    "About",
                    "Home page",
                    "github.com/tasq-app/tasq".to_string(),
                    None,
                ),
            ],
        }
    }

    pub fn settings_move(&mut self, forward: bool) {
        let s = &mut self.settings;
        if s.in_rows {
            let n = self.settings_rows(self.settings.section).len();
            let s = &mut self.settings;
            s.row = if forward {
                (s.row + 1).min(n.saturating_sub(1))
            } else {
                s.row.saturating_sub(1)
            };
        } else {
            s.section = if forward {
                (s.section + 1).min(SECTIONS.len() - 1)
            } else {
                s.section.saturating_sub(1)
            };
            s.row = 0;
        }
    }

    /// `→`: into the section's settings.
    pub fn settings_enter_rows(&mut self) {
        self.settings.in_rows = true;
        self.settings.row = 0;
    }

    /// `←` / `Esc` in the settings: back to the sections.
    pub fn settings_leave_rows(&mut self) {
        self.settings.in_rows = false;
    }

    /// `Enter` on a setting: change it. Some changes are actions the
    /// caller applies; the rest happen here and are saved.
    pub fn settings_activate(&mut self) -> Option<Action> {
        if !self.settings.in_rows {
            self.settings_enter_rows();
            return None;
        }
        let key = self
            .settings_rows(self.settings.section)
            .get(self.settings.row)?
            .key?;
        let action = match key {
            SetKey::Theme => Some(Action::CycleTheme),
            SetKey::Density => Some(Action::CycleDensity),
            SetKey::LineNum => Some(Action::ToggleLineNum),
            SetKey::Sidebar => Some(Action::ToggleLeftPane),
            SetKey::Details => Some(Action::ToggleRightPane),
            SetKey::ShowDone => Some(Action::ToggleShowDone),
            SetKey::ShowFuture => Some(Action::ToggleShowFuture),
            SetKey::Sort => Some(Action::CycleSort),
            SetKey::Help => Some(Action::OpenHelp),
            SetKey::PhoneCapture => Some(Action::OpenShare),
            SetKey::Icons => {
                self.prefs.nerd_icons = !self.prefs.nerd_icons;
                None
            }
            SetKey::Hints => {
                self.prefs.hints = !self.prefs.hints;
                None
            }
            SetKey::StatusBar => {
                self.prefs.layout.status_bar = !self.prefs.layout.status_bar;
                None
            }
            SetKey::StartOn => {
                self.prefs.start_home = !self.prefs.start_home;
                None
            }
            SetKey::ChecklistCompletes => {
                self.prefs.checklist_completes = !self.prefs.checklist_completes;
                None
            }
            SetKey::RecBuilder => {
                self.prefs.recurrence_builder = !self.prefs.recurrence_builder;
                None
            }
            SetKey::WeekStart => {
                self.prefs.cycle_week_start();
                self.week_start = self.prefs.week_start;
                self.recompute_visible();
                None
            }
            SetKey::Trash => {
                self.mode = Mode::Normal;
                self.open_trash();
                return None;
            }
        };
        if action.is_none() {
            self.save_prefs();
        }
        action
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_support::build_app;

    #[test]
    fn settings_walk_sections_and_change_things() {
        let mut app = build_app("a\n");
        app.open_settings();
        // Every section has something in it.
        for (s, name) in SECTIONS.iter().enumerate() {
            assert!(!app.settings_rows(s).is_empty(), "{name}");
        }
        app.settings_move(true); // Appearance
        assert_eq!(app.settings_activate(), None); // into the rows
        assert!(app.settings.in_rows);
        assert_eq!(app.settings_activate(), Some(Action::CycleTheme));
        let hints = app.prefs.hints;
        app.settings.row = app
            .settings_rows(1)
            .iter()
            .position(|r| r.key == Some(SetKey::Hints))
            .unwrap_or(0);
        app.settings_activate();
        assert_eq!(app.prefs.hints, !hints);
    }
}
