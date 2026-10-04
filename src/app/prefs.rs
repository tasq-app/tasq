use std::io;

use super::types::{Density, Scope, Sort};
use crate::app::WeekStart;
use crate::config::Config;
use crate::theme::{self, Theme};

#[derive(Debug, Clone)]
pub struct Layout {
    pub left: bool,
    pub right: bool,
    pub line_num: bool,
    pub status_bar: bool,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            left: true,
            right: true,
            line_num: false,
            status_bar: true,
        }
    }
}

/// User-tunable preferences persisted to `Config`. Cycle/toggle methods return
/// the flash message for the caller to display, sidestepping any `&mut prefs`
/// + `&mut flash_state` borrow tangle on `App`.
#[derive(Debug, Clone)]
pub struct Prefs {
    theme_idx: usize,
    pub density: Density,
    pub sort: Sort,
    /// Today / Upcoming / All (see [`Scope`]).
    pub scope: Scope,
    pub layout: Layout,
    pub show_done: bool,
    pub show_future: bool,
    /// Metadata keys whose `key:value` tokens are hidden from task rows.
    /// Config-only (no in-app toggle); see `Config::hidden_keys`.
    pub hidden_keys: Vec<String>,
    pub week_start: WeekStart,
    /// Whether the create/edit dialog opens the recurrence builder overlay
    /// for `rec:` / `/rec`. Config-only (no in-app toggle); see
    /// `Config::recurrence_builder`.
    pub recurrence_builder: bool,
    /// Nerd Font icons instead of plain Unicode (`icons = nerd`).
    pub nerd_icons: bool,
    /// Short hints for what's selected in the bottom line (`hints`).
    pub hints: bool,
    /// Open on Home (`start = "home"`, the default) or the list.
    pub start_home: bool,
    /// Width of the details pane, in columns (`{` / `}` or drag its edge).
    pub details_w: u16,
    /// Ticking a checklist's last box marks its task done.
    pub checklist_completes: bool,
}

/// The details pane's width: by default, and how narrow or wide it goes.
pub const DETAILS_W: u16 = 34;
pub const DETAILS_MIN: u16 = 24;
pub const DETAILS_MAX: u16 = 100;

/// The look this build saves its config under.
const DESIGN: u32 = 5;

impl Prefs {
    pub fn from_config(mut cfg: Config) -> Self {
        // A config saved before the redesign holds the old defaults, not
        // choices: move them to the new look once.
        if cfg.design.unwrap_or(0) < DESIGN {
            if cfg.theme.as_deref() == Some("Muted Slate") {
                cfg.theme = None;
            }
            cfg.show_line_num = None;
            cfg.start = None;
        }
        let theme_idx = cfg
            .theme
            .as_deref()
            .and_then(|name| theme::all().iter().position(|t| t.name == name))
            .unwrap_or(0);
        Self {
            theme_idx,
            density: cfg.density.unwrap_or(Density::Comfortable),
            sort: cfg.sort.unwrap_or(Sort::Priority),
            scope: cfg.view.unwrap_or(Scope::Today),
            layout: Layout {
                left: cfg.show_left.unwrap_or(true),
                right: cfg.show_right.unwrap_or(true),
                line_num: cfg.show_line_num.unwrap_or(false),
                status_bar: cfg.show_status_bar.unwrap_or(true),
            },
            show_done: cfg.show_done.unwrap_or(false),
            show_future: cfg.show_future.unwrap_or(false),
            hidden_keys: cfg.hidden_keys,
            week_start: cfg.week_start.unwrap_or(WeekStart::Sunday),
            recurrence_builder: cfg.recurrence_builder.unwrap_or(true),
            nerd_icons: cfg.icons.as_deref() == Some("nerd"),
            hints: cfg.hints.unwrap_or(true),
            start_home: cfg.start.as_deref() == Some("home"),
            checklist_completes: cfg.checklist_completes.unwrap_or(true),
            details_w: cfg
                .details_width
                .unwrap_or(DETAILS_W)
                .clamp(DETAILS_MIN, DETAILS_MAX),
        }
    }

    pub fn theme(&self) -> &'static Theme {
        let all = theme::all();
        all[self.theme_idx % all.len()]
    }

    pub fn theme_idx(&self) -> usize {
        self.theme_idx
    }

    /// Jump directly to a specific theme by index. Used by the screenshot
    /// example to render every theme; production code should call
    /// `cycle_theme` instead so the change persists with a flash message.
    pub fn set_theme_idx(&mut self, idx: usize) {
        self.theme_idx = idx % theme::all().len();
    }

    pub fn sort_label(&self) -> &'static str {
        self.sort.as_str()
    }

    pub fn cycle_theme(&mut self) -> String {
        self.theme_idx = (self.theme_idx + 1) % theme::all().len();
        format!("theme: {}", self.theme().name)
    }

    pub fn cycle_density(&mut self) -> String {
        self.density = match self.density {
            Density::Compact => Density::Comfortable,
            Density::Comfortable => Density::Cozy,
            Density::Cozy => Density::Compact,
        };
        format!("density: {}", self.density)
    }

    pub fn cycle_sort(&mut self) -> String {
        self.sort = match self.sort {
            Sort::Priority => Sort::Due,
            Sort::Due => Sort::File,
            Sort::File => Sort::Priority,
        };
        format!("sort: {}", self.sort)
    }

    pub fn toggle_left(&mut self) {
        self.layout.left = !self.layout.left;
    }

    pub fn toggle_right(&mut self) {
        self.layout.right = !self.layout.right;
    }

    pub fn toggle_line_num(&mut self) {
        self.layout.line_num = !self.layout.line_num;
    }

    pub fn toggle_show_done(&mut self) {
        self.show_done = !self.show_done;
    }

    pub fn toggle_show_future(&mut self) {
        self.show_future = !self.show_future;
    }

    pub fn cycle_week_start(&mut self) -> String {
        self.week_start = match self.week_start {
            WeekStart::Sunday => WeekStart::Monday,
            WeekStart::Monday => WeekStart::Sunday,
        };
        format!("week_start: {}", self.week_start)
    }

    /// Persist to the XDG config path. Returns the IO error so the caller
    /// can flash it (writing to stderr from inside the alt-screen would
    /// corrupt the TUI). Saving is best-effort — callers that don't care
    /// about reporting can `let _ = prefs.save();`.
    ///
    /// Loads the on-disk config first so non-pref fields (like
    /// `share_token` / `share_port`, owned by the capture server) are
    /// preserved across pref toggles.
    pub fn save(&self) -> io::Result<()> {
        let mut cfg = Config::load();
        cfg.theme = Some(self.theme().name.to_string());
        cfg.density = Some(self.density);
        cfg.sort = Some(self.sort);
        cfg.view = Some(self.scope);
        cfg.show_left = Some(self.layout.left);
        cfg.show_right = Some(self.layout.right);
        cfg.show_line_num = Some(self.layout.line_num);
        cfg.show_status_bar = Some(self.layout.status_bar);
        cfg.show_done = Some(self.show_done);
        cfg.show_future = Some(self.show_future);
        cfg.hidden_keys = self.hidden_keys.clone();
        cfg.week_start = Some(self.week_start);
        cfg.recurrence_builder = Some(self.recurrence_builder);
        cfg.icons = Some(if self.nerd_icons { "nerd" } else { "unicode" }.to_string());
        cfg.hints = Some(self.hints);
        cfg.design = Some(DESIGN);
        cfg.details_width = Some(self.details_w);
        cfg.checklist_completes = Some(self.checklist_completes);
        cfg.start = Some(if self.start_home { "home" } else { "list" }.to_string());
        cfg.save()
    }
}
