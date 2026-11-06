//! CalDAV (RFC 4791), the protocol iCloud's calendars speak: find the
//! account's calendars, make one, put and delete events in it. Signing in
//! is an Apple ID and an app-specific password, over HTTPS.
//!
//! Only what tasq needs, behind [`DavApi`] so the sync runs the same
//! against a server and against the fake the tests use.

use anyhow::{Result, anyhow, bail};

use super::http::{self, Request};

/// iCloud's CalDAV server.
pub const ICLOUD: &str = "https://caldav.icloud.com/";

/// What the sync needs from a CalDAV account.
pub trait DavApi {
    /// Make a calendar called `name` in `color` (`#RRGGBB`); its URL.
    fn make_calendar(&mut self, name: &str, color: &str) -> Result<String>;
    /// Whether the calendar at `url` is still there.
    fn calendar_exists(&mut self, url: &str) -> Result<bool>;
    /// Change a calendar's colour.
    fn set_color(&mut self, url: &str, color: &str) -> Result<()>;
    /// Remove a calendar and what's in it (gone already is fine).
    fn delete_calendar(&mut self, url: &str) -> Result<()>;
    /// Add or replace the event `uid` in the calendar at `url`.
    fn put(&mut self, url: &str, uid: &str, ics: &str) -> Result<()>;
    /// Remove the event `uid` (gone already is fine).
    fn delete(&mut self, url: &str, uid: &str) -> Result<()>;
}

/// An account: the server, the user (Apple ID) and the app-specific
/// password.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Login {
    pub server: String,
    pub user: String,
    pub password: String,
}

/// A CalDAV account over HTTP.
pub struct CalDav {
    login: Login,
    /// Where the account's calendars live, once found.
    home: Option<String>,
}

/// `scheme://host[:port]` of `url`.
fn origin(url: &str) -> &str {
    let after = url.find("://").map_or(0, |i| i + 3);
    match url[after..].find('/') {
        Some(i) => &url[..after + i],
        None => url,
    }
}

/// `href` (a path or a whole URL) against the server `base`.
pub fn resolve(base: &str, href: &str) -> String {
    if href.starts_with("http://") || href.starts_with("https://") {
        href.to_string()
    } else {
        format!("{}{href}", origin(base))
    }
}

/// The text of the first `<…:href>` inside the first element called `tag`
/// (any namespace prefix) in `xml`.
pub fn href_in(xml: &str, tag: &str) -> Option<String> {
    let start = open_tag(xml, tag)?;
    let rest = &xml[start..];
    let h = open_tag(rest, "href")?;
    let text = &rest[h..];
    let end = text.find('<')?;
    let v = text[..end].trim();
    (!v.is_empty()).then(|| unescape(v))
}

/// Where the content of the first `<tag>` / `<p:tag …>` starts.
fn open_tag(xml: &str, tag: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(i) = xml[from..].find('<') {
        let at = from + i + 1;
        let end = at + xml[at..].find('>')?;
        let name = xml[at..end]
            .split(|c: char| c.is_whitespace() || c == '/')
            .next()
            .unwrap_or("");
        let local = name.rsplit(':').next().unwrap_or(name);
        if local == tag && !xml[at..end].ends_with('/') {
            return Some(end + 1);
        }
        from = end + 1;
    }
    None
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Apple's colour property: `#RRGGBBAA`.
fn apple_color(color: &str) -> String {
    format!("{}FF", color.to_uppercase())
}

impl CalDav {
    pub fn new(login: Login) -> Self {
        Self { login, home: None }
    }

    fn send(
        &self,
        method: &str,
        url: &str,
        depth: Option<&str>,
        body: Option<String>,
        kind: &str,
    ) -> Result<(u16, String)> {
        let auth = http::b64(format!("{}:{}", self.login.user, self.login.password).as_bytes());
        let mut headers = vec![format!("Authorization: Basic {auth}")];
        if let Some(d) = depth {
            headers.push(format!("Depth: {d}"));
        }
        if body.is_some() {
            headers.push(format!("Content-Type: {kind}; charset=utf-8"));
        }
        let (code, text) = http::send(&Request {
            method,
            url: url.to_string(),
            headers,
            body,
        })?;
        if code == 401 {
            bail!("the Apple ID or the app-specific password is wrong");
        }
        Ok((code, text))
    }

    fn propfind(&self, url: &str, prop: &str) -> Result<String> {
        let body = format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
             <D:propfind xmlns:D=\"DAV:\" xmlns:C=\"urn:ietf:params:xml:ns:caldav\">\
             <D:prop>{prop}</D:prop></D:propfind>"
        );
        let (code, text) = self.send("PROPFIND", url, Some("0"), Some(body), "application/xml")?;
        if code != 207 {
            bail!("the calendar server answered {code}");
        }
        Ok(text)
    }

    /// Where the account's calendars are: the principal, then its home.
    fn home(&mut self) -> Result<String> {
        if let Some(h) = &self.home {
            return Ok(h.clone());
        }
        let base = self.login.server.clone();
        let xml = self.propfind(&base, "<D:current-user-principal/>")?;
        let principal = href_in(&xml, "current-user-principal")
            .ok_or_else(|| anyhow!("the server didn't say who you are"))?;
        let principal = resolve(&base, &principal);
        let xml = self.propfind(&principal, "<C:calendar-home-set/>")?;
        let home = href_in(&xml, "calendar-home-set")
            .ok_or_else(|| anyhow!("the server has no calendars for this account"))?;
        let mut home = resolve(&principal, &home);
        if !home.ends_with('/') {
            home.push('/');
        }
        self.home = Some(home.clone());
        Ok(home)
    }

    /// Check the login works (finds the calendars' home).
    pub fn check(&mut self) -> Result<()> {
        self.home().map(|_| ())
    }

    fn event_url(url: &str, uid: &str) -> String {
        let sep = if url.ends_with('/') { "" } else { "/" };
        format!("{url}{sep}{}.ics", http::percent(uid))
    }
}

impl DavApi for CalDav {
    fn make_calendar(&mut self, name: &str, color: &str) -> Result<String> {
        let home = self.home()?;
        let url = format!("{home}tasq-{}/", super::auth::random_id(12));
        let body = format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
             <C:mkcalendar xmlns:D=\"DAV:\" xmlns:C=\"urn:ietf:params:xml:ns:caldav\" \
             xmlns:A=\"http://apple.com/ns/ical/\"><D:set><D:prop>\
             <D:displayname>{}</D:displayname>\
             <A:calendar-color>{}</A:calendar-color>\
             <C:supported-calendar-component-set><C:comp name=\"VEVENT\"/></C:supported-calendar-component-set>\
             </D:prop></D:set></C:mkcalendar>",
            escape(name),
            apple_color(color)
        );
        let (code, _) = self.send("MKCALENDAR", &url, None, Some(body), "application/xml")?;
        match code {
            200 | 201 => Ok(url),
            _ => bail!("couldn't make the calendar \"{name}\" (the server answered {code})"),
        }
    }

    fn calendar_exists(&mut self, url: &str) -> Result<bool> {
        let body = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
                    <D:propfind xmlns:D=\"DAV:\"><D:prop><D:resourcetype/></D:prop></D:propfind>"
            .to_string();
        let (code, _) = self.send("PROPFIND", url, Some("0"), Some(body), "application/xml")?;
        match code {
            207 | 200 => Ok(true),
            404 | 410 => Ok(false),
            _ => bail!("the calendar server answered {code}"),
        }
    }

    fn set_color(&mut self, url: &str, color: &str) -> Result<()> {
        let body = format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
             <D:propertyupdate xmlns:D=\"DAV:\" xmlns:A=\"http://apple.com/ns/ical/\">\
             <D:set><D:prop><A:calendar-color>{}</A:calendar-color></D:prop></D:set>\
             </D:propertyupdate>",
            apple_color(color)
        );
        let (code, _) = self.send("PROPPATCH", url, None, Some(body), "application/xml")?;
        match code {
            200 | 207 => Ok(()),
            _ => bail!("couldn't change the calendar's colour ({code})"),
        }
    }

    fn delete_calendar(&mut self, url: &str) -> Result<()> {
        let (code, _) = self.send("DELETE", url, None, None, "")?;
        match code {
            200 | 204 | 404 | 410 => Ok(()),
            _ => bail!("couldn't remove the calendar ({code})"),
        }
    }

    fn put(&mut self, url: &str, uid: &str, ics: &str) -> Result<()> {
        let (code, text) = self.send(
            "PUT",
            &Self::event_url(url, uid),
            None,
            Some(ics.to_string()),
            "text/calendar",
        )?;
        match code {
            200 | 201 | 204 => Ok(()),
            _ => {
                let why: String = text.chars().take(120).collect();
                bail!("the server turned the event down ({code}) {}", why.trim())
            }
        }
    }

    fn delete(&mut self, url: &str, uid: &str) -> Result<()> {
        let (code, _) = self.send("DELETE", &Self::event_url(url, uid), None, None, "")?;
        match code {
            200 | 204 | 404 | 410 => Ok(()),
            _ => bail!("couldn't remove the event ({code})"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hrefs_come_out_of_any_namespace() {
        let icloud = r#"<?xml version="1.0"?><multistatus xmlns="DAV:"><response><href>/</href><propstat><prop><current-user-principal><href>/123456/principal/</href></current-user-principal></prop></propstat></response></multistatus>"#;
        assert_eq!(
            href_in(icloud, "current-user-principal").as_deref(),
            Some("/123456/principal/")
        );
        let prefixed = r#"<d:multistatus xmlns:d="DAV:" xmlns:cal="urn:ietf:params:xml:ns:caldav"><d:response><d:href>/p/</d:href><d:propstat><d:prop><cal:calendar-home-set><d:href>https://p42-caldav.icloud.com:443/123456/calendars/</d:href></cal:calendar-home-set></d:prop></d:propstat></d:response></d:multistatus>"#;
        assert_eq!(
            href_in(prefixed, "calendar-home-set").as_deref(),
            Some("https://p42-caldav.icloud.com:443/123456/calendars/")
        );
        assert_eq!(href_in("<a><b/></a>", "b"), None);
    }

    #[test]
    fn hrefs_resolve_against_the_server() {
        assert_eq!(
            resolve("https://caldav.icloud.com/", "/1/principal/"),
            "https://caldav.icloud.com/1/principal/"
        );
        assert_eq!(
            resolve(
                "https://caldav.icloud.com/",
                "https://p1-caldav.icloud.com:443/1/"
            ),
            "https://p1-caldav.icloud.com:443/1/"
        );
        assert_eq!(apple_color("#8aadf4"), "#8AADF4FF");
    }
}
