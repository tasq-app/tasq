//! Your other calendars from the app: connecting accounts, choosing their
//! calendars, and keeping their events current. Like Google Calendar's
//! link, it all runs on a thread of its own and reports back; the app
//! never waits.
//!
//! A round runs at startup, every few minutes, and when you ask (Enter on
//! an account in Settings, or showing a calendar). Each round is cheap:
//! only what changed comes (see [`crate::extcal::sync`]).

use std::collections::BTreeMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use super::App;
use crate::extcal::accounts::{self, ExtAccount};
use crate::extcal::cache::{self, Cache, Cached};
use crate::extcal::google::{self, GoogleEvents};
use crate::extcal::sync::{self as esync, CalendarInfo, EventsApi};
use crate::gcal::auth;

/// How often the events are brought up to date while tasq is open.
const EVERY: Duration = Duration::from_secs(5 * 60);

enum Job {
    SignIn,
    Refresh(Vec<ExtAccount>),
    Forget(String),
}

enum Note {
    Connected(String, Vec<CalendarInfo>),
    Calendars(String, Vec<CalendarInfo>),
    Synced(String, Option<String>),
    Forgotten,
    Events(Vec<Cached>),
    Failed(Option<String>, String),
}

/// The thread that reads the other calendars, and its mailbox.
pub struct ExtLink {
    jobs: Sender<Job>,
    notes: Receiver<Note>,
}

fn unix_now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// The events worth having in memory: the cache's whole window.
fn load_events(cache: &Cache) -> Vec<Cached> {
    let now = unix_now();
    let day = 86_400;
    cache
        .between(
            now - (esync::DAYS_BACK + 1) * day,
            now + (esync::DAYS_AHEAD + 1) * day,
        )
        .unwrap_or_default()
}

impl ExtLink {
    fn start(client: auth::Client) -> Self {
        let (jobs, job_rx) = mpsc::channel::<Job>();
        let (out, notes) = mpsc::channel::<Note>();
        std::thread::spawn(move || {
            let cache = cache::default_path()
                .ok_or_else(|| anyhow::anyhow!("no data directory"))
                .and_then(|p| Cache::open(&p));
            let mut cache = match cache {
                Ok(c) => c,
                Err(e) => {
                    let _ = out.send(Note::Failed(None, e.to_string()));
                    return;
                }
            };
            let _ = out.send(Note::Events(load_events(&cache)));
            while let Ok(mut job) = job_rx.recv() {
                // Only the latest refresh matters: skip the ones behind it.
                while let Ok(next) = job_rx.try_recv() {
                    match (&job, &next) {
                        (Job::Refresh(_), Job::Refresh(_)) => job = next,
                        _ => {
                            Self::run(&client, &mut cache, job, &out);
                            job = next;
                        }
                    }
                }
                Self::run(&client, &mut cache, job, &out);
            }
        });
        Self { jobs, notes }
    }

    fn api(client: &auth::Client, email: &str) -> Option<GoogleEvents> {
        let token = accounts::load_token(email)?;
        Some(GoogleEvents::new(
            client.clone(),
            auth::Account {
                email: email.to_string(),
                refresh_token: token,
            },
            None,
        ))
    }

    fn run(client: &auth::Client, cache: &mut Cache, job: Job, out: &Sender<Note>) {
        match job {
            Job::SignIn => {
                let signed = auth::sign_in_for(
                    client,
                    google::SCOPES,
                    "Your calendars are connected to tasq",
                    &|_| {},
                );
                let note = match signed {
                    Err(e) => Note::Failed(None, e.to_string()),
                    Ok((account, access)) => {
                        match accounts::store_token(&account.email, &account.refresh_token) {
                            Err(e) => Note::Failed(None, format!("couldn't keep the access: {e}")),
                            Ok(()) => {
                                let email = account.email.clone();
                                let mut api =
                                    GoogleEvents::new(client.clone(), account, Some(access));
                                match api.calendars() {
                                    Ok(list) => Note::Connected(email, list),
                                    Err(e) => Note::Failed(Some(email), e.to_string()),
                                }
                            }
                        }
                    }
                };
                let _ = out.send(note);
            }
            Job::Refresh(list) => {
                for mut account in list {
                    let email = account.email.clone();
                    let Some(mut api) = Self::api(client, &email) else {
                        let _ = out.send(Note::Failed(
                            Some(email),
                            "the access is gone · disconnect and connect again".to_string(),
                        ));
                        continue;
                    };
                    match api.calendars() {
                        Ok(cals) => {
                            account.refresh_calendars(&cals);
                            let _ = out.send(Note::Calendars(email.clone(), cals));
                        }
                        Err(e) => {
                            let _ = out.send(Note::Failed(Some(email), e.to_string()));
                            continue;
                        }
                    }
                    let mut error = None;
                    for c in &account.calendars {
                        if !c.shown {
                            let _ = cache.forget(&email, Some(&c.id));
                            continue;
                        }
                        if let Err(e) =
                            esync::sync_calendar(&mut api, cache, &email, &c.id, unix_now())
                        {
                            error = Some(format!("{}: {e}", c.name));
                        }
                    }
                    let _ = out.send(Note::Synced(email, error));
                }
                let _ = out.send(Note::Events(load_events(cache)));
            }
            Job::Forget(email) => {
                if let Some(token) = accounts::load_token(&email) {
                    auth::revoke(&auth::Account {
                        email: email.clone(),
                        refresh_token: token,
                    });
                }
                accounts::forget_token(&email);
                let _ = cache.forget(&email, None);
                let _ = out.send(Note::Forgotten);
                let _ = out.send(Note::Events(load_events(cache)));
            }
        }
    }
}

/// Where the other calendars stand, for Settings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExtState {
    pub accounts: Vec<ExtAccount>,
    /// Per account: the last round's outcome.
    pub status: BTreeMap<String, String>,
    /// Waiting for the browser.
    pub connecting: bool,
    /// The events of the calendars you show.
    pub events: Vec<Cached>,
}

impl App {
    fn ext_client(&self) -> Option<auth::Client> {
        auth::Client::find(
            self.google_client.0.as_deref(),
            self.google_client.1.as_deref(),
        )
    }

    fn ext_link(&mut self) -> Option<&ExtLink> {
        if self.ext.is_none() {
            let client = self.ext_client()?;
            self.ext = Some(ExtLink::start(client));
        }
        self.ext.as_ref()
    }

    /// At startup: the kept accounts come back, and a round goes.
    pub fn ext_resume(&mut self) {
        self.ext_state.accounts = accounts::load();
        if self.ext_state.accounts.is_empty() {
            return;
        }
        if self.ext_link().is_some() {
            self.ext_due = Some(Instant::now());
        }
    }

    /// "Add a Google account": the browser opens on Google's consent.
    pub fn ext_connect(&mut self) {
        if self.ext_state.connecting {
            self.flash("waiting for the browser…");
            return;
        }
        let Some(link) = self.ext_link() else {
            self.flash("this build has no Google client · see the README to add your own");
            return;
        };
        let _ = link.jobs.send(Job::SignIn);
        self.ext_state.connecting = true;
        self.flash("finish in your browser · allow tasq to see your calendars, then come back");
    }

    /// A round now (or soon, after a change of choice).
    pub fn ext_touch(&mut self) {
        if !self.ext_state.accounts.is_empty() {
            self.ext_due = Some(Instant::now());
        }
    }

    /// Show or hide calendar `cal` of account `acc`.
    pub fn ext_toggle_calendar(&mut self, acc: usize, cal: usize) {
        let Some(c) = self
            .ext_state
            .accounts
            .get_mut(acc)
            .and_then(|a| a.calendars.get_mut(cal))
        else {
            return;
        };
        c.shown = !c.shown;
        let shown = c.shown;
        let _ = accounts::save(&self.ext_state.accounts);
        if shown {
            self.ext_touch();
        }
    }

    /// Disconnect account `acc`: its access is revoked and its events go.
    pub fn ext_disconnect(&mut self, acc: usize) {
        if acc >= self.ext_state.accounts.len() {
            return;
        }
        let a = self.ext_state.accounts.remove(acc);
        let _ = accounts::save(&self.ext_state.accounts);
        self.ext_state.status.remove(&a.email);
        self.ext_state.events.retain(|e| e.account != a.email);
        if let Some(link) = self.ext_link() {
            let _ = link.jobs.send(Job::Forget(a.email.clone()));
        }
        self.flash(format!(
            "{} disconnected · its events are gone from tasq",
            a.email
        ));
    }

    pub fn ext_deadline(&self) -> Option<Instant> {
        self.ext_due
    }

    /// Every loop: send a round that's due, and take in what the thread
    /// says. Returns whether anything shown changed.
    pub fn ext_poll(&mut self) -> bool {
        if let Some(due) = self.ext_due
            && due <= Instant::now()
        {
            self.ext_due = None;
            let list = self.ext_state.accounts.clone();
            if !list.is_empty()
                && let Some(link) = self.ext_link()
            {
                let _ = link.jobs.send(Job::Refresh(list));
            }
        }
        let Some(link) = &self.ext else {
            return false;
        };
        let notes: Vec<Note> = link.notes.try_iter().collect();
        let changed = !notes.is_empty();
        for note in notes {
            match note {
                Note::Connected(email, cals) => {
                    self.ext_state.connecting = false;
                    let st = &mut self.ext_state;
                    let i = match st.accounts.iter().position(|a| a.email == email) {
                        Some(i) => i,
                        None => {
                            st.accounts.push(ExtAccount::new(&email));
                            st.accounts.len() - 1
                        }
                    };
                    st.accounts[i].refresh_calendars(&cals);
                    let _ = accounts::save(&st.accounts);
                    st.status.insert(email.clone(), "syncing…".to_string());
                    self.flash(format!(
                        "{email} connected · its main calendar shows in tasq"
                    ));
                    self.ext_due = Some(Instant::now());
                }
                Note::Calendars(email, cals) => {
                    if let Some(a) = self
                        .ext_state
                        .accounts
                        .iter_mut()
                        .find(|a| a.email == email)
                    {
                        a.refresh_calendars(&cals);
                        let _ = accounts::save(&self.ext_state.accounts);
                    }
                }
                Note::Synced(email, error) => {
                    let at = chrono::Local::now().format("%H:%M");
                    let s = match error {
                        None => format!("synced {at}"),
                        Some(e) => format!("⚠ {e}"),
                    };
                    self.ext_state.status.insert(email, s);
                    if self.ext_due.is_none() {
                        self.ext_due = Some(Instant::now() + EVERY);
                    }
                }
                Note::Forgotten => {}
                Note::Events(events) => self.ext_state.events = events,
                Note::Failed(email, why) => {
                    self.ext_state.connecting = false;
                    match email {
                        Some(e) => {
                            self.ext_state.status.insert(e, format!("⚠ {why}"));
                        }
                        None => self.flash(format!("Calendars: {why}")),
                    }
                    if self.ext_due.is_none() && !self.ext_state.accounts.is_empty() {
                        self.ext_due = Some(Instant::now() + EVERY);
                    }
                }
            }
        }
        changed
    }

    /// The events of the calendars shown now (a calendar just hidden
    /// drops out at once, before the thread clears it).
    pub fn ext_events(&self) -> impl Iterator<Item = &Cached> {
        self.ext_state.events.iter().filter(|e| {
            self.ext_state.accounts.iter().any(|a| {
                a.email == e.account && a.calendars.iter().any(|c| c.id == e.calendar && c.shown)
            })
        })
    }

    /// What Settings shows for account `acc`.
    pub fn ext_label(&self, acc: usize) -> String {
        let Some(a) = self.ext_state.accounts.get(acc) else {
            return String::new();
        };
        let shown = a.shown().count();
        let status = self
            .ext_state
            .status
            .get(&a.email)
            .cloned()
            .unwrap_or_else(|| "syncing…".to_string());
        format!(
            "● {} · {shown} of {} shown · {status} · Enter syncs",
            a.email,
            a.calendars.len()
        )
    }

    /// What Settings shows on "Add a Google account".
    pub fn ext_add_label(&self) -> String {
        if self.ext_state.connecting {
            "waiting for the browser…".to_string()
        } else if self.ext_client().is_none() {
            "needs a Google client (see README)".to_string()
        } else {
            "work or personal · read-only · Enter connects".to_string()
        }
    }
}

impl App {
    /// The palette slot of a calendar: one each, in the order they're
    /// listed, from the second half of the palette so they read apart from
    /// the spaces (which take the first eight).
    fn ext_slot(&self, account: &str, calendar: &str) -> usize {
        let k = self
            .ext_state
            .accounts
            .iter()
            .flat_map(|a| {
                a.calendars
                    .iter()
                    .map(move |c| (a.email.as_str(), c.id.as_str()))
            })
            .position(|x| x == (account, calendar))
            .unwrap_or(0);
        (crate::core::spaces::AUTO_SLOTS + k) % crate::core::spaces::PALETTE_SLOTS
    }

    /// The other calendars' events from `from` to `to`, as the calendar
    /// views draw them. Each gets an `abs` past the task list, its own,
    /// so it never stands for a task.
    pub fn cal_ext_occurrences(
        &self,
        from: chrono::NaiveDate,
        to: chrono::NaiveDate,
    ) -> Vec<crate::core::calendar::Occurrence> {
        use crate::core::calendar::{ExtMark, Occurrence};
        let events: Vec<Cached> = self.ext_events().cloned().collect();
        let pieces = match &self.frozen_tz {
            Some(tz) => crate::extcal::occur::occurrences(&events, from, to, tz),
            None => crate::extcal::occur::occurrences(&events, from, to, &chrono::Local),
        };
        pieces
            .into_iter()
            .map(|p| {
                let k = events
                    .iter()
                    .position(|e| {
                        e.account == p.account && e.calendar == p.calendar && e.event.id == p.id
                    })
                    .unwrap_or(0);
                let calendar = self
                    .ext_state
                    .accounts
                    .iter()
                    .find(|a| a.email == p.account)
                    .and_then(|a| a.calendars.iter().find(|c| c.id == p.calendar))
                    .map(|c| c.name.clone())
                    .unwrap_or_default();
                Occurrence {
                    abs: usize::MAX - k,
                    date: p.date,
                    start: p.start,
                    minutes: p.minutes,
                    deadline: false,
                    projected: false,
                    late: false,
                    origin: p.origin,
                    span: p.span,
                    day: p.day,
                    ext: Some(Box::new(ExtMark {
                        slot: self.ext_slot(&p.account, &p.calendar),
                        title: p.title,
                        tentative: p.tentative,
                        calendar,
                        account: p.account,
                        calendar_id: p.calendar,
                        id: p.id,
                    })),
                }
            })
            .collect()
    }

    /// Tasks and the other calendars' events together, for drawing.
    pub fn cal_view_occurrences(
        &self,
        from: chrono::NaiveDate,
        to: chrono::NaiveDate,
    ) -> Vec<crate::core::calendar::Occurrence> {
        let mut all = self.cal_occurrences(from, to);
        all.extend(self.cal_ext_occurrences(from, to));
        all.sort_by(|a, b| {
            (a.date, a.start.is_some(), a.start).cmp(&(b.date, b.start.is_some(), b.start))
        });
        all
    }
}

/// Another calendar's event, open on its details.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtDetail {
    pub event: Cached,
    /// Its calendar's name.
    pub calendar: String,
}

impl App {
    /// Open a link of the event in the details: `None` the video call,
    /// `Some(0)` the event in Google, `Some(n)` its n-th file. Google's
    /// links open in the event's own account.
    pub fn ext_detail_open(&mut self, which: Option<usize>) {
        let Some(d) = &self.ext_detail else {
            return;
        };
        let e = &d.event.event;
        let url = match which {
            None => e.meet_link.clone(),
            Some(0) => e.html_link.clone(),
            Some(n) => e.attachments.get(n - 1).map(|a| a.url.clone()),
        };
        let Some(url) = url else {
            self.flash(match which {
                None => "no video call in this one",
                _ => "nothing there",
            });
            return;
        };
        let url = crate::extcal::as_account(&url, &d.event.account);
        match auth::open_browser(&url) {
            Ok(()) => self.flash("opening in your browser…"),
            Err(_) => self.flash(format!("couldn't open the browser · {url}")),
        }
    }
}
