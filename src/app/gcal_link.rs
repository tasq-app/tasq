//! Google Calendar from the app: connecting, and keeping the "tasq"
//! calendar in step as you work. All of it runs on a thread of its own —
//! the browser, the network — and reports back; the app never waits.
//!
//! A change to the tasks asks for a sync a couple of seconds later (so a
//! burst of edits is one sync); only what changed goes (see
//! [`crate::gcal::sync`]).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use serde_json::Value;

use super::App;
use crate::gcal::{self, auth, google::Google, sync};

/// How long after a change the sync goes.
const SETTLE: Duration = Duration::from_secs(2);

/// Where Google Calendar stands, as Settings shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GcalStatus {
    /// Not connected.
    Off,
    /// Waiting for the browser.
    Connecting,
    /// Connected; the last sync's outcome, if any yet.
    Connected { email: String, last: Option<String> },
    /// Something went wrong (kept until the next try).
    Failed(String),
}

enum Job {
    SignIn,
    Sync(BTreeMap<String, Value>, String),
    Disconnect,
}

enum Note {
    Connected(String),
    Synced(sync::Report),
    Disconnected,
    Failed(String),
}

/// The thread that talks to Google, and its mailbox.
pub struct GcalLink {
    jobs: Sender<Job>,
    notes: Receiver<Note>,
}

fn state_path() -> Option<PathBuf> {
    crate::xdg::data_home().map(|d| d.join("tasq").join("google-sync.json"))
}

fn load_state() -> sync::SyncState {
    state_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|s| sync::SyncState::from_json(&s))
        .unwrap_or_default()
}

fn save_state(st: &sync::SyncState) {
    if let Some(p) = state_path() {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(p, st.to_json());
    }
}

impl GcalLink {
    /// Start the thread. `client` is the app's OAuth client.
    fn start(client: auth::Client) -> Self {
        let (jobs, job_rx) = mpsc::channel::<Job>();
        let (note_tx, notes) = mpsc::channel::<Note>();
        std::thread::spawn(move || {
            let mut api: Option<Google> =
                auth::load().map(|account| Google::new(client.clone(), account, None));
            let mut state = load_state();
            while let Ok(mut job) = job_rx.recv() {
                // Only the latest sync matters: skip the ones behind it.
                while let Ok(next) = job_rx.try_recv() {
                    match (&job, &next) {
                        (Job::Sync(..), Job::Sync(..)) => job = next,
                        _ => {
                            Self::run(&client, &mut api, &mut state, job, &note_tx);
                            job = next;
                        }
                    }
                }
                Self::run(&client, &mut api, &mut state, job, &note_tx);
            }
        });
        Self { jobs, notes }
    }

    fn run(
        client: &auth::Client,
        api: &mut Option<Google>,
        state: &mut sync::SyncState,
        job: Job,
        out: &Sender<Note>,
    ) {
        let note = match job {
            Job::SignIn => match auth::sign_in(client, &|_| {}) {
                Ok((account, access)) => match auth::store(&account) {
                    Ok(()) => {
                        let email = account.email.clone();
                        *api = Some(Google::new(client.clone(), account, Some(access)));
                        // A new account starts a new calendar.
                        *state = sync::SyncState::default();
                        save_state(state);
                        Note::Connected(email)
                    }
                    Err(e) => Note::Failed(format!("couldn't keep the access: {e}")),
                },
                Err(e) => Note::Failed(e.to_string()),
            },
            Job::Sync(desired, tz) => match api.as_mut() {
                Some(g) => match sync::sync(g, state, &desired, &tz) {
                    Ok(report) => {
                        save_state(state);
                        Note::Synced(report)
                    }
                    Err(e) => {
                        save_state(state);
                        Note::Failed(e.to_string())
                    }
                },
                None => return,
            },
            Job::Disconnect => {
                if let Some(account) = auth::load() {
                    auth::revoke(&account);
                }
                auth::forget();
                *api = None;
                *state = sync::SyncState::default();
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
    /// The OAuth client to use, if this build or the config has one.
    fn gcal_client(&self) -> Option<auth::Client> {
        auth::Client::find(
            self.google_client.0.as_deref(),
            self.google_client.1.as_deref(),
        )
    }

    /// At startup: a kept connection comes back on, and a sync goes.
    pub fn gcal_resume(&mut self) {
        // A todo.txt opened directly has no task ids to follow: no sync.
        if !self.is_db() {
            return;
        }
        let Some(email) = auth::kept_email() else {
            return;
        };
        let Some(client) = self.gcal_client() else {
            return;
        };
        self.gcal = Some(GcalLink::start(client));
        self.gcal_status = GcalStatus::Connected { email, last: None };
        self.gcal_due = Some(Instant::now());
    }

    /// "Connect Google Calendar": the browser opens on Google's consent.
    pub fn gcal_connect(&mut self) {
        if !self.is_db() {
            self.flash("Google Calendar needs the database (tasks keep their ids there)");
            return;
        }
        let Some(client) = self.gcal_client() else {
            self.flash("this build has no Google client · see the README to add your own");
            return;
        };
        let link = self.gcal.get_or_insert_with(|| GcalLink::start(client));
        let _ = link.jobs.send(Job::SignIn);
        self.gcal_status = GcalStatus::Connecting;
        self.flash("finish in your browser · allow tasq, then come back");
    }

    pub fn gcal_disconnect(&mut self) {
        if let Some(link) = &self.gcal {
            let _ = link.jobs.send(Job::Disconnect);
        }
        self.gcal_status = GcalStatus::Off;
        self.gcal_due = None;
    }

    /// "Sync now", or after a change: a sync soon (a burst of changes is
    /// one sync).
    pub fn gcal_touch(&mut self, now: bool) {
        if matches!(self.gcal_status, GcalStatus::Connected { .. })
            || matches!(self.gcal_status, GcalStatus::Failed(_)) && self.gcal.is_some()
        {
            self.gcal_due = Some(if now {
                Instant::now()
            } else {
                Instant::now() + SETTLE
            });
        }
    }

    /// When a sync is waiting, for the event loop's timeout.
    pub fn gcal_deadline(&self) -> Option<Instant> {
        self.gcal_due
    }

    /// Every loop: send a sync that's due, and take in what the thread
    /// says. Returns whether anything shown changed.
    pub fn gcal_poll(&mut self) -> bool {
        let mut changed = false;
        if let Some(due) = self.gcal_due
            && due <= Instant::now()
            && self.is_db()
            && let Some(link) = &self.gcal
        {
            let rgb = |p: &str| -> Option<(u8, u8, u8)> {
                Some(crate::ui::mode_colors::rgb(self.space_color(p)))
            };
            let tz = gcal::event::local_time_zone();
            let desired = gcal::desired(self.store.tasks(), &rgb, &tz);
            let _ = link.jobs.send(Job::Sync(desired, tz));
            self.gcal_due = None;
        }
        let Some(link) = &self.gcal else {
            return false;
        };
        let mut notes = Vec::new();
        while let Ok(n) = link.notes.try_recv() {
            notes.push(n);
        }
        for note in notes {
            changed = true;
            match note {
                Note::Connected(email) => {
                    self.flash(format!("Google Calendar connected · {email}"));
                    self.gcal_status = GcalStatus::Connected { email, last: None };
                    self.gcal_due = Some(Instant::now());
                }
                Note::Synced(r) => {
                    let email = match &self.gcal_status {
                        GcalStatus::Connected { email, .. } => email.clone(),
                        _ => auth::kept_email().unwrap_or_default(),
                    };
                    let at = chrono::Local::now().format("%H:%M").to_string();
                    let last = if r.errors.is_empty() {
                        format!("synced {at}")
                    } else {
                        format!("synced {at} · {} failed", r.errors.len())
                    };
                    self.gcal_status = GcalStatus::Connected {
                        email,
                        last: Some(last),
                    };
                }
                Note::Disconnected => {
                    self.flash("Google Calendar disconnected");
                    self.gcal_status = GcalStatus::Off;
                }
                Note::Failed(why) => {
                    self.flash(format!("Google Calendar: {why}"));
                    self.gcal_status = GcalStatus::Failed(why);
                }
            }
        }
        changed
    }

    /// What Settings shows for Google Calendar.
    pub fn gcal_label(&self) -> String {
        match &self.gcal_status {
            GcalStatus::Off if self.gcal_client().is_none() => {
                "needs a Google client (see README)".to_string()
            }
            GcalStatus::Off => "not connected · Enter connects".to_string(),
            GcalStatus::Connecting => "waiting for the browser…".to_string(),
            GcalStatus::Connected { email, last } => match last {
                Some(l) => format!("● {email} · {l} · Enter syncs now"),
                None => format!("● {email} · syncing…"),
            },
            GcalStatus::Failed(why) => format!("⚠ {why} · Enter retries"),
        }
    }
}
