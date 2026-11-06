//! One sync round: send what changed here, sealed, then read and fold in
//! what the other devices sent since last time.

use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};

use super::client::Remote;
use super::keys::Keys;
use crate::core::db::Db;
use crate::core::sync::Change;

/// Changes per sealed batch: a first sync of a big list goes in several.
const BATCH: usize = 400;

/// What a round did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Report {
    /// Changes sent.
    pub sent: usize,
    /// Changes received from other devices.
    pub received: usize,
    /// Of those, the ones newer than what was here.
    pub applied: usize,
}

fn seal(keys: &Keys, changes: &[Change]) -> Result<String> {
    let body = json!({
        "v": 1,
        "changes": changes.iter().map(Change::to_json).collect::<Vec<_>>(),
    });
    keys.seal(body.to_string().as_bytes())
}

fn open(keys: &Keys, blob: &str) -> Result<Vec<Change>> {
    let plain = keys.open(blob)?;
    let v: Value = serde_json::from_slice(&plain).context("a batch that isn't JSON")?;
    if v["v"].as_i64() != Some(1) {
        return Err(anyhow!("a batch from a newer tasq: update to read it"));
    }
    Ok(v["changes"]
        .as_array()
        .map(|list| list.iter().filter_map(Change::from_json).collect())
        .unwrap_or_default())
}

/// Send this device's changes and fold in everyone else's. `today` dates
/// what lands in the trash.
pub fn sync_once(db: &mut Db, remote: &mut dyn Remote, keys: &Keys, today: &str) -> Result<Report> {
    let mut report = Report::default();
    let node = db.sync_node()?;

    // Out: everything that changed here, in batches.
    db.sync_scan()?;
    loop {
        let pending = db.sync_pending(BATCH)?;
        let Some((last, _)) = pending.last() else {
            break;
        };
        let last = *last;
        let changes: Vec<Change> = pending.into_iter().map(|(_, c)| c).collect();
        remote.push(&node, &seal(keys, &changes)?)?;
        db.sync_ack(last)?;
        report.sent += changes.len();
    }

    // In: the others' batches since last time, page by page.
    let mut cursor = db.sync_cursor()?;
    loop {
        let (items, more) = remote.pull(cursor)?;
        let mut changes = Vec::new();
        for item in &items {
            if item.device != node {
                changes.extend(open(keys, &item.blob)?);
            }
        }
        if !changes.is_empty() {
            let applied = db.sync_apply(&changes, today)?;
            report.received += changes.len();
            report.applied += applied.applied;
        }
        if let Some(last) = items.last() {
            cursor = last.seq;
            db.set_sync_cursor(cursor)?;
        }
        if !more || items.is_empty() {
            break;
        }
    }
    Ok(report)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::cloud::client::Local;
    use crate::cloud::relay::Relay;
    use crate::core::db::List;
    use crate::todo;

    fn device() -> Db {
        Db::in_memory()
    }

    fn add(db: &mut Db, line: &str) {
        let mut live = db.load(List::Live).unwrap();
        live.push(todo::parse_line(line).unwrap());
        db.save(Some(&mut live), None).unwrap();
    }

    fn titles(db: &Db) -> Vec<String> {
        let mut t: Vec<String> = db
            .load(List::Live)
            .unwrap()
            .into_iter()
            .map(|t| t.raw)
            .collect();
        t.sort();
        t
    }

    #[test]
    fn two_devices_sync_through_the_server_without_it_reading_anything() {
        let relay = Relay::in_memory().unwrap();
        let keys = Keys::derive("me@x.com", "my long password").unwrap();
        let mut mac = device();
        let mut laptop = device();
        add(&mut mac, "Finish thesis +Uni");
        add(&mut mac, "Call mom");

        let mut remote = Local {
            relay: &relay,
            keys: keys.clone(),
        };
        remote.signup().unwrap();
        let r = sync_once(&mut mac, &mut remote, &keys, "2026-11-06").unwrap();
        assert_eq!(r.sent, 3); // two tasks and the space
        let r = sync_once(&mut laptop, &mut remote, &keys, "2026-11-06").unwrap();
        assert_eq!((r.received, r.applied), (3, 3));
        assert_eq!(titles(&laptop), titles(&mac));

        // The laptop adds one; the Mac gets it on its next round.
        add(&mut laptop, "Buy coffee beans");
        sync_once(&mut laptop, &mut remote, &keys, "2026-11-06").unwrap();
        sync_once(&mut mac, &mut remote, &keys, "2026-11-06").unwrap();
        assert_eq!(titles(&mac), titles(&laptop));
        assert_eq!(titles(&mac).len(), 3);

        // Nothing new: a round sends and applies nothing.
        let r = sync_once(&mut mac, &mut remote, &keys, "2026-11-06").unwrap();
        assert_eq!((r.sent, r.applied), (0, 0));

        // What the server holds says nothing about the tasks.
        let auth = format!("Bearer {}", keys.bearer());
        let (_, all) = relay.handle("GET", "/v1/pull?after=0", Some(&auth), b"");
        let text = all.to_string();
        for word in ["thesis", "mom", "coffee", "Uni"] {
            assert!(!text.contains(word), "the server can read {word:?}");
        }
    }

    #[test]
    fn the_wrong_password_reads_nothing_and_says_so() {
        let relay = Relay::in_memory().unwrap();
        let keys = Keys::derive("me@x.com", "my long password").unwrap();
        let mut mac = device();
        add(&mut mac, "Secret plan");
        let mut remote = Local {
            relay: &relay,
            keys: keys.clone(),
        };
        remote.signup().unwrap();
        sync_once(&mut mac, &mut remote, &keys, "2026-11-06").unwrap();

        let wrong = Keys::derive("me@x.com", "not my password").unwrap();
        let mut thief = Local {
            relay: &relay,
            keys: wrong.clone(),
        };
        assert!(thief.check().is_err());
        let mut other = device();
        assert!(sync_once(&mut other, &mut thief, &wrong, "2026-11-06").is_err());
        assert!(titles(&other).is_empty());
    }

    #[test]
    fn a_big_first_sync_goes_in_batches() {
        let relay = Relay::in_memory().unwrap();
        let keys = Keys::derive("me@x.com", "my long password").unwrap();
        let mut mac = device();
        let mut live: Vec<todo::Task> = (0..1000)
            .map(|i| todo::parse_line(&format!("Task number {i}")).unwrap())
            .collect();
        mac.save(Some(&mut live), None).unwrap();
        let mut remote = Local {
            relay: &relay,
            keys: keys.clone(),
        };
        remote.signup().unwrap();
        assert_eq!(
            sync_once(&mut mac, &mut remote, &keys, "2026-11-06")
                .unwrap()
                .sent,
            1000
        );
        let mut laptop = device();
        sync_once(&mut laptop, &mut remote, &keys, "2026-11-06").unwrap();
        assert_eq!(titles(&laptop).len(), 1000);
    }

    /// The real thing: the server on a local port, the HTTP client (curl).
    #[test]
    fn over_http() {
        use crate::cloud::client::Http;
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        std::thread::spawn(move || {
            crate::cloud::relay::serve_on(server, Relay::in_memory().unwrap()).unwrap();
        });
        let base = format!("http://127.0.0.1:{port}");
        let keys = Keys::derive("me@x.com", "my long password").unwrap();
        let mut remote = Http::new(&base, keys.clone());
        remote.signup().unwrap();
        remote.check().unwrap();
        let mut mac = device();
        let mut laptop = device();
        add(&mut mac, "Over the wire +Net");
        sync_once(&mut mac, &mut remote, &keys, "2026-11-06").unwrap();
        sync_once(&mut laptop, &mut remote, &keys, "2026-11-06").unwrap();
        assert_eq!(titles(&laptop), vec!["Over the wire +Net".to_string()]);
        let wrong = Keys::derive("me@x.com", "not my password").unwrap();
        assert!(Http::new(&base, wrong).check().is_err());
    }
}
