use std::fmt;
use std::str::FromStr;
use std::time::Duration;

pub const LEADER_WINDOW: Duration = Duration::from_millis(600);
pub const FLASH_TTL: Duration = Duration::from_millis(1400);
pub const UNDO_LIMIT: usize = 50;
pub const AUTOCOMPLETE_CAP: usize = 8;

/// Outcome of `add_from_draft`. The Enter handler in `main.rs` uses this to
/// decide whether to exit Insert mode: `Parsed` means the NL pre-pass
/// rewrote the buffer but did not save, so the user should stay in Insert
/// to review/edit before pressing Enter a second time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddOutcome {
    Saved,
    Parsed,
    Empty,
    Invalid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Insert,
    Search,
    Visual,
    Help,
    Settings,
    PromptProject,       // text input → add project on current task
    PromptContext,       // text input → add/remove context on current task
    PromptRenameProject, // text input → rename all project occurrences on current project in project list
    PromptRenameContext, // text input → rename all context occurrences on current context in context list
    PromptNewSpace,      // text input → a new space (`a` on a space in the sidebar: one inside it)
    PickProject,         // j/k cycles through projects to filter by
    PickContext,         // j/k cycles through contexts to filter by
    PickSavedFilter,     // j/k cycles through saved searches to apply
    SearchAll,           // Search: tasks, notes and spaces at once
    Filters,             // the "+ filter" popover
    PromptChecklist,     // text input → a new item on the task's checklist
    PromptSaveFilter,    // text input → name the current search and save it
    CommandPalette,
    /// QR + URL overlay for the in-TUI capture server. Any key
    /// dismisses; press `s` again to re-open without rebinding (the
    /// server stays running once started).
    Share,
    /// Theme picker dialog — j/k to preview themes, Enter to accept,
    /// Esc to revert.
    PickTheme,
    /// First-run welcome prompt, shown when `tasq` is launched with no
    /// target and no `./todo.txt` exists. `c` creates `./todo.txt`, `s`
    /// opens the bundled sample, `q`/`Esc` quits without creating anything.
    Welcome,
    /// Floating notes-list popup for the current task (`o`), styled like the
    /// "ADD TASK" dialog. Read-only for now: lists the task's `.md` files
    /// (`App::notes_popup` holds the list + cursor), navigable with j/k,
    /// closed with Esc. Selecting a file to actually open is wired in a
    /// later task.
    Notes,
    /// The shortcut menu (`space`): what you can do from here, with a key
    /// each; some open a submenu (see `App::menu_page`).
    Menu,
}

/// Which page of the shortcut menu is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MenuPage {
    #[default]
    Root,
    /// `g`: go to a place.
    Go,
    /// `t`: the focus timer.
    Timer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    List,
    Archive,
}

impl View {
    /// Stable slot index for keying per-view state arrays. Don't reorder the
    /// `View` variants without updating this together.
    pub fn idx(self) -> usize {
        match self {
            View::List => 0,
            View::Archive => 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sort {
    Priority,
    Due,
    File,
}

impl Sort {
    pub fn as_str(self) -> &'static str {
        match self {
            Sort::Priority => "priority",
            Sort::Due => "due",
            Sort::File => "file",
        }
    }
}

impl fmt::Display for Sort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Sort {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "priority" => Ok(Sort::Priority),
            "due" => Ok(Sort::Due),
            "file" => Ok(Sort::File),
            _ => Err(()),
        }
    }
}

/// Which tasks the list shows (DESIGN.md §2 "Visibility"). Dates never
/// hide a task on their own; the scope decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scope {
    /// Overdue, planned for today or earlier, due today or earlier.
    Today,
    /// Everything with a date after today, grouped by day; further than a
    /// week ahead under "Later".
    Upcoming,
    /// Every task.
    #[default]
    All,
}

impl Scope {
    pub fn as_str(self) -> &'static str {
        match self {
            Scope::Today => "today",
            Scope::Upcoming => "upcoming",
            Scope::All => "all",
        }
    }
}

impl fmt::Display for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Scope {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "today" => Ok(Scope::Today),
            "upcoming" => Ok(Scope::Upcoming),
            "all" => Ok(Scope::All),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Density {
    Compact,
    Comfortable,
    Cozy,
}

impl Density {
    pub fn as_str(self) -> &'static str {
        match self {
            Density::Compact => "compact",
            Density::Comfortable => "comfortable",
            Density::Cozy => "cozy",
        }
    }
}

impl fmt::Display for Density {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Density {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "compact" => Ok(Density::Compact),
            "comfortable" => Ok(Density::Comfortable),
            "cozy" => Ok(Density::Cozy),
            _ => Err(()),
        }
    }
}

/// The built-in filters in the sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    /// Priority A.
    HighPriority,
    Starred,
    /// Open tasks past their deadline.
    Overdue,
    /// Captures still to sort: no space, no date, no repeat.
    Inbox,
}

impl Preset {
    /// The built-in filters the sidebar lists under FILTERS.
    pub const ALL: [Preset; 3] = [Preset::HighPriority, Preset::Starred, Preset::Overdue];
    /// Every built-in filter, the inbox too.
    pub const EVERY: [Preset; 4] = [
        Preset::HighPriority,
        Preset::Starred,
        Preset::Overdue,
        Preset::Inbox,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Preset::HighPriority => "High priority",
            Preset::Starred => "Starred",
            Preset::Overdue => "Overdue",
            Preset::Inbox => "Inbox",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Preset::HighPriority => "⚑",
            Preset::Starred => "★",
            Preset::Overdue => "◷",
            Preset::Inbox => "▤",
        }
    }

    /// The word after `is:` in a saved view's query.
    pub fn key(self) -> &'static str {
        match self {
            Preset::HighPriority => "high",
            Preset::Starred => "starred",
            Preset::Overdue => "overdue",
            Preset::Inbox => "inbox",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Filter {
    pub project: Option<String>,
    pub context: Option<String>,
    pub search: String,
    /// A built-in filter from the sidebar.
    pub preset: Option<Preset>,
}

impl Filter {
    /// True when at least one of project / context / search / preset is set.
    pub fn has_any(&self) -> bool {
        self.project.is_some()
            || self.context.is_some()
            || !self.search.is_empty()
            || self.preset.is_some()
    }

    /// The active `+project` / `@context` tags as an add-prompt prefix, with
    /// a trailing space; empty when neither is set. A task added under a
    /// filter that doesn't carry its tags drops out of the view the moment it
    /// saves. `search` contributes nothing — it is a needle, not a tag.
    pub fn tag_seed(&self) -> String {
        let project = self.project.as_deref().map(|p| format!("+{p} "));
        let context = self.context.as_deref().map(|c| format!("@{c} "));
        project.unwrap_or_default() + &context.unwrap_or_default()
    }

    /// Drop every filter component back to its empty state.
    pub fn clear(&mut self) {
        self.project = None;
        self.context = None;
        self.search.clear();
        self.preset = None;
    }

    /// The filter as a saved view's query: `+space @tag is:starred text`.
    pub fn to_query(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(p) = &self.project {
            parts.push(format!("+{p}"));
        }
        if let Some(c) = &self.context {
            parts.push(format!("@{c}"));
        }
        if let Some(p) = self.preset {
            parts.push(format!("is:{}", p.key()));
        }
        if !self.search.trim().is_empty() {
            parts.push(self.search.trim().to_string());
        }
        parts.join(" ")
    }

    /// Read a saved view's query back: `+x` is a space, `@x` a tag,
    /// `is:high|starred|overdue` a built-in filter, the rest a search.
    pub fn from_query(q: &str) -> Self {
        let mut f = Self::default();
        let mut rest: Vec<&str> = Vec::new();
        for word in q.split_whitespace() {
            if let Some(p) = word.strip_prefix('+').filter(|p| !p.is_empty())
                && f.project.is_none()
            {
                f.project = Some(p.to_string());
            } else if let Some(c) = word.strip_prefix('@').filter(|c| !c.is_empty())
                && f.context.is_none()
            {
                f.context = Some(c.to_string());
            } else if let Some(p) = word
                .strip_prefix("is:")
                .and_then(|k| Preset::EVERY.into_iter().find(|p| p.key() == k))
            {
                f.preset = Some(p);
            } else {
                rest.push(word);
            }
        }
        f.search = rest.join(" ");
        f
    }

    /// Same filter, compared by what it shows.
    pub fn same_as(&self, other: &Filter) -> bool {
        self.to_query() == other.to_query()
    }
}

/// A user-named saved search. `query` is a `/`-search needle (case-insensitive
/// subsequence match on the task body), recalled via the `ff` picker and
/// persisted as a `filter.<name> = <query>` line in the config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedFilter {
    pub name: String,
    pub query: String,
}

#[cfg(test)]
mod tests {
    use super::Filter;

    #[test]
    fn tag_seed_is_empty_without_project_or_context() {
        let filter = Filter {
            search: "milk".to_string(),
            ..Filter::default()
        };
        assert_eq!(filter.tag_seed(), "", "a search needle is not a tag");
    }

    #[test]
    fn tag_seed_leads_with_project_then_context() {
        let filter = Filter {
            project: Some("work".to_string()),
            context: Some("home".to_string()),
            search: String::new(),
            ..Default::default()
        };
        // Trailing space: the seed is a prefix the body gets typed after.
        assert_eq!(filter.tag_seed(), "+work @home ");
    }

    #[test]
    fn tag_seed_covers_a_single_active_filter() {
        let filter = Filter {
            context: Some("home".to_string()),
            ..Filter::default()
        };
        assert_eq!(filter.tag_seed(), "@home ");
    }
}
