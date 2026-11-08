//! Sync from the app: signing in (or making an account) from Settings, and
//! keeping this computer in step in the background — at start, a few
//! seconds after a change, and every minute for what the others sent. The
//! network and the key-making run on a thread of their own; the app never
//! waits. The thread writes through its own database connection, so the
//! app sees another writer and reloads, as with the CLI.

use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use super::App;
use crate::cloud::client::{Http, Remote};
use crate::cloud::keys::{Keys, normalize_email};
use crate::cloud::run::{Report, sync_once};
use crate::cloud::session::{self, Session};
use crate::core::db::Db;

/// After a change, how long until it goes (a burst of edits is one sync).
const SETTLE: Duration = Duration::from_secs(3);
/// How often to look for what the other computers sent.
const EVERY: Duration = Duration::from_secs(60);

/// Where sync stands, as Settings shows it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SyncStatus {
    /// Not signed in.
    #[default]
    Off,
    /// Signing in, or a first sync.
    Working,
    /// Signed in; when it last synced, if yet.
    On { email: String, last: Option<String> },
    /// The last try failed (signed in still, if a session is kept).
    Failed(String),
}

/// What the email / password prompt in Settings is asking for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncStep {
    Email,
    Password,
    /// The email has no account yet: the password again, to make one.
    Confirm,
}

/// The sign-in being typed in Settings. The password lives here only
/// until it's sent to the sync thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncInput {
    pub step: SyncStep,
    pub email: String,
    pub password: String,
    pub buffer: String,
}

enum Job {
    SignIn {
        server: String,
        email: String,
        password: String,
        create: bool,
    },
    Sync,
    SignOut,
}

enum Note {
    /// No account for that email: ask to make one.
    NeedsAccount,
    SignedIn(String),
    Synced(Report),
    Failed(String),
    SignedOut,
}

pub struct SyncLink {
    jobs: Sender<Job>,
    notes: Receiver<Note>,
}

fn open_db() -> anyhow::Result<Db> {
    let path = crate::cli::db_path().ok_or_else(|| anyhow::anyhow!("no data directory"))?;
    Ok(Db::open(&path)?)
}

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

impl SyncLink {
    fn start() -> Self {
        let (jobs, job_rx) = mpsc::channel::<Job>();
        let (note_tx, notes) = mpsc::channel::<Note>();
        std::thread::spawn(move || {
            let mut session = Session::load();
            while let Ok(mut job) = job_rx.recv() {
                // A run of syncs is one sync.
                while let Ok(next) = job_rx.try_recv() {
                    match (&job, &next) {
                        (Job::Sync, Job::Sync) => {}
                        _ => {
                            Self::run(&mut session, job, &note_tx);
                            job = next;
                        }
                    }
                }
                Self::run(&mut session, job, &note_tx);
            }
        });
        Self { jobs, notes }
    }

    fn round(session: &Session) -> anyhow::Result<Report> {
        let mut db = open_db()?;
        let mut remote = Http::new(&session.server, session.keys.clone());
        sync_once(&mut db, &mut remote, &session.keys, &today())
    }

    fn run(session: &mut Option<Session>, job: Job, out: &Sender<Note>) {
        let send = |n: Note| {
            let _ = out.send(n);
        };
        match job {
            Job::SignIn {
                server,
                email,
                password,
                create,
            } => {
                let signed = (|| -> anyhow::Result<Option<Session>> {
                    let keys = Keys::derive(&email, &password)?;
                    let mut remote = Http::new(&server, keys.clone());
                    match remote.check() {
                        Ok(()) => {}
                        Err(e) if e.to_string().contains("no such account") => {
                            if !create {
                                return Ok(None);
                            }
                            remote.signup()?;
                        }
                        Err(e) => return Err(e),
                    }
                    // A fresh start for this computer: a new name, and
                    // everything sent again.
                    let mut db = open_db()?;
                    db.sync_forget()?;
                    db.sync_reset_node()?;
                    let s = Session {
                        server,
                        email: normalize_email(&email),
                        keys,
                    };
                    s.save()?;
                    Ok(Some(s))
                })();
                match signed {
                    Ok(None) => send(Note::NeedsAccount),
                    Ok(Some(s)) => {
                        send(Note::SignedIn(s.email.clone()));
                        match Self::round(&s) {
                            Ok(r) => send(Note::Synced(r)),
                            Err(e) => send(Note::Failed(e.to_string())),
                        }
                        *session = Some(s);
                    }
                    Err(e) => send(Note::Failed(e.to_string())),
                }
            }
            Job::Sync => {
                let Some(s) = session.as_ref() else { return };
                match Self::round(s) {
                    Ok(r) => send(Note::Synced(r)),
                    Err(e) => send(Note::Failed(e.to_string())),
                }
            }
            Job::SignOut => {
                Session::forget();
                if let Ok(mut db) = open_db() {
                    let _ = db.sync_forget();
                }
                *session = None;
                send(Note::SignedOut);
            }
        }
    }
}

impl App {
    fn sync_link(&mut self) -> &SyncLink {
        self.sync.get_or_insert_with(SyncLink::start)
    }

    /// At start: a kept sign-in comes back on, and a sync goes.
    pub fn sync_resume(&mut self) {
        if !self.is_db() {
            return;
        }
        let Some((_, email)) = Session::signed_in_as() else {
            return;
        };
        self.sync_link();
        self.sync_status = SyncStatus::On { email, last: None };
        self.sync_due = Some(Instant::now());
    }

    /// Whether this computer is signed in to sync.
    pub fn sync_signed_in(&self) -> bool {
        matches!(self.sync_status, SyncStatus::On { .. })
            || matches!(self.sync_status, SyncStatus::Failed(_))
                && Session::signed_in_as().is_some()
    }

    /// "Sync now", or after a change: a sync soon.
    pub fn sync_touch(&mut self, now: bool) {
        if self.sync.is_some() && self.sync_signed_in() {
            let at = Instant::now() + if now { Duration::ZERO } else { SETTLE };
            self.sync_due = Some(self.sync_due.map_or(at, |d| d.min(at)));
        }
    }

    pub fn sync_deadline(&self) -> Option<Instant> {
        self.sync_due
    }

    /// Start typing the email (Settings → Sync).
    pub fn sync_begin(&mut self) {
        if !self.is_db() {
            self.flash("sync needs the database (tasks keep their ids there)");
            return;
        }
        self.sync_input = Some(SyncInput {
            step: SyncStep::Email,
            email: String::new(),
            password: String::new(),
            buffer: String::new(),
        });
    }

    pub fn sync_input_char(&mut self, c: char) {
        if let Some(i) = self.sync_input.as_mut() {
            i.buffer.push(c);
        }
    }

    pub fn sync_input_paste(&mut self, s: &str) {
        if let Some(i) = self.sync_input.as_mut() {
            i.buffer.push_str(s.trim_end_matches(['\n', '\r']));
        }
    }

    pub fn sync_input_backspace(&mut self) {
        if let Some(i) = self.sync_input.as_mut() {
            i.buffer.pop();
        }
    }

    pub fn sync_input_cancel(&mut self) {
        self.sync_input = None;
    }

    /// Enter: the email, then the password (sign in), then — for an email
    /// with no account yet — the password again (make it).
    pub fn sync_input_submit(&mut self) {
        let Some(mut i) = self.sync_input.take() else {
            return;
        };
        let text = std::mem::take(&mut i.buffer);
        match i.step {
            SyncStep::Email => {
                let email = normalize_email(&text);
                if !email.contains('@') {
                    self.flash("that doesn't look like an email");
                    i.buffer = text;
                    self.sync_input = Some(i);
                    return;
                }
                i.email = email;
                i.step = SyncStep::Password;
                self.sync_input = Some(i);
            }
            SyncStep::Password => {
                if text.chars().count() < 10 {
                    self.flash("at least 10 characters");
                    self.sync_input = Some(i);
                    return;
                }
                i.password = text;
                self.sync_send_sign_in(&i, false);
                // Kept (with the password) in case the account is new.
                self.sync_pending_input = Some(i);
            }
            SyncStep::Confirm => {
                if text != i.password {
                    self.flash("the passwords don't match · try again");
                    self.sync_input = Some(i);
                    return;
                }
                self.sync_send_sign_in(&i, true);
            }
        }
    }

    fn sync_send_sign_in(&mut self, i: &SyncInput, create: bool) {
        let server = session::server(self.sync_url.as_deref());
        let _ = self.sync_link().jobs.send(Job::SignIn {
            server,
            email: i.email.clone(),
            password: i.password.clone(),
            create,
        });
        self.sync_status = SyncStatus::Working;
        self.flash(if create {
            "making your account…"
        } else {
            "signing in…"
        });
    }

    pub fn sync_sign_out(&mut self) {
        let _ = self.sync_link().jobs.send(Job::SignOut);
        self.sync_status = SyncStatus::Off;
        self.sync_due = None;
    }

    /// Every loop: send a sync that's due, take in what the thread says.
    pub fn sync_poll(&mut self) -> bool {
        if let Some(due) = self.sync_due
            && due <= Instant::now()
            && self.is_db()
            && let Some(link) = &self.sync
        {
            let _ = link.jobs.send(Job::Sync);
            self.sync_due = None;
        }
        let Some(link) = &self.sync else {
            return false;
        };
        let mut notes = Vec::new();
        while let Ok(n) = link.notes.try_recv() {
            notes.push(n);
        }
        let changed = !notes.is_empty();
        for note in notes {
            match note {
                Note::NeedsAccount => {
                    self.sync_status = SyncStatus::Off;
                    if let Some(mut i) = self.sync_pending_input.take() {
                        i.step = SyncStep::Confirm;
                        self.sync_input = Some(i);
                    }
                    self.flash(
                        "no account for that email yet · type the password again to create it",
                    );
                }
                Note::SignedIn(email) => {
                    self.sync_pending_input = None;
                    self.flash(format!("signed in as {email} · syncing…"));
                    self.sync_status = SyncStatus::On { email, last: None };
                }
                Note::Synced(r) => {
                    let email = match &self.sync_status {
                        SyncStatus::On { email, .. } => email.clone(),
                        _ => Session::signed_in_as().map(|s| s.1).unwrap_or_default(),
                    };
                    let at = chrono::Local::now().format("%H:%M").to_string();
                    self.sync_status = SyncStatus::On {
                        email,
                        last: Some(format!("synced {at}")),
                    };
                    if r.applied > 0 {
                        // The other computers' changes are in the database:
                        // take them in now, not at the next tick.
                        self.check_external_changes();
                        self.store.reload_spaces();
                        self.ext_reconcile_shared();
                        self.recompute_visible();
                        self.gcal_touch(false);
                    }
                    self.sync_due = Some(Instant::now() + EVERY);
                }
                Note::Failed(why) => {
                    self.sync_pending_input = None;
                    self.flash(format!("sync: {why}"));
                    self.sync_status = SyncStatus::Failed(why);
                    // Try again later (offline, say): in a minute.
                    if Session::signed_in_as().is_some() {
                        self.sync_due = Some(Instant::now() + EVERY);
                    }
                }
                Note::SignedOut => {
                    self.flash("signed out · your tasks stay on this computer");
                    self.sync_status = SyncStatus::Off;
                }
            }
        }
        changed
    }

    /// What Settings shows on the sync row.
    pub fn sync_label(&self) -> String {
        if let Some(i) = &self.sync_input {
            let dots = "•".repeat(i.buffer.chars().count());
            return match i.step {
                SyncStep::Email => format!("email: {}▏", i.buffer),
                SyncStep::Password => format!("{} · password: {dots}▏", i.email),
                SyncStep::Confirm => format!("new account · password again: {dots}▏"),
            };
        }
        match &self.sync_status {
            SyncStatus::Off => "sign in or create an account".to_string(),
            SyncStatus::Working => "signing in…".to_string(),
            SyncStatus::On { email, last } => match last {
                Some(l) => format!("● {email} · {l}"),
                None => format!("● {email} · syncing…"),
            },
            SyncStatus::Failed(why) => format!("⚠ {why}"),
        }
    }
}

/// The marker that the first-start question was answered on this computer.
fn onboarded_path() -> Option<std::path::PathBuf> {
    crate::xdg::data_home().map(|d| d.join("tasq").join("welcomed"))
}

/// Don't ask the first-start question again on this computer.
pub fn mark_onboarded() {
    if let Some(p) = onboarded_path() {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(p, "");
    }
}

impl App {
    /// Whether to ask "sign in, or carry on here?": a database with no task
    /// in it at all, not signed in, and not asked before.
    pub fn should_onboard(&self) -> bool {
        self.is_db()
            && self.store.tasks().is_empty()
            && self.store.archive().tasks.is_empty()
            && Session::signed_in_as().is_none()
            && onboarded_path().is_some_and(|p| !p.exists())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::app::test_support::build_app;

    #[test]
    fn the_sign_in_prompt_asks_email_then_password() {
        let mut app = build_app("a\n");
        app.sync_input = Some(SyncInput {
            step: SyncStep::Email,
            email: String::new(),
            password: String::new(),
            buffer: String::new(),
        });
        for c in "Me@Example.com".chars() {
            app.sync_input_char(c);
        }
        assert_eq!(app.sync_label(), "email: Me@Example.com▏");
        app.sync_input_submit();
        assert_eq!(app.sync_input.as_ref().unwrap().email, "me@example.com");
        app.sync_input_paste("short");
        app.sync_input_submit();
        // Too short: still asking, nothing sent.
        assert_eq!(app.sync_input.as_ref().unwrap().step, SyncStep::Password);
        assert_eq!(app.sync_label(), "me@example.com · password: ▏");
        app.sync_input_cancel();
        assert_eq!(app.sync_label(), "sign in or create an account");
    }
}
