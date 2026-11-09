//! External calendars: events from your other accounts (a work Google
//! Workspace, your personal Google with the flights Gmail adds), shown in
//! tasq next to your tasks, read-only.
//!
//! They are not tasks. They live in their own cache ([`cache`]), outside
//! the task database, so nothing that sends tasks anywhere — the Google
//! and Apple calendars tasq writes, sync between devices — ever sees them:
//! no duplicates, no echo. Disconnecting an account wipes its events.
//!
//! - [`event`]: an event as Google sends it, and what tasq keeps of it.
//! - [`sync`]: keeping the cache current, sending only what changed.
//! - [`google`]: the Google Calendar API behind [`sync::EventsApi`].
//! - [`occur`]: what falls on which day, in this device's time zone, for
//!   the calendar views.

pub mod accounts;
pub mod cache;
pub mod event;
pub mod google;
pub mod occur;
pub mod sync;

/// `url` opened as `email`: Google takes `authuser` on its links (the
/// event, Meet, Drive), so a link from a second account doesn't open in
/// whichever account the browser has first.
/// Only Google's own links take it: a Zoom or Teams link goes as it is.
pub fn as_account(url: &str, email: &str) -> String {
    let google = event::host_of(url).is_some_and(|h| {
        let h = h.to_ascii_lowercase();
        h == "google.com" || h.ends_with(".google.com")
    });
    if email.is_empty() || !google {
        return url.to_string();
    }
    let (base, frag) = match url.split_once('#') {
        Some((b, f)) => (b, Some(f)),
        None => (url, None),
    };
    let sep = if base.contains('?') { '&' } else { '?' };
    let mut out = format!("{base}{sep}authuser={}", crate::gcal::http::percent(email));
    if let Some(f) = frag {
        out.push('#');
        out.push_str(f);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_open_in_their_own_account() {
        assert_eq!(
            as_account("https://meet.google.com/abc-defg-hij", "me@work.com"),
            "https://meet.google.com/abc-defg-hij?authuser=me%40work.com"
        );
        assert_eq!(
            as_account("https://www.google.com/calendar/event?eid=X1#top", "a@b.c"),
            "https://www.google.com/calendar/event?eid=X1&authuser=a%40b.c#top"
        );
        assert_eq!(as_account("https://x.y/z", ""), "https://x.y/z");
        assert_eq!(
            as_account("https://us02web.zoom.us/j/1", "me@work.com"),
            "https://us02web.zoom.us/j/1",
            "not Google's"
        );
        assert_eq!(
            as_account("https://docs.google.com/document/d/1", "a@b.c"),
            "https://docs.google.com/document/d/1?authuser=a%40b.c"
        );
    }
}
