//! Apple Calendar from the app: signing in with an Apple ID and an
//! app-specific password, and keeping a calendar per space in step as you
//! work. Like Google, it all runs on a thread of its own and reports back.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use super::{App, GcalStatus};
use crate::gcal::apple::{self, AppleState, Wanted};
use crate::gcal::caldav::{CalDav, ICLOUD, Login};
use crate::gcal::{self, ics};

/// How long after a change the sync goes.
const SETTLE: Duration = Duration::from_secs(2);

/// Typing the Apple ID, then the app-specific password, in Settings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppleInput {
    /// The Apple ID, once given; until then the buffer is the Apple ID.
    pub user: Option<String>,
    pub buffer: String,
}

enum Job {
    SignIn(Login),
    Sync(BTreeMap<String, Wanted>, BTreeSet<String>),
    Disconnect,
}

enum Note {
    Connected(String),
    Synced(gcal::sync::Report),
    Disconnected,
    Failed(String),
}

pub struct AcalLink {
    jobs: Sender<Job>,
    notes: Receiver<Note>,
}

fn state_path() -> Option<PathBuf> {
    crate::xdg::data_home().map(|d| d.join("tasq").join("apple-sync.json"))
}

fn load_state() -> AppleState {
    state_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|s| AppleState::from_json(&s))
        .unwrap_or_default()
}

fn save_state(st: &AppleState) {
    if let Some(p) = state_path() {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(p, st.to_json());
    }
}

impl AcalLink {
    fn start() -> Self {
        let (jobs, job_rx) = mpsc::channel::<Job>();
        let (note_tx, notes) = mpsc::channel::<Note>();
        std::thread::spawn(move || {
            let mut dav: Option<CalDav> = apple::load().map(CalDav::new);
            let mut state = load_state();
            while let Ok(mut job) = job_rx.recv() {
                // Only the latest sync matters.
                while let Ok(next) = job_rx.try_recv() {
                    match (&job, &next) {
                        (Job::Sync(..), Job::Sync(..)) => job = next,
                        _ => {
                            Self::run(&mut dav, &mut state, job, &note_tx);
                            job = next;
                        }
                    }
                }
                Self::run(&mut dav, &mut state, job, &note_tx);
            }
        });
        Self { jobs, notes }
    }

    fn run(dav: &mut Option<CalDav>, state: &mut AppleState, job: Job, out: &Sender<Note>) {
        let note = match job {
            Job::SignIn(login) => {
                let mut d = CalDav::new(login.clone());
                match d.check().and_then(|()| apple::store(&login)) {
                    Ok(()) => {
                        *dav = Some(d);
                        // A new account starts new calendars.
                        *state = AppleState::default();
                        save_state(state);
                        Note::Connected(login.user)
                    }
                    Err(e) => Note::Failed(e.to_string()),
                }
            }
            Job::Sync(desired, spaces) => match dav.as_mut() {
                Some(d) => {
                    let r = apple::sync(d, state, &desired, &spaces, &ics::stamp_now());
                    save_state(state);
                    match r {
                        Ok(report) => Note::Synced(report),
                        Err(e) => Note::Failed(e.to_string()),
                    }
                }
                None => return,
            },
            Job::Disconnect => {
                apple::forget();
                *dav = None;
                *state = AppleState::default();
                if let Some(p) = state_path() {
                    let _ = std::fs::remove_file(p);
                }
                Note::Disconnected
            }
        };
        let _ = out.send(note);
    }
}

impl App {
    /// At startup: a kept Apple account comes back on, and a sync goes.
    pub fn acal_resume(&mut self) {
        if !self.is_db() {
            return;
        }
        let Some((_, user)) = apple::kept_user() else {
            return;
        };
        self.acal = Some(AcalLink::start());
        self.acal_status = GcalStatus::Connected {
            email: user,
            last: None,
        };
        self.acal_due = Some(Instant::now());
    }

    /// "Connect Apple Calendar": ask for the Apple ID, then the password.
    pub fn acal_begin(&mut self) {
        if !self.is_db() {
            self.flash("Apple Calendar needs the database (tasks keep their ids there)");
            return;
        }
        self.apple_input = Some(AppleInput::default());
    }

    /// A key typed while the Apple ID or password is being asked for.
    pub fn apple_input_char(&mut self, c: char) {
        if let Some(i) = self.apple_input.as_mut() {
            i.buffer.push(c);
        }
    }

    pub fn apple_input_paste(&mut self, s: &str) {
        if let Some(i) = self.apple_input.as_mut() {
            i.buffer.push_str(s.trim());
        }
    }

    pub fn apple_input_backspace(&mut self) {
        if let Some(i) = self.apple_input.as_mut() {
            i.buffer.pop();
        }
    }

    pub fn apple_input_cancel(&mut self) {
        self.apple_input = None;
    }

    /// Enter: the Apple ID is kept and the password asked for; with the
    /// password, sign in.
    pub fn apple_input_submit(&mut self) {
        let Some(mut i) = self.apple_input.take() else {
            return;
        };
        let text = i.buffer.trim().to_string();
        if text.is_empty() {
            self.apple_input = Some(i);
            return;
        }
        match i.user.take() {
            None => {
                self.apple_input = Some(AppleInput {
                    user: Some(text),
                    buffer: String::new(),
                });
            }
            Some(user) => {
                let server = self
                    .caldav_url
                    .clone()
                    .unwrap_or_else(|| ICLOUD.to_string());
                let link = self.acal.get_or_insert_with(AcalLink::start);
                let _ = link.jobs.send(Job::SignIn(Login {
                    server,
                    user,
                    password: text,
                }));
                self.acal_status = GcalStatus::Connecting;
                self.flash("signing in to Apple Calendar…");
            }
        }
    }

    pub fn acal_disconnect(&mut self) {
        if let Some(link) = &self.acal {
            let _ = link.jobs.send(Job::Disconnect);
        }
        self.acal_status = GcalStatus::Off;
        self.acal_due = None;
    }

    /// "Sync now", or after a change: a sync soon.
    pub fn acal_touch(&mut self, now: bool) {
        if self.acal.is_some()
            && matches!(
                self.acal_status,
                GcalStatus::Connected { .. } | GcalStatus::Failed(_)
            )
        {
            self.acal_due = Some(if now {
                Instant::now()
            } else {
                Instant::now() + SETTLE
            });
        }
    }

    pub fn acal_deadline(&self) -> Option<Instant> {
        self.acal_due
    }

    /// Every loop: send a sync that's due, take in what the thread says.
    pub fn acal_poll(&mut self) -> bool {
        if let Some(due) = self.acal_due
            && due <= Instant::now()
            && self.is_db()
            && let Some(link) = &self.acal
        {
            let rgb = |p: &str| -> Option<(u8, u8, u8)> {
                Some(crate::ui::mode_colors::rgb(self.space_color(p)))
            };
            let tz = gcal::event::local_time_zone();
            let desired = apple::desired(self.store.tasks(), &rgb, &tz);
            let spaces: BTreeSet<String> = self
                .store
                .known_spaces()
                .iter()
                .filter_map(|s| s.path.split('/').next())
                .chain(
                    self.store
                        .tasks()
                        .iter()
                        .filter_map(|t| t.projects.first()?.split('/').next()),
                )
                .map(str::to_string)
                .collect();
            let _ = link.jobs.send(Job::Sync(desired, spaces));
            self.acal_due = None;
        }
        let Some(link) = &self.acal else {
            return false;
        };
        let mut notes = Vec::new();
        while let Ok(n) = link.notes.try_recv() {
            notes.push(n);
        }
        let changed = !notes.is_empty();
        for note in notes {
            match note {
                Note::Connected(user) => {
                    self.flash(format!("Apple Calendar connected · {user}"));
                    self.acal_status = GcalStatus::Connected {
                        email: user,
                        last: None,
                    };
                    self.acal_due = Some(Instant::now());
                }
                Note::Synced(r) => {
                    let email = match &self.acal_status {
                        GcalStatus::Connected { email, .. } => email.clone(),
                        _ => apple::kept_user().map(|u| u.1).unwrap_or_default(),
                    };
                    let at = chrono::Local::now().format("%H:%M").to_string();
                    let last = if r.errors.is_empty() {
                        format!("synced {at}")
                    } else {
                        format!("synced {at} · {} failed", r.errors.len())
                    };
                    self.acal_status = GcalStatus::Connected {
                        email,
                        last: Some(last),
                    };
                }
                Note::Disconnected => {
                    self.flash("Apple Calendar disconnected");
                    self.acal_status = GcalStatus::Off;
                }
                Note::Failed(why) => {
                    self.flash(format!("Apple Calendar: {why}"));
                    self.acal_status = GcalStatus::Failed(why);
                }
            }
        }
        changed
    }

    /// What Settings shows for Apple Calendar.
    pub fn acal_label(&self) -> String {
        if let Some(i) = &self.apple_input {
            return match &i.user {
                None => format!("Apple ID: {}▏", i.buffer),
                Some(_) => format!(
                    "app-specific password: {}▏",
                    "•".repeat(i.buffer.chars().count())
                ),
            };
        }
        match &self.acal_status {
            GcalStatus::Off => "not connected · Enter connects".to_string(),
            GcalStatus::Connecting => "signing in…".to_string(),
            GcalStatus::Connected { email, last } => match last {
                Some(l) => format!("● {email} · {l} · Enter syncs now"),
                None => format!("● {email} · syncing…"),
            },
            GcalStatus::Failed(why) => format!("⚠ {why} · Enter retries"),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::app::test_support::build_app;

    #[test]
    fn the_apple_id_then_the_password_are_asked_for() {
        let mut app = build_app("a\n");
        // Not on a database: it says why instead.
        app.acal_begin();
        assert!(app.apple_input.is_none());
        app.apple_input = Some(Default::default());
        for c in "me@icloud.com".chars() {
            app.apple_input_char(c);
        }
        assert_eq!(app.acal_label(), "Apple ID: me@icloud.com▏");
        app.apple_input_submit();
        app.apple_input_paste(" abcd-efgh ");
        assert_eq!(app.acal_label(), "app-specific password: •••••••••▏");
        app.apple_input_cancel();
        assert_eq!(app.acal_label(), "not connected · Enter connects");
    }
}
