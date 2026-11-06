//! Apple Calendar (iCloud, or any CalDAV server): tasq's dated tasks in a
//! calendar per top-level space — "tasq · Uni", "tasq · Work" — each in
//! its space's colour. Apple colours whole calendars, not single events,
//! so a calendar per space is how the colours carry over; spaces inside
//! one go in their top space's calendar, and tasks in no space in "tasq".
//!
//! Like Google: only what changed is sent, from this computer straight to
//! the server.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};

use super::auth::{plain, run, write_private};
use super::caldav::{DavApi, ICLOUD, Login};
use super::event::{self, Context};
use super::sync::{Report, fingerprint};
use super::{SpaceRgb, ics};
use crate::todo::Task;

/// The calendar for tasks in no space.
pub const BASE_NAME: &str = "tasq";
/// Its colour.
const BASE_COLOR: &str = "#7986CB";

/// What a task should be in Apple Calendar.
#[derive(Debug, Clone, PartialEq)]
pub struct Wanted {
    /// Its calendar: the top-level space ("" for none).
    pub key: String,
    /// That calendar's name and colour.
    pub name: String,
    pub color: String,
    /// The event, in Google's shape (see [`event::body`]); sent as
    /// iCalendar.
    pub body: Value,
}

/// The top-level space a task's calendar is for.
fn top(t: &Task) -> String {
    t.projects
        .first()
        .and_then(|p| p.split('/').next())
        .unwrap_or("")
        .to_string()
}

/// The name of the calendar for top-level space `key`.
pub fn calendar_name(key: &str) -> String {
    if key.is_empty() {
        BASE_NAME.to_string()
    } else {
        format!("{BASE_NAME} · {}", crate::core::spaces::display(key))
    }
}

/// Every task that belongs in Apple Calendar, by task id.
pub fn desired(tasks: &[Task], space_rgb: SpaceRgb, time_zone: &str) -> BTreeMap<String, Wanted> {
    let cx = Context {
        time_zone,
        color_id: None,
    };
    tasks
        .iter()
        .filter(|t| !t.id.is_empty())
        .filter_map(|t| {
            let body = event::body(t, &cx)?;
            let key = top(t);
            let color = if key.is_empty() {
                BASE_COLOR.to_string()
            } else {
                space_rgb(&key).map_or(BASE_COLOR.to_string(), |(r, g, b)| {
                    format!("#{r:02X}{g:02X}{b:02X}")
                })
            };
            Some((
                t.id.clone(),
                Wanted {
                    name: calendar_name(&key),
                    key,
                    color,
                    body,
                },
            ))
        })
        .collect()
}

/// What was sent: the calendars made (by top-level space: URL and colour)
/// and per task its calendar and a fingerprint.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppleState {
    pub calendars: BTreeMap<String, (String, String)>,
    pub events: BTreeMap<String, (String, u64)>,
}

impl AppleState {
    pub fn to_json(&self) -> String {
        json!({
            "calendars": self.calendars.iter()
                .map(|(k, (url, color))| (k.clone(), json!([url, color])))
                .collect::<serde_json::Map<_, _>>(),
            "events": self.events.iter()
                .map(|(t, (k, h))| (t.clone(), json!([k, h.to_string()])))
                .collect::<serde_json::Map<_, _>>(),
        })
        .to_string()
    }

    pub fn from_json(s: &str) -> Self {
        let Ok(v) = serde_json::from_str::<Value>(s) else {
            return Self::default();
        };
        let pairs = |name: &str| -> Vec<(String, String, String)> {
            v[name]
                .as_object()
                .map(|m| {
                    m.iter()
                        .filter_map(|(k, p)| {
                            Some((
                                k.clone(),
                                p.get(0)?.as_str()?.to_string(),
                                p.get(1)?.as_str()?.to_string(),
                            ))
                        })
                        .collect()
                })
                .unwrap_or_default()
        };
        Self {
            calendars: pairs("calendars")
                .into_iter()
                .map(|(k, a, b)| (k, (a, b)))
                .collect(),
            events: pairs("events")
                .into_iter()
                .filter_map(|(t, k, h)| Some((t, (k, h.parse().ok()?))))
                .collect(),
        }
    }
}

/// Bring the calendars in line with `desired`: make the ones missing (and
/// again if deleted by hand), put what's new or changed, remove what's
/// gone. A calendar left empty goes too once its space is gone (`spaces`:
/// the top-level spaces there are).
pub fn sync(
    api: &mut dyn DavApi,
    state: &mut AppleState,
    desired: &BTreeMap<String, Wanted>,
    spaces: &BTreeSet<String>,
    stamp: &str,
) -> Result<Report> {
    let mut report = Report::default();
    if desired.is_empty() && state.events.len() >= 3 {
        bail!("no tasks to send · the calendars are left as they are");
    }
    // Calendars deleted by hand: forget them, and what was in them.
    let kept: Vec<(String, String)> = state
        .calendars
        .iter()
        .map(|(k, (url, _))| (k.clone(), url.clone()))
        .collect();
    for (key, url) in kept {
        if !api.calendar_exists(&url)? {
            state.calendars.remove(&key);
            state.events.retain(|_, (k, _)| *k != key);
        }
    }

    // The calendars needed, in their colours.
    let mut needed: BTreeMap<&str, (&str, &str)> = BTreeMap::new();
    for w in desired.values() {
        needed.entry(&w.key).or_insert((&w.name, &w.color));
    }
    for (key, (name, color)) in &needed {
        match state.calendars.get(*key) {
            None => {
                let url = api.make_calendar(name, color)?;
                state
                    .calendars
                    .insert(key.to_string(), (url, color.to_string()));
            }
            Some((url, c)) if c != color => {
                let url = url.clone();
                match api.set_color(&url, color) {
                    Ok(()) => {
                        state
                            .calendars
                            .insert(key.to_string(), (url, color.to_string()));
                    }
                    Err(e) => report.errors.push(e.to_string()),
                }
            }
            Some(_) => {}
        }
    }

    // Gone, or moved to another space's calendar: out of the old one.
    let stale: Vec<(String, String)> = state
        .events
        .iter()
        .filter(|(t, (k, _))| desired.get(*t).is_none_or(|w| w.key != *k))
        .map(|(t, (k, _))| (t.clone(), k.clone()))
        .collect();
    for (task, key) in stale {
        let Some((url, _)) = state.calendars.get(&key).cloned() else {
            state.events.remove(&task);
            continue;
        };
        match api.delete(&url, &task) {
            Ok(()) => {
                state.events.remove(&task);
                report.deleted += 1;
            }
            Err(e) => report.errors.push(format!("delete {task}: {e}")),
        }
    }

    for (task, w) in desired {
        let hash = fingerprint(&w.body);
        let before = state.events.get(task).map(|(_, h)| *h);
        if before == Some(hash) {
            continue;
        }
        let Some((url, _)) = state.calendars.get(&w.key).cloned() else {
            continue;
        };
        let Some(text) = ics::from_body(&w.body, task, stamp) else {
            continue;
        };
        match api.put(&url, task, &text) {
            Ok(()) => {
                state.events.insert(task.clone(), (w.key.clone(), hash));
                if before.is_some() {
                    report.updated += 1;
                } else {
                    report.inserted += 1;
                }
            }
            Err(e) => report.errors.push(format!("{task}: {e}")),
        }
    }

    // Empty calendars of spaces that are gone.
    let empty: Vec<(String, String)> = state
        .calendars
        .iter()
        .filter(|(k, _)| {
            !needed.contains_key(k.as_str())
                && !spaces.contains(*k)
                && !state.events.values().any(|(ek, _)| ek == *k)
        })
        .map(|(k, (url, _))| (k.clone(), url.clone()))
        .collect();
    for (key, url) in empty {
        match api.delete_calendar(&url) {
            Ok(()) => {
                state.calendars.remove(&key);
            }
            Err(e) => report.errors.push(e.to_string()),
        }
    }
    Ok(report)
}

// ---------------------------------------------------------------------------
// The account, kept
// ---------------------------------------------------------------------------

const SERVICE: &str = "tasq-apple-calendar";

/// The server and the Apple ID (not secrets), beside the config; with no
/// keychain, the password on a third line (the file is only yours).
fn account_path() -> Option<std::path::PathBuf> {
    crate::xdg::config_home().map(|d| d.join("tasq").join("apple-account"))
}

/// Keep `login`: the password in the keychain when there is one.
pub fn store(login: &Login) -> Result<()> {
    let pw = &login.password;
    let in_keychain = if cfg!(target_os = "macos") {
        plain(pw)
            && run(
                "security",
                &["-i"],
                Some(&format!(
                    "add-generic-password -U -a tasq -s {SERVICE} -w {pw}\n"
                )),
            )
            .is_some()
    } else {
        run(
            "secret-tool",
            &[
                "store",
                "--label",
                "tasq Apple Calendar",
                "service",
                SERVICE,
            ],
            Some(pw),
        )
        .is_some()
    };
    let path = account_path().ok_or_else(|| anyhow!("no config directory"))?;
    let mut text = format!("{}\n{}\n", login.server, login.user);
    if !in_keychain {
        text.push_str(pw);
        text.push('\n');
    }
    write_private(&path, &text)
}

/// The kept server and Apple ID, without touching the keychain.
pub fn kept_user() -> Option<(String, String)> {
    let text = std::fs::read_to_string(account_path()?).ok()?;
    let mut lines = text.lines();
    let server = lines.next()?.trim().to_string();
    let user = lines.next()?.trim().to_string();
    (!user.is_empty()).then_some((server, user))
}

/// The kept login, password and all.
pub fn load() -> Option<Login> {
    let text = std::fs::read_to_string(account_path()?).ok()?;
    let (server, user) = kept_user()?;
    let password = match text.lines().nth(2).map(str::trim).filter(|p| !p.is_empty()) {
        Some(p) => p.to_string(),
        None if cfg!(target_os = "macos") => run(
            "security",
            &["find-generic-password", "-a", "tasq", "-s", SERVICE, "-w"],
            None,
        )?,
        None => run("secret-tool", &["lookup", "service", SERVICE], None)?,
    };
    Some(Login {
        server: if server.is_empty() {
            ICLOUD.to_string()
        } else {
            server
        },
        user,
        password,
    })
}

/// Forget the kept login, everywhere it may be.
pub fn forget() {
    if cfg!(target_os = "macos") {
        let _ = run(
            "security",
            &["delete-generic-password", "-a", "tasq", "-s", SERVICE],
            None,
        );
    } else {
        let _ = run("secret-tool", &["clear", "service", SERVICE], None);
    }
    if let Some(p) = account_path() {
        let _ = std::fs::remove_file(p);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::todo::parse_line;

    /// A CalDAV account in memory: calendars by URL, with a name, a colour
    /// and events by uid.
    #[derive(Default)]
    struct FakeDav {
        cals: BTreeMap<String, (String, String, BTreeMap<String, String>)>,
        next: usize,
        calls: usize,
    }

    impl DavApi for FakeDav {
        fn make_calendar(&mut self, name: &str, color: &str) -> Result<String> {
            self.next += 1;
            let url = format!("/cal{}/", self.next);
            self.cals
                .insert(url.clone(), (name.into(), color.into(), BTreeMap::new()));
            Ok(url)
        }
        fn calendar_exists(&mut self, url: &str) -> Result<bool> {
            Ok(self.cals.contains_key(url))
        }
        fn set_color(&mut self, url: &str, color: &str) -> Result<()> {
            self.cals.get_mut(url).unwrap().1 = color.into();
            Ok(())
        }
        fn delete_calendar(&mut self, url: &str) -> Result<()> {
            self.cals.remove(url);
            Ok(())
        }
        fn put(&mut self, url: &str, uid: &str, ics: &str) -> Result<()> {
            self.calls += 1;
            self.cals
                .get_mut(url)
                .ok_or_else(|| anyhow!("no calendar"))?
                .2
                .insert(uid.into(), ics.into());
            Ok(())
        }
        fn delete(&mut self, url: &str, uid: &str) -> Result<()> {
            self.calls += 1;
            if let Some(c) = self.cals.get_mut(url) {
                c.2.remove(uid);
            }
            Ok(())
        }
    }

    impl FakeDav {
        /// Calendar name → its events' summaries.
        fn view(&self) -> BTreeMap<String, Vec<String>> {
            self.cals
                .values()
                .map(|(name, _, evs)| {
                    let titles = evs
                        .values()
                        .map(|ics| {
                            ics.lines()
                                .find_map(|l| l.strip_prefix("SUMMARY:"))
                                .unwrap_or("")
                                .to_string()
                        })
                        .collect();
                    (name.clone(), titles)
                })
                .collect()
        }
    }

    fn tasks(lines: &[&str]) -> Vec<Task> {
        lines
            .iter()
            .enumerate()
            .map(|(i, l)| {
                let mut t = parse_line(l).unwrap();
                t.id = format!("t{i}");
                t
            })
            .collect()
    }

    fn want(ts: &[Task]) -> BTreeMap<String, Wanted> {
        let rgb = |p: &str| -> Option<(u8, u8, u8)> {
            Some(if p == "Uni" {
                (0xed, 0x87, 0x96)
            } else {
                (0x8a, 0xad, 0xf4)
            })
        };
        desired(ts, &rgb, "Europe/Madrid")
    }

    fn spaces(s: &[&str]) -> BTreeSet<String> {
        s.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_calendar_per_top_space_in_its_colour() {
        let mut dav = FakeDav::default();
        let mut st = AppleState::default();
        let ts = tasks(&[
            "Exam +Uni/Exams plan:2026-10-12 at:09:00",
            "Essay +Uni plan:2026-10-13",
            "Deploy +Work plan:2026-10-14",
            "Haircut plan:2026-10-15",
            "Someday +Work",
        ]);
        let r = sync(
            &mut dav,
            &mut st,
            &want(&ts),
            &spaces(&["Uni", "Work"]),
            "S",
        )
        .unwrap();
        assert_eq!(r.inserted, 4);
        assert_eq!(
            dav.view(),
            BTreeMap::from([
                ("tasq".into(), vec!["Haircut".into()]),
                ("tasq · Uni".into(), vec!["Exam".into(), "Essay".into()]),
                ("tasq · Work".into(), vec!["Deploy".into()]),
            ])
        );
        let uni = &st.calendars["Uni"];
        assert_eq!(dav.cals[&uni.0].1, "#ED8796");

        // Nothing changed: no calls.
        let calls = dav.calls;
        sync(
            &mut dav,
            &mut st,
            &want(&ts),
            &spaces(&["Uni", "Work"]),
            "S",
        )
        .unwrap();
        assert_eq!(dav.calls, calls);

        // The essay moves to Work; the deploy is done; Work's gone after.
        let ts = tasks(&[
            "Exam +Uni/Exams plan:2026-10-12 at:09:00",
            "Essay +Work plan:2026-10-13",
            "x 2026-10-12 Deploy +Work plan:2026-10-14",
            "Haircut plan:2026-10-15",
        ]);
        let r = sync(
            &mut dav,
            &mut st,
            &want(&ts),
            &spaces(&["Uni", "Work"]),
            "S",
        )
        .unwrap();
        assert_eq!((r.inserted, r.updated, r.deleted), (1, 0, 2));
        assert_eq!(dav.view()["tasq · Work"], vec!["Essay".to_string()]);

        // The state survives a restart.
        assert_eq!(AppleState::from_json(&st.to_json()), st);
    }

    #[test]
    fn a_calendar_deleted_by_hand_comes_back_and_an_old_one_goes() {
        let mut dav = FakeDav::default();
        let mut st = AppleState::default();
        let ts = tasks(&["Exam +Uni plan:2026-10-12", "Deploy +Work plan:2026-10-14"]);
        sync(
            &mut dav,
            &mut st,
            &want(&ts),
            &spaces(&["Uni", "Work"]),
            "S",
        )
        .unwrap();
        let uni = st.calendars["Uni"].0.clone();
        dav.cals.remove(&uni);
        let r = sync(
            &mut dav,
            &mut st,
            &want(&ts),
            &spaces(&["Uni", "Work"]),
            "S",
        )
        .unwrap();
        assert_eq!(r.inserted, 1);
        assert_eq!(dav.view()["tasq · Uni"], vec!["Exam".to_string()]);

        // Work renamed to Job: its old calendar empties and goes.
        let ts = tasks(&["Exam +Uni plan:2026-10-12", "Deploy +Job plan:2026-10-14"]);
        sync(&mut dav, &mut st, &want(&ts), &spaces(&["Uni", "Job"]), "S").unwrap();
        let names: Vec<String> = dav.view().into_keys().collect();
        assert_eq!(names, ["tasq · Job", "tasq · Uni"]);
    }

    #[test]
    fn an_empty_list_leaves_the_calendars_alone() {
        let mut dav = FakeDav::default();
        let mut st = AppleState::default();
        let ts = tasks(&[
            "a plan:2026-10-12",
            "b plan:2026-10-13",
            "c plan:2026-10-14",
        ]);
        sync(&mut dav, &mut st, &want(&ts), &spaces(&[]), "S").unwrap();
        assert!(sync(&mut dav, &mut st, &BTreeMap::new(), &spaces(&[]), "S").is_err());
        assert_eq!(dav.view()["tasq"].len(), 3);
    }
}

/// Against a real server: `TASQ_CALDAV_URL`, `TASQ_CALDAV_USER` and
/// `TASQ_CALDAV_PASSWORD` set, then
/// `cargo test caldav_server -- --ignored --nocapture`. It makes, fills,
/// changes and removes calendars named "tasq-test · …".
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod live {
    use super::*;
    use crate::gcal::caldav::CalDav;
    use crate::todo::parse_line;

    #[test]
    #[ignore = "needs a CalDAV server"]
    fn caldav_server() {
        let env = |k: &str| std::env::var(k).unwrap();
        let mut dav = CalDav::new(Login {
            server: env("TASQ_CALDAV_URL"),
            user: env("TASQ_CALDAV_USER"),
            password: env("TASQ_CALDAV_PASSWORD"),
        });
        dav.check().unwrap();
        let mk = |lines: &[&str]| -> BTreeMap<String, Wanted> {
            let ts: Vec<Task> = lines
                .iter()
                .enumerate()
                .map(|(i, l)| {
                    let mut t = parse_line(l).unwrap();
                    t.id = format!("tasqtest{i}");
                    t
                })
                .collect();
            let rgb = |_: &str| Some((0xed, 0x87, 0x96));
            desired(&ts, &rgb, "Europe/Madrid")
                .into_iter()
                .map(|(k, mut w)| {
                    w.name = w.name.replace("tasq", "tasq-test");
                    (k, w)
                })
                .collect()
        };
        let all: BTreeSet<String> = ["TestA".into()].into();
        let mut st = AppleState::default();
        let r = sync(
            &mut dav,
            &mut st,
            &mk(&[
                "Exam +TestA plan:2026-10-12 at:09:00 dur:2h remind:30m",
                "Gym +TestA plan:2026-10-12 at:07:00 rec:1w skip:2026-10-19",
                "Trip plan:2026-10-20 end:2026-10-22",
            ]),
            &all,
            &ics::stamp_now(),
        )
        .unwrap();
        println!("first: {r:?} {st:?}");
        assert_eq!((r.inserted, r.errors.len()), (3, 0));
        let r = sync(
            &mut dav,
            &mut st,
            &mk(&["Exam moved +TestB plan:2026-10-13 at:10:00"]),
            &BTreeSet::new(),
            &ics::stamp_now(),
        )
        .unwrap();
        println!("second: {r:?} {st:?}");
        assert!(r.errors.is_empty());
        // Clean up.
        for (url, _) in st.calendars.values() {
            dav.delete_calendar(url).unwrap();
        }
    }
}
