//! `tasq sync …`: sign in, sync now, see where things stand, sign out.
//!
//! ```text
//! tasq sync login [--server URL] EMAIL
//! tasq sync            one round: send, then receive
//! tasq sync status
//! tasq sync logout
//! ```
//!
//! The password is asked for without echo (or read from
//! `TASQ_SYNC_PASSWORD`, for scripts) and never kept: only the keys made
//! from it are, in a file only you can read.

use anyhow::{Context, Result, anyhow, bail};
use std::io::Write;

use super::client::{Http, Remote};
use super::keys::{Keys, normalize_email};
use super::run::sync_once;
use super::session::{self, Session};
use crate::core::db::Db;

/// A password, typed without echo (or from `TASQ_SYNC_PASSWORD`).
fn ask_password(prompt: &str) -> Result<String> {
    if let Ok(p) = std::env::var("TASQ_SYNC_PASSWORD") {
        return Ok(p);
    }
    use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
    eprint!("{prompt}");
    std::io::stderr().flush()?;
    crossterm::terminal::enable_raw_mode()?;
    let mut out = String::new();
    let result = loop {
        match event::read() {
            Ok(Event::Key(k)) if k.kind == KeyEventKind::Press => match k.code {
                KeyCode::Enter => break Ok(()),
                KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                    break Err(anyhow!("cancelled"));
                }
                KeyCode::Char(c) => out.push(c),
                KeyCode::Backspace => {
                    out.pop();
                }
                KeyCode::Esc => break Err(anyhow!("cancelled")),
                _ => {}
            },
            Ok(Event::Paste(p)) => out.push_str(&p),
            Ok(_) => {}
            Err(e) => break Err(e.into()),
        }
    };
    crossterm::terminal::disable_raw_mode()?;
    eprintln!();
    result.map(|()| out)
}

fn open_db() -> Result<Db> {
    let path = crate::cli::db_path().ok_or_else(|| anyhow!("no data directory"))?;
    Db::open(&path).with_context(|| format!("opening {}", path.display()))
}

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// Run `tasq sync …` (`args` after `sync`); the exit code. Failures are
/// one line on stderr, not an error trace.
pub fn run(args: &[String]) -> Result<i32> {
    match run_inner(args) {
        Ok(code) => Ok(code),
        Err(e) => {
            eprintln!("tasq: sync: {e:#}");
            Ok(1)
        }
    }
}

fn run_inner(args: &[String]) -> Result<i32> {
    match args.first().map(String::as_str) {
        None => sync_now(),
        Some("login") => login(&args[1..]),
        Some("status") => status(),
        Some("logout") => logout(),
        Some(other) => {
            eprintln!(
                "tasq: sync: unknown command {other:?} (login, status, logout, or nothing to sync now)"
            );
            Ok(2)
        }
    }
}

fn login(args: &[String]) -> Result<i32> {
    let mut server = Some(session::server(None));
    let mut email = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--server" => server = it.next().cloned(),
            e => email = Some(e.to_string()),
        }
    }
    let Some(email) = email else {
        eprintln!("usage: tasq sync login [--server URL] EMAIL");
        return Ok(2);
    };
    let Some(server) = server else {
        bail!("which sync server? tasq sync login --server https://… {email}");
    };
    let server = server.trim_end_matches('/').to_string();
    let email = normalize_email(&email);
    let password = ask_password("Password: ")?;
    eprintln!("making the keys (a second)…");
    let keys = Keys::derive(&email, &password)?;
    let mut remote = Http::new(&server, keys.clone());
    match remote.check() {
        Ok(()) => eprintln!("signed in to {server}"),
        Err(e) if e.to_string().contains("no such account") => {
            // A new account: the password twice, since nothing can recover it.
            let again =
                ask_password("No account for that email yet. Password again to create it: ")?;
            if again != password {
                bail!("the passwords don't match");
            }
            remote.signup()?;
            eprintln!("account created on {server}");
            eprintln!(
                "keep the password safe: your tasks are encrypted with it, and nobody can reset it"
            );
        }
        Err(e) => return Err(e),
    }
    let mut db = open_db()?;
    // A fresh start for this device: a new name, and everything sent again.
    db.sync_forget()?;
    db.sync_reset_node()?;
    Session {
        server,
        email,
        keys,
    }
    .save()?;
    sync_now()
}

fn sync_now() -> Result<i32> {
    let Some(session) = Session::load() else {
        eprintln!("not signed in: tasq sync login EMAIL");
        return Ok(1);
    };
    let mut db = open_db()?;
    let mut remote = Http::new(&session.server, session.keys.clone());
    let r = sync_once(&mut db, &mut remote, &session.keys, &today())?;
    println!(
        "synced · {} sent · {} received ({} new)",
        r.sent, r.received, r.applied
    );
    Ok(0)
}

fn status() -> Result<i32> {
    let Some(session) = Session::load() else {
        println!("not signed in");
        return Ok(0);
    };
    let mut db = open_db()?;
    let waiting = {
        db.sync_scan()?;
        db.sync_pending(100_000)?.len()
    };
    println!("signed in as {} on {}", session.email, session.server);
    println!("this device: {}", db.sync_node()?);
    println!("{waiting} change(s) waiting to be sent");
    Ok(0)
}

fn logout() -> Result<i32> {
    Session::forget();
    open_db()?.sync_forget()?;
    println!("signed out · your tasks stay on this device");
    Ok(0)
}
