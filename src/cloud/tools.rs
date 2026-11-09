//! What an assistant can do with an account's tasks, over MCP: read the
//! day, the calendar, tasks, spaces, tags, routines and notes; add, change,
//! complete and delete tasks (one occurrence of a repeat, or all of it);
//! subtasks (the checklist in a task's notes), descriptions and notes in
//! Markdown, and spaces. See [`super::assistant`] for how it signs in.
//!
//! Every call reads the account afresh (its changes replayed into a
//! database in memory) and sends what it changed, like one more computer.

use std::path::PathBuf;

use anyhow::{Result, anyhow, bail};
use chrono::{Duration, NaiveDate, Timelike};
use serde_json::{Value, json};

use super::client::Local;
use super::external::{ExtCalendar, event_json};
use super::keys::Keys;
use super::relay::Relay;
use super::run::sync_once;
use crate::app::checklist;
use crate::core::calendar::{self, Occurrence};
use crate::core::db::{Db, title_of};
use crate::core::spaces::{self, SpaceColor};
use crate::core::{
    AddOutcome, CompleteOutcome, DeleteOutcome, EditOutcome, RenameOutcome, SpaceSettingOutcome,
    Store, series,
};
use crate::note::{
    NOTES_TASKS_SUBDIR, NOTES_UNLINKED_SUBDIR, generate_notes_id, notes_id_from_raw,
};
use crate::todo::{self, Task};

/// The widest agenda one call returns.
const MAX_DAYS: i64 = 62;
/// The tag naming a task's main note (as the Notes screen sets it).
const MAIN_KEY: &str = "main";
/// Where subtasks added here go, as in the app's inspector.
const CHECKLIST_FILE: &str = "checklist.md";

pub(super) const INSTRUCTIONS: &str = "tasq is the user's task manager and calendar (a terminal \
app synced across their computers). Times are the user's local time; `now` tells you the date. \
Reading: `now` (right now, next, today), `agenda` (days or a range), `late`, `inbox` (no day yet), \
`list_tasks`, `search` (tasks and notes), `get_task` (everything about one task: notes, subtasks, \
next repeats, streak), `free_time`, `routines`, `list_spaces`, `list_tags`, `list_notes`, \
`read_note`. `now`, `agenda` and `free_time` include the meetings of the user's other calendars (work Google accounts…), read-only. Changing: `add_task` takes the user's own words as they'd type them in tasq, in \
English or Spanish (\"dentista el viernes a las 6 durante 1h en salud\", \"gym every monday at \
7am\", \"pay rent by the 1st high priority\"); tasq reads dates, deadlines (\"by\"), times, \
durations, repeats, spaces (\"in work\") and @tags from the text, and the optional fields \
override or add to it. Use `preview_task` first if unsure how it will read. `update_task`, \
`delete_task` and `complete_task` take a task's `id` from an earlier answer; for a repeating \
task pass `occurrence` (a date) to change or delete just that one. Subtasks are the checklist in \
the task's notes. Deleted tasks go to the trash on the user's computers for 30 days. Say what you \
changed, briefly.";

/// The account's tasks, read by replaying its changes into a database in
/// memory. Nothing is written to disk.
pub(super) struct Account<'a> {
    relay: &'a Relay,
    keys: Keys,
    today: NaiveDate,
    pub(super) store: Store,
}

impl<'a> Account<'a> {
    pub(super) fn open(relay: &'a Relay, keys: &Keys, today: NaiveDate) -> Result<Self> {
        let today_s = today.to_string();
        let mut db = Db::memory()?;
        let mut remote = Local {
            relay,
            keys: keys.clone(),
        };
        sync_once(&mut db, &mut remote, keys, &today_s)?;
        let store = Store::from_db(db, PathBuf::from(":memory:"), today_s)?;
        Ok(Self {
            relay,
            keys: keys.clone(),
            today,
            store,
        })
    }

    /// Send what changed here to the account, for the user's computers.
    fn send(&mut self) -> Result<()> {
        let today = self.today.to_string();
        let keys = self.keys.clone();
        let mut remote = Local {
            relay: self.relay,
            keys: keys.clone(),
        };
        let db = self
            .store
            .db
            .as_mut()
            .ok_or_else(|| anyhow!("no database"))?;
        sync_once(db, &mut remote, &keys, &today)?;
        Ok(())
    }

    fn db(&self) -> Result<&Db> {
        self.store.db.as_ref().ok_or_else(|| anyhow!("no database"))
    }

    /// Your own themes, as your computers shared them.
    pub(super) fn db_shared_themes(&self) -> Vec<(String, String)> {
        self.db()
            .ok()
            .and_then(|db| db.shared_themes().ok())
            .unwrap_or_default()
    }

    /// The other calendars' events from `from` to `to` (local days in
    /// `tz`), for the accounts shared on all devices.
    pub(super) fn external(&self, from: NaiveDate, to: NaiveDate) -> ExtCalendar {
        let shared = self
            .db()
            .ok()
            .and_then(|db| db.shared_accounts().ok())
            .unwrap_or_default();
        let secs = |d: NaiveDate| {
            d.and_hms_opt(0, 0, 0)
                .map_or(0, |t| t.and_utc().timestamp())
        };
        // A day of margin each side: the zone moves the edges.
        let window = (secs(from) - 86_400, secs(to) + 2 * 86_400);
        self.relay.externals.read(
            &self.keys.account,
            &shared,
            window,
            chrono::Utc::now().timestamp(),
        )
    }

    fn db_mut(&mut self) -> Result<&mut Db> {
        self.store.db.as_mut().ok_or_else(|| anyhow!("no database"))
    }

    pub(super) fn tasks(&self) -> &[Task] {
        self.store.tasks()
    }

    fn find(&self, id: &str) -> Result<usize> {
        if id.trim().is_empty() {
            bail!("which task? pass its id");
        }
        self.tasks()
            .iter()
            .position(|t| t.id == id.trim())
            .ok_or_else(|| {
                anyhow!(
                    "no task with id {id} (it may be done and archived, or deleted; search again)"
                )
            })
    }

    fn space_paths(&self) -> Vec<String> {
        // Most used first, as the add dialog orders them.
        let mut counted: Vec<(String, usize)> = self
            .store
            .known_spaces()
            .iter()
            .map(|s| {
                let n = self
                    .tasks()
                    .iter()
                    .filter(|t| spaces::in_space(&t.projects, &s.path))
                    .count();
                (s.path.clone(), n)
            })
            .collect();
        counted.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        counted.into_iter().map(|(p, _)| p).collect()
    }

    fn edit(&mut self, abs: usize, raw: &str) -> Result<usize> {
        match self.store.edit_line(abs, raw) {
            EditOutcome::Saved { abs } => Ok(abs),
            other => Err(anyhow!("couldn't change it: {other:?}")),
        }
    }

    /// The task's notes folder id, made (and linked) if it has none.
    fn notes_id(&mut self, abs: usize) -> Result<(String, usize)> {
        let raw = self.tasks()[abs].raw.clone();
        if let Some(id) = notes_id_from_raw(&raw) {
            return Ok((id, abs));
        }
        let id = generate_notes_id();
        let abs = self.edit(abs, &format!("{raw} notes:{id}/"))?;
        Ok((id, abs))
    }

    /// The notes of task `t`: `(file name, body)`, the main note first.
    pub(super) fn task_notes(&self, t: &Task) -> Result<Vec<(String, String)>> {
        let Some(id) = notes_id_from_raw(&t.raw) else {
            return Ok(Vec::new());
        };
        let prefix = format!("{NOTES_TASKS_SUBDIR}/{id}/");
        let main = todo::find_kv(&t.raw, MAIN_KEY);
        let mut notes: Vec<(String, String)> = self
            .db()?
            .notes()?
            .into_iter()
            .filter_map(|n| Some((n.path.strip_prefix(&prefix)?.to_string(), n.body)))
            .filter(|(name, _)| !name.contains('/'))
            .collect();
        notes.sort_by(|a, b| {
            let rank = |n: &str| (Some(n) != main.as_deref(), n.to_string());
            rank(&a.0).cmp(&rank(&b.0))
        });
        Ok(notes)
    }

    fn task_with_folder(&self, notes_id: &str) -> Option<&Task> {
        self.tasks()
            .iter()
            .find(|t| notes_id_from_raw(&t.raw).as_deref() == Some(notes_id))
    }
}

// ---------------------------------------------------------------- shapes

fn hhmm(m: u32) -> String {
    format!("{:02}:{:02}", m / 60 % 24, m % 60)
}

/// A task as the assistant sees it.
fn task_json(t: &Task) -> Value {
    let body = todo::body_after_priority(&t.clean_raw);
    json!({
        "id": t.id,
        "title": title_of(&t.raw),
        "space": t.projects.first(),
        "tags": t.contexts,
        "priority": t.priority.map(|p| p.to_string()),
        "date": t.planned,
        "due": t.due,
        "time": todo::find_kv(body, "at"),
        "duration": t.duration,
        "repeat": t.rec,
        "repeat_until": t.until,
        "repeat_times_left": t.times,
        "reminders": t.reminders,
        "ends": t.end,
        "event": t.event,
        "starred": t.starred,
        "done": t.done,
        "done_on": t.done_date,
        "has_notes": notes_id_from_raw(&t.raw).is_some(),
    })
}

fn occurrence_json(o: &Occurrence, tasks: &[Task]) -> Value {
    let mut v = task_json(&tasks[o.abs]);
    v["on"] = json!(o.date.to_string());
    v["start"] = json!(o.start.map(hhmm));
    v["end"] = json!(o.end().map(hhmm));
    v["all_day"] = json!(o.start.is_none());
    v["deadline_marker"] = json!(o.deadline);
    v["late"] = json!(o.late);
    v["upcoming_repeat"] = json!(o.projected);
    if o.span > 1 {
        v["day_of_span"] = json!(format!("{}/{}", o.day + 1, o.span));
    }
    v
}

pub(super) fn note_title(name: &str, body: &str) -> String {
    body.lines()
        .find_map(|l| l.trim().strip_prefix("# ").map(str::trim))
        .filter(|t| !t.is_empty())
        .map_or_else(|| name.trim_end_matches(".md").to_string(), str::to_string)
}

fn preview(body: &str) -> String {
    let text: String = body
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .take(3)
        .collect::<Vec<_>>()
        .join(" ");
    text.chars().take(200).collect()
}

fn subtasks_json(notes: &[(String, String)]) -> Vec<Value> {
    let mut out = Vec::new();
    for (name, body) in notes {
        for (i, line) in body.lines().enumerate() {
            if let Some((done, text)) = checklist::parse_item(line) {
                out.push(json!({ "subtask": format!("{name}:{i}"), "done": done, "text": text }));
            }
        }
    }
    out
}

// ---------------------------------------------------------------- arguments

fn s<'v>(args: &'v Value, key: &str) -> Option<&'v str> {
    args.get(key)?.as_str()
}

fn strings(args: &Value, key: &str) -> Vec<String> {
    match args.get(key) {
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.trim().to_string()))
            .filter(|s| !s.is_empty())
            .collect(),
        Some(Value::String(s)) if !s.trim().is_empty() => vec![s.trim().to_string()],
        _ => Vec::new(),
    }
}

/// `YYYY-MM-DD`, or words tasq reads ("friday", "mañana", "next week").
fn read_date(v: &str, today: NaiveDate) -> Result<NaiveDate> {
    let v = v.trim();
    if let Ok(d) = NaiveDate::parse_from_str(v, "%Y-%m-%d") {
        return Ok(d);
    }
    let p = crate::nl::detect(&format!("x {v}"), today, &[]).parsed;
    p.planned
        .or(p.due)
        .ok_or_else(|| anyhow!("couldn't read {v:?} as a date (use YYYY-MM-DD)"))
}

fn arg_date(args: &Value, key: &str, today: NaiveDate) -> Result<Option<NaiveDate>> {
    match s(args, key).map(str::trim).filter(|v| !v.is_empty()) {
        None => Ok(None),
        Some(v) => read_date(v, today).map(Some),
    }
}

/// `HH:MM`, or "6pm", "a las 10".
fn read_time(v: &str, today: NaiveDate) -> Result<u32> {
    if let Some(m) = calendar::parse_time(v.trim()) {
        return Ok(m);
    }
    let p = crate::nl::detect(&format!("x at {}", v.trim()), today, &[]).parsed;
    p.time
        .map(|(h, m)| h * 60 + m)
        .ok_or_else(|| anyhow!("couldn't read {v:?} as a time (use HH:MM, 24h)"))
}

fn is_priority_token(tok: &str) -> bool {
    let b = tok.as_bytes();
    b.len() == 3 && b[0] == b'(' && b[1].is_ascii_uppercase() && b[2] == b')'
}

fn token_is_kv(tok: &str) -> bool {
    tok.split_once(':')
        .is_some_and(|(k, v)| todo::is_valid_key(k) && !v.is_empty() && !v.starts_with("//"))
}

fn token_is_tag(tok: &str) -> bool {
    (tok.starts_with('+') || tok.starts_with('@')) && tok.len() > 1
}

/// `raw` with its words (the title) replaced, keeping the markup.
fn retitle(raw: &str, title: &str) -> String {
    let toks: Vec<&str> = raw.split_whitespace().collect();
    let mut lead = 0;
    if toks.first() == Some(&"x") {
        lead += 1;
    }
    while lead < toks.len()
        && (is_priority_token(toks[lead])
            || NaiveDate::parse_from_str(toks[lead], "%Y-%m-%d").is_ok())
    {
        lead += 1;
    }
    let mut out: Vec<String> = toks[..lead].iter().map(|t| t.to_string()).collect();
    out.push(title.trim().to_string());
    out.extend(
        toks[lead..]
            .iter()
            .filter(|t| token_is_kv(t) || token_is_tag(t))
            .map(|t| t.to_string()),
    );
    out.join(" ")
}

fn set_priority(raw: &str, p: Option<char>) -> String {
    let mut toks: Vec<String> = raw.split_whitespace().map(str::to_string).collect();
    if toks.first().is_some_and(|t| is_priority_token(t)) {
        toks.remove(0);
    }
    if let Some(p) = p
        && toks.first().is_none_or(|t| t != "x")
    {
        toks.insert(0, format!("({p})"));
    }
    toks.join(" ")
}

fn read_priority(v: &str) -> Result<Option<char>> {
    Ok(match crate::nl_es::fold(v.trim()).as_str() {
        "" | "none" | "ninguna" | "sin" => None,
        "a" | "high" | "alta" | "1" => Some('A'),
        "b" | "medium" | "media" | "2" => Some('B'),
        "c" | "low" | "baja" | "3" => Some('C'),
        other => bail!("priority {other:?}: use A (high), B (medium), C (low) or none"),
    })
}

fn tag_name(v: &str) -> Result<String> {
    let name = v.trim().trim_start_matches(['@', '+']).replace(' ', "-");
    if !todo::is_valid_tag_name(&name) {
        bail!("{v:?} can't be a name here");
    }
    Ok(name)
}

/// A space typed as its name ("exams") or path ("Uni/Exams"): the known
/// one it means, or a new one.
fn resolve_space(v: &str, known: &[String]) -> Result<String> {
    let v = v.trim().trim_start_matches('+');
    let f = crate::nl_es::fold(v);
    if let Some(p) = known.iter().find(|p| crate::nl_es::fold(p) == f) {
        return Ok(p.clone());
    }
    if let Some(p) = known
        .iter()
        .find(|p| crate::nl_es::fold(spaces::leaf(p)) == f)
    {
        return Ok(p.clone());
    }
    let path: Vec<String> = v.split('/').map(tag_name).collect::<Result<_>>()?;
    Ok(path.join("/"))
}

fn without_tokens(raw: &str, keep: impl Fn(&str) -> bool) -> String {
    raw.split_whitespace()
        .filter(|t| keep(t))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Apply the optional fields of `args` to `raw`.
fn apply(raw: &str, args: &Value, today: NaiveDate, known: &[String]) -> Result<String> {
    let mut raw = raw.to_string();
    if let Some(t) = s(args, "title").filter(|t| !t.trim().is_empty()) {
        raw = retitle(&raw, t);
    }
    if let Some(p) = s(args, "priority") {
        raw = set_priority(&raw, read_priority(p)?);
    }
    if let Some(sp) = s(args, "space") {
        raw = without_tokens(&raw, |t| !(t.starts_with('+') && t.len() > 1));
        if !sp.trim().is_empty() {
            raw = format!("{raw} +{}", resolve_space(sp, known)?);
        }
    }
    for tag in strings(args, "remove_tags") {
        let name = tag_name(&tag)?;
        raw = without_tokens(&raw, |t| !t.eq_ignore_ascii_case(&format!("@{name}")));
    }
    for tag in strings(args, "tags")
        .into_iter()
        .chain(strings(args, "add_tags"))
    {
        let name = tag_name(&tag)?;
        if !raw
            .split_whitespace()
            .any(|t| t.eq_ignore_ascii_case(&format!("@{name}")))
        {
            raw = format!("{raw} @{name}");
        }
    }
    for (arg, key) in [("date", "plan"), ("due", "due"), ("ends", "end")] {
        if let Some(v) = s(args, arg) {
            let d = if v.trim().is_empty() {
                None
            } else {
                Some(read_date(v, today)?.to_string())
            };
            raw = series::set_kv(&raw, key, d.as_deref());
        }
    }
    if let Some(v) = s(args, "time") {
        let t = if v.trim().is_empty() {
            None
        } else {
            Some(hhmm(read_time(v, today)?))
        };
        raw = series::set_kv(&raw, "at", t.as_deref());
    }
    if let Some(v) = args.get("duration") {
        let minutes = match v {
            Value::Number(n) => n.as_u64().map(|n| n as u32),
            Value::String(s) if s.trim().is_empty() => None,
            Value::String(s) => Some(
                crate::duration::parse_minutes(s.trim())
                    .or_else(|| {
                        crate::nl::detect(&format!("x for {s}"), today, &[])
                            .parsed
                            .duration
                    })
                    .ok_or_else(|| {
                        anyhow!("couldn't read {s:?} as a duration (e.g. 45m, 1h30m)")
                    })?,
            ),
            _ => None,
        };
        let v = minutes
            .filter(|m| *m > 0)
            .map(crate::duration::format_minutes);
        raw = series::set_kv(&raw, "dur", v.as_deref());
    }
    if let Some(v) = s(args, "reminders") {
        let v = v.trim();
        let list = if v.is_empty() {
            None
        } else {
            let mins = crate::duration::parse_reminders(&v.replace(' ', ""))
                .ok_or_else(|| anyhow!("reminders like \"15m\" or \"15m,1d\""))?;
            Some(
                mins.into_iter()
                    .map(crate::duration::format_minutes)
                    .collect::<Vec<_>>()
                    .join(","),
            )
        };
        raw = series::set_kv(&raw, todo::REMIND_KEY, list.as_deref());
    }
    if let Some(v) = s(args, "repeat") {
        for key in ["rec", todo::UNTIL_KEY, todo::TIMES_KEY, "skip"] {
            raw = series::set_kv(&raw, key, None);
        }
        if !v.trim().is_empty() {
            let p = crate::nl::detect(&format!("x {v}"), today, &[]).parsed;
            let rec = p.rec.ok_or_else(|| {
                anyhow!(
                    "couldn't read {v:?} as a repeat (e.g. \"every monday\", \"cada 2 semanas\")"
                )
            })?;
            raw = series::set_kv(&raw, "rec", Some(&rec));
            if let Some(u) = p.until {
                raw = series::set_kv(&raw, todo::UNTIL_KEY, Some(&u.to_string()));
            }
            if let Some(n) = p.times {
                raw = series::set_kv(&raw, todo::TIMES_KEY, Some(&n.to_string()));
            }
            if let Some(d) = p.planned.filter(|_| s(args, "date").is_none()) {
                raw = series::set_kv(&raw, "plan", Some(&d.to_string()));
            }
        }
    }
    raw = with_first_day(&raw, today);
    for (arg, key) in [("starred", todo::STAR_KEY), ("event", "event")] {
        if let Some(b) = args.get(arg).and_then(Value::as_bool) {
            raw = series::set_kv(&raw, key, b.then_some("1"));
        }
    }
    todo::parse_line(&raw).map_err(|e| anyhow!("that would make an invalid task: {e}"))?;
    Ok(raw)
}

/// A repeat needs a first day to fall on: its first match from today on.
fn with_first_day(raw: &str, today: NaiveDate) -> String {
    let body = todo::body_after_priority(raw);
    match todo::find_kv(body, "rec") {
        Some(rec)
            if todo::find_kv(body, "plan").is_none() && todo::find_kv(body, "due").is_none() =>
        {
            let first = crate::nl::first_rec_day(&rec, today).unwrap_or(today);
            series::set_kv(raw, "plan", Some(&first.to_string()))
        }
        _ => raw.to_string(),
    }
}

/// What the user typed, read as tasq's add box reads it.
fn read_text(text: &str, today: NaiveDate, known: &[String]) -> String {
    let d = crate::nl::detect_in(text, today, &[], known);
    if d.is_empty() {
        text.trim().to_string()
    } else {
        d.to_todo_txt()
    }
}

fn slug(title: &str) -> String {
    let f = crate::nl_es::fold(title);
    let s: String = f
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let s = s
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let s: String = s.chars().take(60).collect();
    if s.is_empty() { "note".into() } else { s }
}

/// A note path the assistant passes: kept inside the database's notes.
fn note_path(p: &str) -> Result<String> {
    let p = p.trim().trim_start_matches('/');
    let ok = (p.starts_with(&format!("{NOTES_TASKS_SUBDIR}/"))
        || p.starts_with(&format!("{NOTES_UNLINKED_SUBDIR}/")))
        && p.ends_with(".md")
        && !p.split('/').any(|c| c.is_empty() || c == "." || c == "..");
    if ok {
        Ok(p.to_string())
    } else {
        bail!("{p:?} isn't a note path (use one from list_notes or get_task)")
    }
}

fn file_name(name: &str) -> Result<String> {
    let base = name.trim().trim_end_matches(".md");
    if base.is_empty() || base.contains('/') {
        bail!("a note name like \"notes\" or \"apuntes.md\"");
    }
    Ok(format!("{}.md", slug(base)))
}

// ---------------------------------------------------------------- the list

fn tool(
    name: &str,
    title: &str,
    description: &str,
    props: Value,
    required: &[&str],
    kind: Kind,
) -> Value {
    json!({
        "name": name,
        "title": title,
        "description": description,
        "inputSchema": { "type": "object", "properties": props, "required": required },
        "annotations": {
            "title": title,
            "readOnlyHint": kind == Kind::Read,
            "destructiveHint": kind == Kind::Destructive,
            "idempotentHint": kind == Kind::Read,
            "openWorldHint": false,
        },
    })
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum Kind {
    Read,
    Write,
    Destructive,
}

const WRITES: &[&str] = &[
    "add_task",
    "update_task",
    "complete_task",
    "reopen_task",
    "delete_task",
    "add_subtasks",
    "set_subtask",
    "remove_subtask",
    "write_task_note",
    "create_note",
    "update_note",
    "delete_note",
    "create_space",
    "update_space",
];

pub(super) fn tool_list(write: bool) -> Vec<Value> {
    let st = |d: &str| json!({ "type": "string", "description": d });
    let date = st("YYYY-MM-DD, or words like \"friday\", \"mañana\"");
    let id = st("the task's id, from an earlier answer");
    let occurrence = st(
        "for a repeating task: the date (YYYY-MM-DD) of the one occurrence to act on; leave out for the whole task",
    );
    let list =
        |d: &str| json!({ "type": "array", "items": { "type": "string" }, "description": d });
    let fields = |extra: Value| {
        let mut p = json!({
            "title": st("new title (words only)"),
            "priority": st("A/high, B/medium, C/low, or none"),
            "space": st("space name or path (\"work\", \"Uni/Exams\"); empty removes it; a new name makes a space"),
            "tags": list("tags to add (without @)"),
            "date": st("the planned day: YYYY-MM-DD or words; empty removes it"),
            "due": st("the deadline: YYYY-MM-DD or words; empty removes it"),
            "time": st("time of day: HH:MM (24h) or \"6pm\"; empty makes it all-day"),
            "duration": st("how long: \"45m\", \"1h30m\"; empty removes it"),
            "reminders": st("before its time: \"15m\" or \"15m,1d\"; empty removes them"),
            "repeat": st("a repeat in words: \"every monday\", \"cada 2 semanas until dec 20\", \"daily 10 times\"; empty stops repeating"),
            "ends": st("last day of something lasting several days (YYYY-MM-DD); empty removes it"),
            "starred": { "type": "boolean", "description": "pinned to the top of its group" },
            "event": { "type": "boolean", "description": "an event (a class, a trip): not ticked off, never late" },
        });
        if let (Some(p), Some(e)) = (p.as_object_mut(), extra.as_object()) {
            for (k, v) in e {
                p.insert(k.clone(), v.clone());
            }
        }
        p
    };
    let mut tools = vec![
        tool(
            "now",
            "What's on now",
            "The user's local date and time; what they're doing right now; what's next; the rest of today (timed and all-day); what's late; how many of today's tasks are done. Meetings from the user's other calendars (kind `external_event`, read-only, with their video-call link) count too.",
            json!({}),
            &[],
            Kind::Read,
        ),
        tool(
            "agenda",
            "Agenda",
            "The calendar from `from` to `to` (inclusive, at most 62 days; both default to today), by day: timed blocks with start and end, all-day tasks, events lasting several days, deadline markers and upcoming repeats. Late open tasks show on today. Also the events of the user's other calendars (kind `external_event`, read-only; left out when filtering by space).",
            json!({ "from": date, "to": date, "space": st("only this space (and its sub-spaces)"), "include_done": { "type": "boolean" } }),
            &[],
            Kind::Read,
        ),
        tool(
            "late",
            "Late tasks",
            "Open tasks whose day or deadline has passed.",
            json!({}),
            &[],
            Kind::Read,
        ),
        tool(
            "inbox",
            "Inbox",
            "Open tasks with no day and no deadline yet: what's left to put in the calendar.",
            json!({}),
            &[],
            Kind::Read,
        ),
        tool(
            "list_tasks",
            "List tasks",
            "Tasks filtered by space, tag, priority, starred, status and whether they have a day.",
            json!({
                "space": st("space name or path (includes sub-spaces)"),
                "tag": st("a tag (without @)"),
                "priority": st("A, B or C"),
                "starred": { "type": "boolean" },
                "status": { "type": "string", "enum": ["open", "done", "all"], "description": "default open" },
                "dated": { "type": "string", "enum": ["any", "yes", "no"], "description": "default any" },
                "limit": { "type": "integer", "description": "default 100" },
            }),
            &[],
            Kind::Read,
        ),
        tool(
            "search",
            "Search",
            "Find tasks (title, space, tags) and notes (title and text, with the matching line) containing all the words; accents and case don't matter.",
            json!({ "query": st("words to find"), "include_done": { "type": "boolean" } }),
            &["query"],
            Kind::Read,
        ),
        tool(
            "get_task",
            "Get a task",
            "Everything about one task: its fields, its notes (Markdown, the main one first), its subtasks (checklist), the next dates it repeats on, its streak if it's a routine, and its todo.txt line.",
            json!({ "id": id }),
            &["id"],
            Kind::Read,
        ),
        tool(
            "free_time",
            "Free time",
            "The free gaps in a day between `from` and `to` (default 08:00–22:00) at least `min_minutes` long (default 30), around the timed tasks and the meetings of the user's other calendars.",
            json!({ "date": date, "from": st("HH:MM"), "to": st("HH:MM"), "min_minutes": { "type": "integer" } }),
            &[],
            Kind::Read,
        ),
        tool(
            "routines",
            "Routines",
            "The user's repeating tasks (habits), each with its streak (done in a row), how it repeats, its next date and the last seven days.",
            json!({}),
            &[],
            Kind::Read,
        ),
        tool(
            "list_spaces",
            "Spaces",
            "The spaces (folders like Uni › Exams) as a tree, with open-task counts, colours and whether they're hidden.",
            json!({}),
            &[],
            Kind::Read,
        ),
        tool(
            "list_tags",
            "Tags",
            "Every tag in use on open tasks, with how many.",
            json!({}),
            &[],
            Kind::Read,
        ),
        tool(
            "list_notes",
            "Notes",
            "Notes, the one changed last first: path, title, a preview and the task it belongs to (if any). Optionally only those containing `query`.",
            json!({ "query": st("words to find"), "limit": { "type": "integer", "description": "default 30" } }),
            &[],
            Kind::Read,
        ),
        tool(
            "read_note",
            "Read a note",
            "A note's full Markdown, by its path.",
            json!({ "path": st("from list_notes or get_task") }),
            &["path"],
            Kind::Read,
        ),
        tool(
            "preview_task",
            "Preview a task",
            "How tasq would read some text as a new task (date, time, space, repeat…), without saving anything.",
            json!({ "text": st("as the user would type it") }),
            &["text"],
            Kind::Read,
        ),
    ];
    if write {
        tools.extend([
            tool("add_task", "Add a task",
                "Add a task from the user's own words, read the way tasq's add box reads them (English or Spanish): dates (\"tomorrow\", \"el viernes\"), deadlines (\"by friday\"), times (\"at 6pm\", \"a las 10\"), durations (\"for 1h\"), repeats (\"every monday\"), spaces (\"in work\"), @tags, priority (\"high priority\"), events lasting days (\"from dec 22 to jan 7\"). The optional fields set or override any of it. `description` becomes the task's main note (Markdown); `subtasks` its checklist. Returns the task as saved.",
                fields(json!({
                    "text": st("the task, in the user's words"),
                    "description": st("Markdown: the task's main note"),
                    "subtasks": list("checklist items"),
                })),
                &["text"], Kind::Write),
            tool("update_task", "Change a task",
                "Change any of a task's fields; only the ones given change. For a repeating task, `occurrence` changes just that day's one (it becomes a task of its own and the series skips that day).",
                fields(json!({
                    "id": id,
                    "occurrence": occurrence,
                    "remove_tags": list("tags to remove"),
                })),
                &["id"], Kind::Write),
            tool("complete_task", "Complete tasks",
                "Mark one or more tasks done. A repeating one moves on to its next date (and its streak grows).",
                json!({ "id": id, "ids": list("several task ids at once") }),
                &[], Kind::Write),
            tool("reopen_task", "Reopen a task", "Mark a done task as not done again.", json!({ "id": id }), &["id"], Kind::Write),
            tool("delete_task", "Delete a task",
                "Delete a task (it waits in the trash on the user's computers for 30 days). For a repeating task, `occurrence` deletes just that day's one and the series goes on.",
                json!({ "id": id, "occurrence": occurrence }),
                &["id"], Kind::Destructive),
            tool("add_subtasks", "Add subtasks",
                "Add checklist items to a task (in its notes, as - [ ] lines).",
                json!({ "id": id, "items": list("the subtasks") }),
                &["id", "items"], Kind::Write),
            tool("set_subtask", "Tick a subtask",
                "Tick or untick a subtask, by its `subtask` handle from get_task.",
                json!({ "id": id, "subtask": st("e.g. \"checklist.md:3\""), "done": { "type": "boolean" } }),
                &["id", "subtask", "done"], Kind::Write),
            tool("remove_subtask", "Remove a subtask", "Remove a subtask, by its `subtask` handle from get_task.",
                json!({ "id": id, "subtask": st("e.g. \"checklist.md:3\"") }),
                &["id", "subtask"], Kind::Destructive),
            tool("write_task_note", "Write a task's note",
                "Write a task's description or one of its notes, in Markdown. `mode` replace (default) or append. Without `name`, it's the task's main note (made if there's none). `main: true` makes it the main note.",
                json!({
                    "id": id,
                    "body": st("Markdown"),
                    "name": st("note name, e.g. \"apuntes\"; default the main note"),
                    "mode": { "type": "string", "enum": ["replace", "append"] },
                    "main": { "type": "boolean" },
                }),
                &["id", "body"], Kind::Write),
            tool("create_note", "Create a note",
                "A new note on its own (not a task's), in Markdown, under a title.",
                json!({ "title": st("the note's title"), "body": st("Markdown") }),
                &["title"], Kind::Write),
            tool("update_note", "Change a note",
                "Replace or append to a note's Markdown, by its path.",
                json!({ "path": st("from list_notes or get_task"), "body": st("Markdown"), "mode": { "type": "string", "enum": ["replace", "append"] } }),
                &["path", "body"], Kind::Write),
            tool("delete_note", "Delete a note", "Delete a note, by its path.", json!({ "path": st("from list_notes or get_task") }), &["path"], Kind::Destructive),
            tool("create_space", "Create a space", "A new space (folder), e.g. \"Work\" or \"Uni/Exams\".", json!({ "path": st("space path") }), &["path"], Kind::Write),
            tool("update_space", "Change a space",
                "Rename a space (its tasks and sub-spaces follow), set its colour, or hide/show it.",
                json!({
                    "space": st("space name or path"),
                    "rename_to": st("new path"),
                    "color": st("#rrggbb, a palette slot number, or empty for automatic"),
                    "hidden": { "type": "boolean" },
                }),
                &["space"], Kind::Write),
        ]);
    }
    tools
}

// ---------------------------------------------------------------- calls

pub(super) fn call_tool(
    relay: &Relay,
    keys: &Keys,
    tz: chrono_tz::Tz,
    write: bool,
    name: &str,
    args: &Value,
) -> Result<Value> {
    if WRITES.contains(&name) && !write {
        bail!("this connection can only read: reconnect tasq in Claude allowing changes");
    }
    let local = chrono::Utc::now().with_timezone(&tz);
    let today = local.date_naive();
    let minute = local.hour() * 60 + local.minute();
    let mut a = Account::open(relay, keys, today)?;
    let out = match name {
        "now" => now(
            &a,
            today,
            minute,
            &local.format("%Y-%m-%d %H:%M, %A").to_string(),
            tz,
        ),
        "agenda" => agenda(&a, args, today, tz),
        "late" => Ok(late(&a, today)),
        "inbox" => Ok(inbox(&a)),
        "list_tasks" => list_tasks(&a, args),
        "search" => search(&a, args),
        "get_task" => get_task(&a, args, today),
        "free_time" => free_time(&a, args, today, tz),
        "routines" => Ok(routines(&a, today)),
        "list_spaces" => Ok(list_spaces(&a)),
        "list_tags" => Ok(list_tags(&a)),
        "list_notes" => list_notes(&a, args),
        "read_note" => read_note(&a, args),
        "preview_task" => preview_task(&a, args, today),
        "add_task" => add_task(&mut a, args, today),
        "update_task" => update_task(&mut a, args, today),
        "complete_task" => complete_task(&mut a, args),
        "reopen_task" => reopen_task(&mut a, args),
        "delete_task" => delete_task(&mut a, args, today),
        "add_subtasks" => add_subtasks(&mut a, args),
        "set_subtask" | "remove_subtask" => change_subtask(&mut a, args, name == "remove_subtask"),
        "write_task_note" => write_task_note(&mut a, args),
        "create_note" => create_note(&mut a, args),
        "update_note" => update_note(&mut a, args),
        "delete_note" => delete_note(&mut a, args),
        "create_space" => create_space(&mut a, args),
        "update_space" => update_space(&mut a, args),
        other => Err(anyhow!("no tool called {other}")),
    }?;
    if WRITES.contains(&name) {
        a.send()?;
    }
    Ok(out)
}

/// Tasks and the other calendars' events from `from` to `to`, by day, then
/// all-day first, then by start.
pub(super) fn with_external(
    a: &Account,
    from: NaiveDate,
    to: NaiveDate,
    today: NaiveDate,
    tz: chrono_tz::Tz,
) -> (Vec<Occurrence>, ExtCalendar) {
    let ext = a.external(from, to);
    let mut all = calendar::occurrences(a.tasks(), from, to, today);
    all.extend(ext.occurrences(from, to, &tz));
    all.sort_by(|x, y| {
        (x.date, x.start.is_some(), x.start).cmp(&(y.date, y.start.is_some(), y.start))
    });
    (all, ext)
}

/// A task's occurrence, or another calendar's event.
fn item_json(o: &Occurrence, tasks: &[Task], ext: &ExtCalendar) -> Value {
    match (ext.event_of(o), &o.ext) {
        (Some(c), Some(m)) => event_json(o, c, &m.calendar),
        _ => occurrence_json(o, tasks),
    }
}

/// Still to do: an open task, or any event.
fn is_open(o: &Occurrence, tasks: &[Task]) -> bool {
    o.ext.is_some() || tasks.get(o.abs).is_some_and(|t| !t.done)
}

fn now(
    a: &Account,
    today: NaiveDate,
    minute: u32,
    now_s: &str,
    tz: chrono_tz::Tz,
) -> Result<Value> {
    let tasks = a.tasks();
    let (occs, ext) = with_external(a, today, today + Duration::days(1), today, tz);
    let todays: Vec<&Occurrence> = occs.iter().filter(|o| o.date == today).collect();
    let current: Vec<Value> = todays
        .iter()
        .filter(|o| is_open(o, tasks))
        .filter(|o| o.start.is_some_and(|s| s <= minute) && o.end().is_some_and(|e| e > minute))
        .map(|o| item_json(o, tasks, &ext))
        .collect();
    let next = occs
        .iter()
        .filter(|o| is_open(o, tasks))
        .find(|o| o.start.is_some() && (o.date > today || o.start.is_some_and(|s| s > minute)))
        .map(|o| item_json(o, tasks, &ext));
    let done_today = tasks
        .iter()
        .filter(|t| t.done && t.done_date.as_deref() == Some(&today.to_string()))
        .count();
    let open_today = todays
        .iter()
        .filter(|o| o.ext.is_none() && is_open(o, tasks))
        .filter(|o| !tasks[o.abs].event)
        .count();
    let mut v = json!({
        "now": now_s,
        "timezone": tz.name(),
        "current": current,
        "next": next,
        "today": todays.iter().map(|o| item_json(o, tasks, &ext)).collect::<Vec<_>>(),
        "progress_today": { "done": done_today, "left": open_today },
    });
    note_ext_status(&mut v, &ext);
    Ok(v)
}

/// Say which other calendars couldn't be read just now.
fn note_ext_status(v: &mut Value, ext: &ExtCalendar) {
    if !ext.status.is_empty() {
        v["other_calendars_unavailable"] = json!(ext.status);
    }
}

fn agenda(a: &Account, args: &Value, today: NaiveDate, tz: chrono_tz::Tz) -> Result<Value> {
    let from = arg_date(args, "from", today)?.unwrap_or(today);
    let to = arg_date(args, "to", today)?.unwrap_or(from);
    if to < from {
        bail!("`to` is before `from`");
    }
    if (to - from).num_days() >= MAX_DAYS {
        bail!("at most {MAX_DAYS} days at a time");
    }
    let space = match s(args, "space").filter(|v| !v.trim().is_empty()) {
        Some(v) => Some(resolve_space(v, &a.space_paths())?),
        None => None,
    };
    let include_done = args["include_done"].as_bool().unwrap_or(true);
    let tasks = a.tasks();
    let (occs, ext) = with_external(a, from, to, today, tz);
    let mut days = Vec::new();
    let mut d = from;
    while d <= to {
        let items: Vec<Value> = occs
            .iter()
            .filter(|o| o.date == d)
            .filter(|o| include_done || is_open(o, tasks))
            .filter(|o| match &space {
                None => true,
                // A space asks for tasks: the other calendars stay out.
                Some(sp) => o.ext.is_none() && spaces::in_space(&tasks[o.abs].projects, sp),
            })
            .map(|o| item_json(o, tasks, &ext))
            .collect();
        days.push(
            json!({ "date": d.to_string(), "weekday": d.format("%A").to_string(), "items": items }),
        );
        d += Duration::days(1);
    }
    let mut v = json!({ "today": today.to_string(), "timezone": tz.name(), "days": days });
    note_ext_status(&mut v, &ext);
    Ok(v)
}

fn date_of(s: Option<&String>) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s?, "%Y-%m-%d").ok()
}

fn late(a: &Account, today: NaiveDate) -> Value {
    let found: Vec<Value> = a
        .tasks()
        .iter()
        .filter(|t| !t.done && !t.event)
        .filter(|t| {
            date_of(t.planned.as_ref()).is_some_and(|d| d < today)
                || date_of(t.due.as_ref()).is_some_and(|d| d < today)
        })
        .map(task_json)
        .collect();
    json!({ "count": found.len(), "tasks": found })
}

fn inbox(a: &Account) -> Value {
    let found: Vec<Value> = a
        .tasks()
        .iter()
        .filter(|t| !t.done && t.planned.is_none() && t.due.is_none())
        .map(task_json)
        .collect();
    json!({ "count": found.len(), "tasks": found })
}

fn list_tasks(a: &Account, args: &Value) -> Result<Value> {
    let space = match s(args, "space").filter(|v| !v.trim().is_empty()) {
        Some(v) => Some(resolve_space(v, &a.space_paths())?),
        None => None,
    };
    let tag = s(args, "tag").map(|t| crate::nl_es::fold(t.trim_start_matches('@')));
    let prio = match s(args, "priority") {
        Some(p) => read_priority(p)?,
        None => None,
    };
    let starred = args["starred"].as_bool();
    let status = s(args, "status").unwrap_or("open");
    let dated = s(args, "dated").unwrap_or("any");
    let limit = args["limit"].as_u64().unwrap_or(100).clamp(1, 500) as usize;
    let all: Vec<&Task> = a
        .tasks()
        .iter()
        .filter(|t| match status {
            "done" => t.done,
            "all" => true,
            _ => !t.done,
        })
        .filter(|t| {
            space
                .as_ref()
                .is_none_or(|sp| spaces::in_space(&t.projects, sp))
        })
        .filter(|t| {
            tag.as_ref()
                .is_none_or(|g| t.contexts.iter().any(|c| crate::nl_es::fold(c) == *g))
        })
        .filter(|t| prio.is_none() || t.priority == prio)
        .filter(|t| starred.is_none_or(|s| t.starred == s))
        .filter(|t| {
            let has = t.planned.is_some() || t.due.is_some();
            match dated {
                "yes" => has,
                "no" => !has,
                _ => true,
            }
        })
        .collect();
    Ok(json!({
        "count": all.len(),
        "tasks": all.iter().take(limit).map(|t| task_json(t)).collect::<Vec<_>>(),
    }))
}

fn search(a: &Account, args: &Value) -> Result<Value> {
    let words: Vec<String> = crate::nl_es::fold(s(args, "query").unwrap_or_default())
        .split_whitespace()
        .map(str::to_string)
        .collect();
    if words.is_empty() {
        bail!("search for something");
    }
    let has_all = |hay: &str| {
        let f = crate::nl_es::fold(hay);
        words.iter().all(|w| f.contains(w.as_str()))
    };
    let include_done = args["include_done"].as_bool().unwrap_or(false);
    let tasks: Vec<Value> = a
        .tasks()
        .iter()
        .filter(|t| include_done || !t.done)
        .filter(|t| {
            has_all(&format!(
                "{} {} {}",
                title_of(&t.raw),
                t.projects.join(" "),
                t.contexts.join(" ")
            ))
        })
        .take(50)
        .map(task_json)
        .collect();
    let mut notes = Vec::new();
    for n in a.db()?.notes()? {
        let name = n.path.rsplit('/').next().unwrap_or_default().to_string();
        if !has_all(&format!("{name} {}", n.body)) {
            continue;
        }
        let line = n
            .body
            .lines()
            .find(|l| {
                let f = crate::nl_es::fold(l);
                words.iter().any(|w| f.contains(w.as_str()))
            })
            .map(|l| l.trim().chars().take(200).collect::<String>());
        notes.push(json!({ "path": n.path, "title": note_title(&name, &n.body), "line": line }));
        if notes.len() >= 30 {
            break;
        }
    }
    Ok(json!({ "tasks": tasks, "notes": notes }))
}

fn routine_of(a: &Account, abs: usize, today: NaiveDate) -> Option<crate::app::Routine> {
    let t = &a.tasks()[abs];
    if t.done || t.event {
        return None;
    }
    let spec = crate::recurrence::parse_rec_spec(t.rec.as_deref()?)?;
    let k = crate::app::routines::key(t)?;
    let done: Vec<(Option<NaiveDate>, NaiveDate)> = a
        .tasks()
        .iter()
        .chain(a.store.archive().tasks())
        .filter(|d| d.done && crate::app::routines::key(d).as_ref() == Some(&k))
        .filter_map(|d| {
            Some((
                crate::app::routines::scheduled(d),
                date_of(d.done_date.as_ref())?,
            ))
        })
        .collect();
    Some(crate::app::routines::routine(t, abs, &spec, &done, today))
}

fn get_task(a: &Account, args: &Value, today: NaiveDate) -> Result<Value> {
    let abs = a.find(s(args, "id").unwrap_or_default())?;
    let t = &a.tasks()[abs];
    let notes = a.task_notes(t)?;
    let folder = notes_id_from_raw(&t.raw);
    let mut v = task_json(t);
    v["line"] = json!(t.raw);
    v["notes"] = json!(
        notes
            .iter()
            .map(|(name, body)| json!({
                "path": format!("{NOTES_TASKS_SUBDIR}/{}/{name}", folder.clone().unwrap_or_default()),
                "name": name,
                "title": note_title(name, body),
                "body": body,
            }))
            .collect::<Vec<_>>()
    );
    let subs = subtasks_json(&notes);
    let done = subs.iter().filter(|s| s["done"] == true).count();
    v["subtasks"] = json!(subs);
    if !subs.is_empty() {
        v["subtasks_done"] = json!(format!("{done}/{}", subs.len()));
    }
    if t.rec.is_some() && !t.done {
        v["next_dates"] = json!(
            series::repeats_from(t, today, 6)
                .iter()
                .map(|d| d.to_string())
                .collect::<Vec<_>>()
        );
    }
    if let Some(r) = routine_of(a, abs, today) {
        v["streak"] = json!(r.streak);
    }
    Ok(v)
}

fn free_time(a: &Account, args: &Value, today: NaiveDate, tz: chrono_tz::Tz) -> Result<Value> {
    let day = arg_date(args, "date", today)?.unwrap_or(today);
    let from = s(args, "from").map_or(Ok(8 * 60), |v| read_time(v, today))?;
    let to = s(args, "to").map_or(Ok(22 * 60), |v| read_time(v, today))?;
    let min = args["min_minutes"].as_u64().unwrap_or(30) as u32;
    let tasks = a.tasks();
    let (occs, ext) = with_external(a, day, day, today, tz);
    let busy: Vec<&Occurrence> = occs
        .iter()
        .filter(|o| o.date == day && o.start.is_some() && is_open(o, tasks))
        .collect();
    let slots = calendar::free_slots(&busy, from, to, min.max(1));
    Ok(json!({
        "date": day.to_string(),
        "free": slots
            .iter()
            .map(|(s, e)| json!({ "from": hhmm(*s), "to": hhmm(*e), "minutes": e - s }))
            .collect::<Vec<_>>(),
        "busy": busy.iter().map(|o| item_json(o, tasks, &ext)).collect::<Vec<_>>(),
    }))
}

fn routines(a: &Account, today: NaiveDate) -> Value {
    let mut out: Vec<(usize, Value)> = (0..a.tasks().len())
        .filter_map(|abs| {
            let r = routine_of(a, abs, today)?;
            let t = &a.tasks()[abs];
            let week: Vec<&str> = r
                .week
                .iter()
                .map(|m| match m {
                    crate::app::DayMark::Done => "done",
                    crate::app::DayMark::Missed => "missed",
                    crate::app::DayMark::Pending => "today",
                    crate::app::DayMark::Off => "-",
                })
                .collect();
            Some((
                r.streak,
                json!({
                    "id": t.id,
                    "title": r.title,
                    "streak": r.streak,
                    "streak_in": if r.daily { "days" } else { "times" },
                    "repeat": t.rec,
                    "next": t.planned.clone().or(t.due.clone()),
                    "last_7_days": week,
                }),
            ))
        })
        .collect();
    out.sort_by_key(|x| std::cmp::Reverse(x.0));
    json!({ "routines": out.into_iter().map(|(_, v)| v).collect::<Vec<_>>() })
}

fn list_spaces(a: &Account) -> Value {
    let known = a.store.known_spaces();
    let rows: Vec<Value> = a
        .store
        .space_tree()
        .iter()
        .map(|r| {
            let open = a
                .tasks()
                .iter()
                .filter(|t| !t.done && spaces::in_space(&t.projects, &r.path))
                .count();
            json!({
                "path": r.path,
                "name": spaces::leaf(&r.path),
                "depth": r.depth,
                "open_tasks": open,
                "hidden": r.hidden,
                "color": spaces::color_of(&r.path, known).to_value(),
            })
        })
        .collect();
    json!({ "spaces": rows })
}

fn list_tags(a: &Account) -> Value {
    let mut counts: std::collections::BTreeMap<String, usize> = Default::default();
    for t in a.tasks().iter().filter(|t| !t.done) {
        for c in &t.contexts {
            *counts.entry(c.clone()).or_default() += 1;
        }
    }
    json!({ "tags": counts.into_iter().map(|(k, n)| json!({ "tag": k, "open_tasks": n })).collect::<Vec<_>>() })
}

fn list_notes(a: &Account, args: &Value) -> Result<Value> {
    let words: Vec<String> = crate::nl_es::fold(s(args, "query").unwrap_or_default())
        .split_whitespace()
        .map(str::to_string)
        .collect();
    let limit = args["limit"].as_u64().unwrap_or(30).clamp(1, 200) as usize;
    let mut out = Vec::new();
    for n in a.db()?.notes()? {
        let name = n.path.rsplit('/').next().unwrap_or_default().to_string();
        let f = crate::nl_es::fold(&format!("{name} {}", n.body));
        if !words.iter().all(|w| f.contains(w.as_str())) {
            continue;
        }
        let task = n
            .path
            .strip_prefix(&format!("{NOTES_TASKS_SUBDIR}/"))
            .and_then(|r| r.split('/').next())
            .and_then(|id| a.task_with_folder(id))
            .map(|t| json!({ "id": t.id, "title": title_of(&t.raw) }));
        out.push(json!({
            "path": n.path,
            "title": note_title(&name, &n.body),
            "preview": preview(&n.body),
            "updated": n.updated,
            "task": task,
        }));
        if out.len() >= limit {
            break;
        }
    }
    Ok(json!({ "notes": out }))
}

fn read_note(a: &Account, args: &Value) -> Result<Value> {
    let path = note_path(s(args, "path").unwrap_or_default())?;
    let body = a
        .db()?
        .note(&path)?
        .ok_or_else(|| anyhow!("no note at {path}"))?;
    Ok(json!({ "path": path, "body": body }))
}

fn preview_task(a: &Account, args: &Value, today: NaiveDate) -> Result<Value> {
    let text = s(args, "text").unwrap_or_default();
    if text.trim().is_empty() {
        bail!("what would you add?");
    }
    let line = read_text(text, today, &a.space_paths());
    let t = crate::inbox::finalize_line(&line, &today.to_string())
        .map_err(|e| anyhow!("tasq couldn't read that: {e}"))?;
    let mut v = task_json(&t);
    v["line"] = json!(t.raw);
    v.as_object_mut().map(|o| o.remove("id"));
    Ok(json!({ "would_add": v }))
}

fn add_task(a: &mut Account, args: &Value, today: NaiveDate) -> Result<Value> {
    let text = s(args, "text").unwrap_or_default().trim();
    if text.is_empty() {
        bail!("say what to add");
    }
    let known = a.space_paths();
    let line = apply(&read_text(text, today, &known), args, today, &known)?;
    let abs = match a.store.add_finalized(&line) {
        AddOutcome::Added { abs } => abs,
        AddOutcome::Empty => bail!("say what to add"),
        AddOutcome::Aborted(_) => bail!("the tasks changed meanwhile; try again"),
        AddOutcome::Error(e) => bail!("tasq couldn't read that: {e}"),
    };
    let id = a.tasks()[abs].id.clone();
    if let Some(d) = s(args, "description").filter(|d| !d.trim().is_empty()) {
        write_note_of(a, abs, None, d, false, true)?;
    }
    let items = strings(args, "subtasks");
    if !items.is_empty() {
        let abs = a.find(&id)?;
        add_items(a, abs, &items)?;
    }
    let abs = a.find(&id)?;
    let t = &a.tasks()[abs];
    let notes = a.task_notes(t)?;
    let mut v = task_json(t);
    if !notes.is_empty() {
        v["subtasks"] = json!(subtasks_json(&notes));
    }
    Ok(json!({ "added": v }))
}

fn update_task(a: &mut Account, args: &Value, today: NaiveDate) -> Result<Value> {
    let abs = a.find(s(args, "id").unwrap_or_default())?;
    let known = a.space_paths();
    let t = a.tasks()[abs].clone();
    if let Some(on) = arg_date(args, "occurrence", today)? {
        if t.rec.is_none() {
            bail!("that task doesn't repeat: leave out `occurrence`");
        }
        // That day's one becomes a task of its own; the series skips it.
        let single = apply(&series::one_off(&t, on), args, today, &known)?;
        let rest = series::add_skip(&t.raw, on);
        return match a.store.rewrite_and_add(abs, &rest, &single) {
            EditOutcome::Saved { abs } => Ok(json!({
                "changed_occurrence": task_json(&a.tasks()[abs]),
                "series_skips": on.to_string(),
            })),
            other => Err(anyhow!("couldn't change it: {other:?}")),
        };
    }
    let raw = apply(&t.raw, args, today, &known)?;
    if raw == t.raw {
        return Ok(json!({ "unchanged": task_json(&t) }));
    }
    let abs = a.edit(abs, &raw)?;
    Ok(json!({ "updated": task_json(&a.tasks()[abs]) }))
}

fn complete_task(a: &mut Account, args: &Value) -> Result<Value> {
    let mut ids = strings(args, "ids");
    ids.extend(strings(args, "id"));
    if ids.is_empty() {
        bail!("which task? pass its id");
    }
    let mut done = Vec::new();
    for id in ids {
        let abs = a.find(&id)?;
        if a.tasks()[abs].done {
            done.push(json!({ "already_done": task_json(&a.tasks()[abs]) }));
            continue;
        }
        let next = match a.store.toggle_complete(abs) {
            CompleteOutcome::CompletedSpawned { next, .. } => Some(task_json(&a.tasks()[next])),
            CompleteOutcome::Completed { .. } | CompleteOutcome::CompletedLast { .. } => None,
            other => bail!("couldn't complete it: {other:?}"),
        };
        let abs = a.find(&id)?;
        done.push(json!({ "completed": task_json(&a.tasks()[abs]), "next_repeat": next }));
    }
    Ok(json!({ "results": done }))
}

fn reopen_task(a: &mut Account, args: &Value) -> Result<Value> {
    let id = s(args, "id").unwrap_or_default().to_string();
    let abs = a.find(&id)?;
    if !a.tasks()[abs].done {
        return Ok(json!({ "already_open": task_json(&a.tasks()[abs]) }));
    }
    match a.store.toggle_complete(abs) {
        CompleteOutcome::Uncompleted { abs } => {
            Ok(json!({ "reopened": task_json(&a.tasks()[abs]) }))
        }
        other => Err(anyhow!("couldn't reopen it: {other:?}")),
    }
}

fn delete_task(a: &mut Account, args: &Value, today: NaiveDate) -> Result<Value> {
    let abs = a.find(s(args, "id").unwrap_or_default())?;
    let t = a.tasks()[abs].clone();
    if let Some(on) = arg_date(args, "occurrence", today)? {
        if t.rec.is_none() {
            bail!("that task doesn't repeat: leave out `occurrence`");
        }
        let abs = a.edit(abs, &series::add_skip(&t.raw, on))?;
        return Ok(json!({
            "deleted_occurrence": on.to_string(),
            "series": task_json(&a.tasks()[abs]),
        }));
    }
    match a.store.delete(abs) {
        DeleteOutcome::Deleted { .. } => Ok(json!({
            "deleted": task_json(&t),
            "note": "in the trash on the user's computers for 30 days",
        })),
        other => Err(anyhow!("couldn't delete it: {other:?}")),
    }
}

/// Write note `name` (default: the main note, or `notes.md`) of task `abs`.
fn write_note_of(
    a: &mut Account,
    abs: usize,
    name: Option<&str>,
    body: &str,
    append: bool,
    make_main: bool,
) -> Result<String> {
    let (folder, abs) = a.notes_id(abs)?;
    let t = a.tasks()[abs].clone();
    let existing = a.task_notes(&t)?;
    let main = todo::find_kv(&t.raw, MAIN_KEY);
    let name = match name {
        Some(n) => file_name(n)?,
        None => main
            .clone()
            .or_else(|| existing.first().map(|(n, _)| n.clone()))
            .unwrap_or_else(|| "notes.md".into()),
    };
    let path = format!("{NOTES_TASKS_SUBDIR}/{folder}/{name}");
    let old = a.db()?.note(&path)?;
    let new_body = match old {
        Some(old) if append => {
            let sep = if old.ends_with('\n') { "" } else { "\n" };
            format!("{old}{sep}{body}\n")
        }
        // A new note starts with the task's title, as in the app.
        None if !body.trim_start().starts_with('#') => {
            format!("{}{}\n", crate::note::note_template(&t), body.trim_end())
        }
        _ => format!("{}\n", body.trim_end()),
    };
    a.db_mut()?.put_note(&path, &new_body)?;
    if (make_main || existing.is_empty()) && main.as_deref() != Some(&name) {
        let raw = series::set_kv(&t.raw, MAIN_KEY, Some(&name));
        a.edit(abs, &raw)?;
    }
    Ok(path)
}

fn add_items(a: &mut Account, abs: usize, items: &[String]) -> Result<Vec<Value>> {
    let (folder, abs) = a.notes_id(abs)?;
    let t = a.tasks()[abs].clone();
    let path = format!("{NOTES_TASKS_SUBDIR}/{folder}/{CHECKLIST_FILE}");
    let mut body = a
        .db()?
        .note(&path)?
        .unwrap_or_else(|| crate::note::note_template(&t));
    for it in items {
        body = checklist::add_item(&body, it.trim());
    }
    a.db_mut()?.put_note(&path, &body)?;
    if a.task_notes(&t)?.len() == 1 && todo::find_kv(&t.raw, MAIN_KEY).is_none() {
        let raw = series::set_kv(&t.raw, MAIN_KEY, Some(CHECKLIST_FILE));
        a.edit(abs, &raw)?;
    }
    let t = &a.tasks()[a.find(&t.id)?];
    Ok(subtasks_json(&a.task_notes(t)?))
}

fn add_subtasks(a: &mut Account, args: &Value) -> Result<Value> {
    let abs = a.find(s(args, "id").unwrap_or_default())?;
    let items = strings(args, "items");
    if items.is_empty() {
        bail!("which subtasks?");
    }
    let subs = add_items(a, abs, &items)?;
    Ok(json!({ "subtasks": subs }))
}

fn change_subtask(a: &mut Account, args: &Value, remove: bool) -> Result<Value> {
    let abs = a.find(s(args, "id").unwrap_or_default())?;
    let t = a.tasks()[abs].clone();
    let handle = s(args, "subtask").unwrap_or_default();
    let (name, line) = handle
        .rsplit_once(':')
        .and_then(|(n, l)| Some((n, l.parse::<usize>().ok()?)))
        .ok_or_else(|| anyhow!("a subtask handle like \"checklist.md:3\" (from get_task)"))?;
    let folder = notes_id_from_raw(&t.raw).ok_or_else(|| anyhow!("that task has no subtasks"))?;
    let path = format!("{NOTES_TASKS_SUBDIR}/{folder}/{name}");
    let body = a
        .db()?
        .note(&path)?
        .ok_or_else(|| anyhow!("no subtask {handle} (get_task again)"))?;
    let item = body.lines().nth(line).and_then(checklist::parse_item);
    let Some((done, _)) = item else {
        bail!("no subtask {handle} (get_task again)");
    };
    let new = if remove {
        checklist::remove_line(&body, line)
    } else if args["done"].as_bool().unwrap_or(true) != done {
        checklist::toggle_line(&body, line)
    } else {
        Some(body.clone())
    }
    .ok_or_else(|| anyhow!("couldn't change {handle}"))?;
    a.db_mut()?.put_note(&path, &new)?;
    Ok(json!({ "subtasks": subtasks_json(&a.task_notes(&t)?) }))
}

fn write_task_note(a: &mut Account, args: &Value) -> Result<Value> {
    let abs = a.find(s(args, "id").unwrap_or_default())?;
    let body = s(args, "body").unwrap_or_default();
    let append = s(args, "mode") == Some("append");
    let main = args["main"].as_bool().unwrap_or(false);
    let path = write_note_of(a, abs, s(args, "name"), body, append, main)?;
    Ok(json!({ "written": path }))
}

fn create_note(a: &mut Account, args: &Value) -> Result<Value> {
    let title = s(args, "title").unwrap_or_default().trim();
    if title.is_empty() {
        bail!("give the note a title");
    }
    let base = slug(title);
    let mut path = format!("{NOTES_UNLINKED_SUBDIR}/{base}.md");
    let mut n = 2;
    while a.db()?.note(&path)?.is_some() {
        path = format!("{NOTES_UNLINKED_SUBDIR}/{base}-{n}.md");
        n += 1;
    }
    let body = s(args, "body").unwrap_or_default();
    let full = if body.trim_start().starts_with('#') {
        format!("{}\n", body.trim_end())
    } else {
        format!("# {title}\n\n{}\n", body.trim_end())
    };
    a.db_mut()?.put_note(&path, &full)?;
    Ok(json!({ "created": path }))
}

fn update_note(a: &mut Account, args: &Value) -> Result<Value> {
    let path = note_path(s(args, "path").unwrap_or_default())?;
    let old = a
        .db()?
        .note(&path)?
        .ok_or_else(|| anyhow!("no note at {path}"))?;
    let body = s(args, "body").unwrap_or_default();
    let new = if s(args, "mode") == Some("append") {
        let sep = if old.ends_with('\n') { "" } else { "\n" };
        format!("{old}{sep}{body}\n")
    } else {
        format!("{}\n", body.trim_end())
    };
    a.db_mut()?.put_note(&path, &new)?;
    Ok(json!({ "updated": path }))
}

fn delete_note(a: &mut Account, args: &Value) -> Result<Value> {
    let path = note_path(s(args, "path").unwrap_or_default())?;
    if !a.db_mut()?.delete_note(&path)? {
        bail!("no note at {path}");
    }
    // A task whose main note it was has none now.
    let mut parts = path.split('/');
    if parts.next() == Some(NOTES_TASKS_SUBDIR)
        && let (Some(folder), Some(name)) = (parts.next(), parts.next())
        && let Some(t) = a.task_with_folder(folder).cloned()
        && todo::find_kv(&t.raw, MAIN_KEY).as_deref() == Some(name)
    {
        let abs = a.find(&t.id)?;
        a.edit(abs, &series::set_kv(&t.raw, MAIN_KEY, None))?;
    }
    Ok(json!({ "deleted": path }))
}

fn create_space(a: &mut Account, args: &Value) -> Result<Value> {
    let path = resolve_space(s(args, "path").unwrap_or_default(), &[])?;
    match a.store.create_space(&path) {
        SpaceSettingOutcome::Done => Ok(json!({ "created": path })),
        other => Err(anyhow!("couldn't make it: {other:?}")),
    }
}

fn update_space(a: &mut Account, args: &Value) -> Result<Value> {
    let known = a.space_paths();
    let given = s(args, "space").unwrap_or_default();
    let mut path = resolve_space(given, &known)?;
    if !known.contains(&path) {
        bail!("no space called {given:?}");
    }
    let mut did = Vec::new();
    if let Some(to) = s(args, "rename_to").filter(|t| !t.trim().is_empty()) {
        let to = resolve_space(to, &[])?;
        match a.store.rename_project(&path, &to) {
            RenameOutcome::Done { .. }
            | RenameOutcome::NothingToRename
            | RenameOutcome::Unchanged => {}
            other => bail!("couldn't rename it: {other:?}"),
        }
        did.push(format!("renamed to {to}"));
        path = to;
    }
    if let Some(c) = s(args, "color") {
        let color = if c.trim().is_empty() {
            None
        } else {
            Some(
                SpaceColor::parse(c.trim())
                    .ok_or_else(|| anyhow!("a colour like #8aadf4, or a palette number"))?,
            )
        };
        match a.store.set_space_color(&path, color) {
            SpaceSettingOutcome::Done => did.push("colour set".into()),
            other => bail!("couldn't colour it: {other:?}"),
        }
    }
    if let Some(h) = args["hidden"].as_bool() {
        match a.store.set_space_hidden(&path, h) {
            SpaceSettingOutcome::Done => did.push(if h { "hidden" } else { "shown" }.into()),
            other => bail!("couldn't hide it: {other:?}"),
        }
    }
    Ok(json!({ "space": path, "done": did }))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn titles_change_and_the_markup_stays() {
        assert_eq!(
            retitle(
                "(A) 2026-11-01 Call mom +Home @phone due:2026-11-09",
                "Call dad"
            ),
            "(A) 2026-11-01 Call dad +Home @phone due:2026-11-09"
        );
        assert_eq!(set_priority("(A) Call", Some('C')), "(C) Call");
        assert_eq!(set_priority("Call", None), "Call");
    }

    #[test]
    fn fields_apply() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 9).unwrap();
        let known = vec!["Uni/Exams".to_string(), "Work".to_string()];
        let raw = apply(
            "2026-11-09 Study",
            &json!({
                "priority": "high", "space": "exams", "tags": ["deep"], "date": "2026-11-12",
                "time": "6pm", "duration": "1h30m", "repeat": "every monday", "starred": true,
            }),
            today,
            &known,
        )
        .unwrap();
        let t = todo::parse_line(&raw).unwrap();
        assert_eq!(t.priority, Some('A'));
        assert_eq!(t.projects, ["Uni/Exams"]);
        assert_eq!(t.contexts, ["deep"]);
        assert_eq!(t.planned.as_deref(), Some("2026-11-12"));
        assert!(
            raw.contains("at:18:00") && raw.contains("dur:1h30m"),
            "{raw}"
        );
        assert!(t.rec.is_some() && t.starred);
        let cleared = apply(
            &raw,
            &json!({ "space": "", "time": "", "repeat": "" }),
            today,
            &known,
        )
        .unwrap();
        assert!(!cleared.contains('+') && !cleared.contains("at:") && !cleared.contains("rec:"));
        assert!(note_path("../etc/passwd").is_err());
        assert!(note_path("unlinked/a.md").is_ok());
        assert_eq!(slug("Reunión con María!"), "reunion-con-maria");
    }
}
