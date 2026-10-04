use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::{App, DialogInputMode, Mode, NoteEditorMode, View};
use crate::ui::dialog::draft_cursor_spans;
use crate::ui::note_editor;

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let theme = app.theme();
    let mut mode_label: std::borrow::Cow<'static, str> = match app.mode {
        Mode::Normal => "NORMAL".into(),
        Mode::Insert => match app.draft.input_mode() {
            DialogInputMode::Normal => "NORMAL",
            DialogInputMode::Insert => "INSERT",
        }
        .into(),
        Mode::Search => "SEARCH".into(),
        Mode::Visual => "VISUAL".into(),
        Mode::Help => "HELP".into(),
        Mode::Settings => "SETTINGS".into(),
        Mode::PromptProject => "PROJECT".into(),
        Mode::PromptContext => "CONTEXT".into(),
        Mode::PickProject => "PICK SPACE".into(),
        Mode::PickContext => "PICK @CONTEXT".into(),
        Mode::PromptRenameProject => "RENAME +PROJECT".into(),
        Mode::PromptRenameContext => "RENAME @CONTEXT".into(),
        Mode::PickSavedFilter => "PICK FILTER".into(),
        Mode::PromptSaveFilter => "SAVE FILTER".into(),
        Mode::PromptChecklist => "CHECKLIST".into(),
        Mode::CommandPalette => "COMMAND".into(),
        Mode::Share => "SHARE".into(),
        Mode::PickTheme => "PICK THEME".into(),
        Mode::Welcome => "WELCOME".into(),
        // A note editor's own sub-mode overrides this just below.
        Mode::Notes => "NOTES".into(),
        Mode::Menu => "MENU".into(),
        Mode::Filters => "FILTER".into(),
    };
    // The focused note editor's own sub-mode wins over `app.mode`: a pinned
    // note keeps `app.mode == Mode::Normal` while it has focus, which used
    // to leave the chip stuck on NORMAL even while typing in Insert.
    let editor_mode = app.focused_note_editor().map(|e| e.mode());
    if let Some(editor) = app.focused_note_editor() {
        mode_label = match editor.pending_keys() {
            Some(keys) => format!("{} {keys}…", note_editor::mode_label(editor.mode())).into(),
            None => note_editor::mode_label(editor.mode()).into(),
        };
    }
    if matches!(app.view, View::Archive) {
        mode_label = "ARCHIVE".into();
    }
    if app.mode == Mode::Normal
        && let Some(cal) = &app.calendar
    {
        mode_label = cal.view.label().into();
    }

    // T11: while the pinned note has keyboard focus, `app.mode` stays
    // `Mode::Normal` (that's the point — see `src/app/pinned_note.rs`), so
    // it can't drive the hint on its own; check `pinned_focus` ahead of the
    // per-mode match instead of trying to fold it into that match's arms.
    let mut hint: std::borrow::Cow<'static, str> = if let Some(editor) = app.focused_note_editor() {
        let tail = if !app.pinned_focus {
            "z pin · Esc back to list"
        } else if app.pinned_notes.len() > 1 {
            "Tab/S-Tab switch tab · z unfocus · Z close tab"
        } else {
            "z unfocus · Z close pinned"
        };
        note_editor_hint(editor.mode(), tail).into()
    } else if app.pinned_focus {
        "z unfocus · Z close pinned".into()
    } else if app.mode == Mode::Normal
        && let Some(cal) = &app.calendar
    {
        match cal.view {
            crate::app::CalView::Day => {
                "↑↓ task · ←→ day · Enter edit · x done · n new · J/K move · Esc list"
            }
            crate::app::CalView::Week => {
                "←→ day · ↑↓ task · < > week · v blocks/agenda · Enter edit · n new · Esc list"
            }
            crate::app::CalView::Month => {
                "←→↑↓ day · < > month · v counts/titles · Enter day · n new · Esc list"
            }
        }
        .into()
    } else {
        match app.mode {
            Mode::Insert => match app.draft.input_mode() {
                DialogInputMode::Normal => {
                    "h/l navigate · w/b/e word · i/a insert · Enter save · Esc cancel"
                }
                DialogInputMode::Insert if app.live_chip_focus().is_some() => {
                    "←/→ chips · Enter pick · x reject · Esc back to text"
                }
                DialogInputMode::Insert if app.live_add_active() => {
                    "type naturally: tomorrow at 6pm, every fri and sat, +project @context · Tab chips · Ctrl+Z undo · Enter add"
                }
                DialogInputMode::Insert => "Enter save · Esc normal",
            },
            Mode::Visual => "space toggle · x complete · dd delete · Esc cancel",
            Mode::Help => "Tab tasks ⇄ notes page · ? close help",
            Mode::Settings => "Esc back",
            Mode::PromptProject => "type +project name · Enter save · Esc cancel",
            Mode::PromptContext => "type @context name · Enter toggle · Esc cancel",
            Mode::PickProject => "j/k or ↑↓ cycle spaces · c colour · h hide/show · r rename · d delete if empty · Enter keep · Esc clear",
            Mode::PickContext => "j/k or ↑↓ cycle contexts · r rename · Enter keep · Esc clear",
            Mode::PickSavedFilter => "j/k or ↑↓ cycle filters · Enter keep · Esc revert",
            Mode::PromptSaveFilter => "type a filter name · Enter save · Esc cancel",
            Mode::PromptChecklist => "Enter add · Esc done",
            Mode::CommandPalette => "type to filter · Enter run · Esc cancel",
            Mode::Share => "scan the QR · any key dismisses",
            Mode::Welcome => "c create ./todo.txt · s open sample · q quit",
            // With an editor open, the focused-editor branch above wins.
            Mode::Menu => "press a key · Esc close",
            Mode::Filters => "type to search · ↑↓ move · Enter add/remove · ⌫ drop last · Esc close",
            Mode::Notes => {
                "j/k navigate · e/i edit · p preview · z zoom · n new · r rename · d delete · u unlink · ? help · Esc close"
            }
            _ => {
                "j/k · n new · r reschedule · x done · * star · o notes · z pin · / search · ? help · u undo · q quit"
            }
        }
        .into()
    };
    // A note pinned-but-unfocused still needs `Z` surfaced somewhere — it's
    // not covered by any per-mode arm above since the user could be in
    // almost any mode while it sits docked in the background.
    if !app.pinned_focus && !app.pinned_notes.is_empty() {
        hint = format!("{hint} · Z close pinned").into();
    }

    // In the list, the hint is short and about what's selected — or
    // nothing, with `hints = false`. The menu (`␣`) has the rest.
    let list_normal = app.mode == Mode::Normal
        && app.calendar.is_none()
        && editor_mode.is_none()
        && !app.pinned_focus
        && !app.sidebar_focus
        && !app.inspector_focus;
    if list_normal {
        hint = if !app.prefs.hints {
            "".into()
        } else if app.cur_abs().is_some() && app.filter.preset == Some(crate::app::Preset::Inbox) {
            "+ space · r date · x done · e edit".into()
        } else if app.cur_abs().is_some() {
            "x done · e edit · r reschedule · Tab details".into()
        } else {
            "n new task · Tab sidebar".into()
        };
    }
    if app.home && app.mode == Mode::Normal && !app.sidebar_focus {
        hint = if app.prefs.hints {
            match app.home_sel {
                Some(crate::app::HomeSel { tile: 0, .. }) => "Enter add a task · Tab next".into(),
                Some(crate::app::HomeSel { item: Some(_), .. }) => {
                    "Enter open · x done · ↑↓ move · Esc back".into()
                }
                Some(_) => "Enter open · ←→↑↓ tiles · Tab next · Esc back".into(),
                None => "n new task · Tab pick a tile · i inbox · Enter today".into(),
            }
        } else {
            "".into()
        };
        mode_label = "HOME".into();
    }
    if let Some(ns) = app
        .notes_screen
        .as_ref()
        .filter(|s| app.mode == Mode::Normal && s.editor.is_none())
        && !app.sidebar_focus
    {
        hint = if ns.searching {
            "type to search · Enter done · Esc clear".into()
        } else if app.prefs.hints {
            "Enter edit · / search · n next hit · E $EDITOR · p pin · t its task".into()
        } else {
            "".into()
        };
        mode_label = "NOTES".into();
    }
    if app.trash_screen.is_some() && app.mode == Mode::Normal && !app.sidebar_focus {
        hint = if app.prefs.hints {
            "r restore · D delete for good · E empty · Esc list".into()
        } else {
            "".into()
        };
        mode_label = "TRASH".into();
    }
    if app.inspector_focus && app.mode == Mode::Normal {
        hint = if !app.prefs.hints {
            "".into()
        } else {
            match app.inspector_current() {
                Some(crate::app::InspectorRow::Item(_)) => {
                    "x check · a add · d remove · Tab sidebar · Esc list"
                }
                Some(crate::app::InspectorRow::Note(_)) => {
                    "Enter open · a add item · Tab sidebar · Esc list"
                }
                _ => "Enter add item · Tab sidebar · Esc list",
            }
            .into()
        };
        mode_label = "DETAILS".into();
    }
    if app.sidebar_focus && app.mode == Mode::Normal {
        hint = if !app.prefs.hints {
            "".into()
        } else if matches!(app.sidebar_current(), Some(crate::app::NavItem::Space(_))) {
            "Enter open · c colour · H hide · r rename · d delete · Tab back".into()
        } else {
            "↑↓ move · Enter open · Tab back".into()
        };
        mode_label = "SIDEBAR".into();
    }

    // Left: the mode as a calm, rounded pill (coloured only while you type
    // or select), then where you are, quietly.
    let bg = Style::default().bg(theme.bg);
    let loud =
        editor_mode.is_some() || matches!(app.mode, Mode::Insert | Mode::Visual | Mode::Search);
    let (pill_bg, pill_fg) = if loud {
        (
            editor_mode.map_or(theme.mode_bg, |m| note_editor::mode_color(theme, m)),
            theme.mode_fg,
        )
    } else {
        (theme.cursor, theme.status_fg)
    };
    let chord_suffix = app
        .chord
        .active()
        .map(|c| format!(" {c}…"))
        .unwrap_or_default();
    let label = format!("● {}{chord_suffix}", mode_label.to_lowercase());
    let nerd = app.prefs.nerd_icons;
    let (cap_l, cap_r) = if nerd {
        ("\u{e0b6}", "\u{e0b4}")
    } else {
        (" ", " ")
    };
    let mut pill_style = Style::default().bg(pill_bg).fg(pill_fg);
    if loud {
        pill_style = pill_style.add_modifier(Modifier::BOLD);
    }
    let cap_style = if nerd { bg.fg(pill_bg) } else { pill_style };
    let mut left: Vec<Span> = vec![
        Span::styled(" ", bg),
        Span::styled(cap_l, cap_style),
        Span::styled(label, pill_style),
        Span::styled(cap_r, cap_style),
    ];
    if !app.selection.is_empty() {
        left.push(Span::styled(
            format!("  {} selected", app.selection.len()),
            bg.fg(theme.accent),
        ));
    } else if app.mode == Mode::Normal
        && !app.home
        && app.calendar.is_none()
        && app.notes_screen.is_none()
        && app.trash_screen.is_none()
        && editor_mode.is_none()
    {
        left.push(Span::styled(
            format!("  {}", where_you_are(app)),
            bg.fg(theme.dim),
        ));
    }

    // Right: the hints, each key a shade brighter than its word, then the
    // way into the menu.
    let mut right: Vec<Span> = Vec::new();
    if let Some(tag) = app.update_available() {
        right.push(Span::styled(
            format!("↑ {tag} · tasq update   "),
            bg.fg(theme.accent).add_modifier(Modifier::BOLD),
        ));
    }
    let key = bg.fg(theme.status_fg).add_modifier(Modifier::BOLD);
    let word = bg.fg(theme.dim);
    for part in hint.split(" · ").filter(|p| !p.is_empty()) {
        let (k, w) = part.split_once(' ').unwrap_or((part, ""));
        right.push(Span::styled(k.to_string(), key));
        right.push(Span::styled(format!(" {w}   "), word));
    }
    if app.mode == Mode::Normal {
        right.push(Span::styled("␣", key));
        let more = if hint.is_empty() {
            " menu "
        } else {
            " more… "
        };
        right.push(Span::styled(more, word));
    }
    let left_w: u16 = left.iter().map(|s| s.content.chars().count() as u16).sum();
    let right_w: u16 = right
        .iter()
        .map(|s| s.content.chars().count() as u16)
        .sum::<u16>()
        .min(area.width.saturating_sub(left_w + 1));
    let [left_area, right_area] =
        Layout::horizontal([Constraint::Min(1), Constraint::Length(right_w)]).areas(area);
    frame.render_widget(Paragraph::new(Line::from(left)).style(bg), left_area);
    frame.render_widget(
        Paragraph::new(Line::from(right)).style(bg).right_aligned(),
        right_area,
    );
}

/// Where the list is, in a few words: `Today · 4 tasks`, `Uni › AII · 3
/// tasks`.
fn where_you_are(app: &App) -> String {
    use crate::app::Scope;
    let n = app.visible_indices().len();
    let place = if let Some(p) = &app.filter.project {
        crate::core::spaces::display(p)
    } else if let Some(p) = app.filter.preset {
        p.label().to_string()
    } else {
        match app.prefs.scope {
            Scope::Today => "Today",
            Scope::Upcoming => "Upcoming",
            Scope::All => "All tasks",
        }
        .to_string()
    };
    format!("{place} · {n} {}", if n == 1 { "task" } else { "tasks" })
}

/// The hint line for a focused note editor in `mode`, ending in the
/// context-specific `tail` (popup vs. pinned tab keys).
fn note_editor_hint(mode: NoteEditorMode, tail: &str) -> String {
    let body = match mode {
        NoteEditorMode::Normal => {
            "hjkl w b e 0 $ gg G move · i a o insert · v V visual · d c y p · u undo · Enter tick [ ] · M preview · E $EDITOR · : cmd"
        }
        NoteEditorMode::Insert => {
            "type to edit · Enter continues lists · Tab/S-Tab nest · Ctrl+S save · Esc normal"
        }
        NoteEditorMode::Visual | NoteEditorMode::VisualLine => {
            "motions extend · d delete · y yank · c change · > < indent · o other end · Esc cancel"
        }
        NoteEditorMode::Preview => {
            "j/k scroll · space/b page · Ctrl-d/u half · gg G · p edit · i insert · E $EDITOR"
        }
    };
    if matches!(mode, NoteEditorMode::Normal | NoteEditorMode::Preview) {
        format!("{body} · {tail}")
    } else {
        body.to_string()
    }
}

pub fn render_command_line(frame: &mut Frame, area: Rect, app: &App) {
    let theme = app.theme();
    let visible_count = app.visible_indices().len();
    let suggestion = format!("  {visible_count} matches · Enter accept · Esc cancel");
    let mut spans = vec![
        Span::raw(" "),
        Span::styled(
            "/",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
    ];
    spans.extend(draft_cursor_spans(
        app.draft.text(),
        app.draft.cursor(),
        theme.fg,
        theme.bg,
    ));
    spans.push(Span::styled(suggestion, Style::default().fg(theme.dim)));
    let line = Line::from(spans).style(Style::default().bg(theme.bg));
    frame.render_widget(
        Paragraph::new(line).style(Style::default().bg(theme.bg)),
        area,
    );
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use crate::app::App;
    use crate::config::Config;

    fn build_app() -> App {
        let path =
            std::env::temp_dir().join(format!("tasq-status-test-{}.txt", std::process::id()));
        let body = "(A) Buy milk\n".to_string();
        std::fs::write(&path, &body).unwrap();
        App::new(path, body, "2026-05-06".to_string(), Config::default())
    }

    /// The global Normal/List hint bar (`status::render`'s catch-all arm) is
    /// a hardcoded string with no other test coverage — confirm it actually
    /// advertises the `o` notes action rather than only trusting a code
    /// review of the literal.
    #[test]
    fn normal_mode_hint_follows_the_selection_and_points_at_the_menu() {
        let mut app = build_app();
        let line = |app: &App| {
            let backend = TestBackend::new(200, 1);
            let mut terminal = Terminal::new(backend).unwrap();
            terminal.draw(|f| super::render(f, f.area(), app)).unwrap();
            let buf = terminal.backend().buffer();
            (0..buf.area.width)
                .map(|x| buf[(x, 0)].symbol().to_string())
                .collect::<String>()
        };
        let text = line(&app);
        assert!(text.contains("␣ more…"), "{text}");
        assert!(
            text.contains("x done") || text.contains("n new task"),
            "{text}"
        );
        app.prefs.hints = false;
        let text = line(&app);
        assert!(
            !text.contains("x done") && !text.contains("n new task"),
            "{text}"
        );
        assert!(text.contains("␣ menu"), "{text}");
    }

    fn chip_cell(app: &App) -> (String, ratatui::style::Color) {
        let backend = TestBackend::new(200, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| super::render(f, f.area(), app)).unwrap();
        let buf = terminal.backend().buffer();
        let mut text = String::new();
        for x in 0..14 {
            text.push_str(buf[(x, 0)].symbol());
        }
        (text.to_uppercase(), buf[(3, 0)].bg)
    }

    /// A pinned note with focus leaves `app.mode` at `Mode::Normal`; the chip
    /// must still follow the pinned editor's own sub-mode, in its own color.
    #[test]
    fn mode_chip_follows_the_focused_pinned_note_sub_mode() {
        use crate::app::{NoteEditorMode, NoteEditorState};

        let mut app = build_app();
        let path =
            std::env::temp_dir().join(format!("tasq-status-pinned-mode-{}.md", std::process::id()));
        std::fs::write(&path, "hello").unwrap();
        app.pinned_notes
            .push(NoteEditorState::load(path.clone(), NoteEditorMode::Normal));
        app.pinned_focus = true;
        let (normal_text, normal_bg) = chip_cell(&app);
        assert!(normal_text.contains("NORMAL"), "{normal_text}");

        app.pinned_notes[0].enter_insert();
        let (insert_text, insert_bg) = chip_cell(&app);
        assert!(insert_text.contains("INSERT"), "{insert_text}");
        assert_ne!(normal_bg, insert_bg, "Insert gets its own chip color");

        app.pinned_focus = false;
        let (unfocused, _) = chip_cell(&app);
        assert!(unfocused.contains("NORMAL"), "main list is back in Normal");
        let _ = std::fs::remove_file(&path);
    }

    /// T13: the embedded editor's Normal-sub-mode hint should advertise the
    /// new `:`-command prompt, not just the pre-existing `Ctrl+S`.
    #[test]
    fn note_editor_normal_submode_hint_advertises_the_command_prompt() {
        use crate::app::{Mode, NoteEditorMode, NoteEditorState};

        let mut app = build_app();
        let path = std::env::temp_dir().join(format!(
            "tasq-status-note-editor-hint-{}.md",
            std::process::id()
        ));
        std::fs::write(&path, "hello").unwrap();
        app.notes_popup.active_editor =
            Some(NoteEditorState::load(path.clone(), NoteEditorMode::Normal));
        app.mode = Mode::Notes;

        let backend = TestBackend::new(200, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| super::render(f, f.area(), &app)).unwrap();
        let buf = terminal.backend().buffer();
        let mut text = String::new();
        for x in 0..buf.area.width {
            text.push_str(buf[(x, 0)].symbol());
        }
        assert!(
            text.contains(": cmd"),
            "editor Normal-sub-mode hint should advertise ':' for the command prompt: {text}"
        );

        let _ = std::fs::remove_file(&path);
    }

    /// User feedback: the notes-list browsing hint (no active editor) didn't
    /// mention `z`, even though it's usable directly from the list (pins the
    /// selected note straight to the side panel — `App::pin_selected_note_directly`).
    #[test]
    fn notes_list_browsing_hint_advertises_z_zoom() {
        use crate::app::Mode;

        let mut app = build_app();
        app.mode = Mode::Notes;

        let backend = TestBackend::new(200, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| super::render(f, f.area(), &app)).unwrap();
        let buf = terminal.backend().buffer();
        let mut text = String::new();
        for x in 0..buf.area.width {
            text.push_str(buf[(x, 0)].symbol());
        }
        assert!(
            text.contains("z zoom"),
            "notes-list browsing hint should advertise 'z zoom': {text}"
        );
    }
}
