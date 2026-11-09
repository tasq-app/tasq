//! Your calendar in the browser: a small web app served by `tasq-relay`
//! itself, drawn from what the server works out ([`super::webcal`]).
//!
//! It reads your account the way Claude does ([`super::assistant`]): you
//! sign in on a tasq page with the email and password your computers sync
//! with, the server makes the keys from them and seals the keys under a
//! token that only your browser holds (an `HttpOnly` cookie). The server
//! keeps the sealed keys and the token's hash; each time the page asks, it
//! opens the keys with the token, reads the account in memory, answers,
//! and lets it go. The browser shows up in tasq → Settings → Sync next to
//! Claude, and disconnecting it there (or signing out here) forgets the
//! keys. It only reads: nothing here changes a task.
//!
//! The routes:
//!
//! - `GET /`: the app (or the sign-in page, until you sign in).
//! - `GET`/`POST /login`, `POST /logout`.
//! - `GET /v1/calendar?from=YYYY-MM-DD&to=YYYY-MM-DD&tz=Area/City`: the
//!   days, as JSON.
//! - `GET /app.js`, `/app.css`, `/themes.json`, `/manifest.webmanifest`,
//!   `/sw.js`: what the page is made of.

use chrono::{NaiveDate, Timelike};
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};

use super::assistant::{
    READ, WEB_CLIENT, asset, base_url, esc_html, field, html_out, issue, page, page_message,
    parse_form, redirect, unix, with_conn,
};
use super::keys::{self, Keys};
use super::relay::{Incoming, Outgoing, Relay, now};
use super::tools::{Account, with_external};
use super::webcal::{self, MAX_DAYS};

/// The cookie that holds the browser's token.
const COOKIE: &str = "tasq_web";
/// How long a remembered browser stays signed in without coming back.
const REMEMBER_SECS: i64 = 90 * 24 * 3600;
/// How long a sign-in lasts on a browser you didn't ask to remember.
const SESSION_SECS: i64 = 12 * 3600;
/// The scope word that marks a remembered browser.
const REMEMBER: &str = "remember";

const INDEX_HTML: &str = include_str!("assets/web/index.html");
const APP_JS: &str = include_str!("assets/web/app.js");
const APP_CSS: &str = include_str!("assets/web/app.css");
const SW_JS: &str = include_str!("assets/web/sw.js");
const MANIFEST: &str = include_str!("assets/web/manifest.webmanifest");

/// Answer `req` if it's one of the web routes.
pub fn respond(relay: &Relay, req: &Incoming) -> Option<Outgoing> {
    let (route, query) = req.url.split_once('?').unwrap_or((&req.url, ""));
    let es = spanish(req);
    Some(match (req.method.as_str(), route) {
        ("GET", "/" | "/index.html") => match session(relay, req) {
            Some(_) => app_page(),
            None => redirect("/login"),
        },
        ("GET", "/login") => {
            if session(relay, req).is_some() {
                redirect("/")
            } else {
                login_page(es, "", None)
            }
        }
        ("POST", "/login") => {
            let form = parse_form(&String::from_utf8_lossy(&req.body));
            login(relay, req, &form, es)
        }
        ("POST", "/logout") => logout(relay, req),
        ("GET", "/v1/calendar") => calendar(relay, req, query),
        ("GET", "/app.js") => text_asset("text/javascript; charset=utf-8", APP_JS),
        ("GET", "/app.css") => text_asset("text/css; charset=utf-8", APP_CSS),
        ("GET", "/sw.js") => text_asset("text/javascript; charset=utf-8", SW_JS),
        ("GET", "/manifest.webmanifest") => {
            text_asset("application/manifest+json; charset=utf-8", MANIFEST)
        }
        ("GET", "/themes.json") => {
            let mut out = Outgoing::json((200, webcal::themes_json()));
            out.headers
                .push(("Cache-Control".into(), "public, max-age=3600".into()));
            out
        }
        _ => return None,
    })
}

/// The page's own files: kept an hour, asked again after.
fn text_asset(content_type: &str, body: &str) -> Outgoing {
    let mut out = asset(content_type, body.as_bytes());
    out.headers = vec![("Cache-Control".into(), "no-cache".into())];
    out
}

fn app_page() -> Outgoing {
    let mut out = html_out(200, INDEX_HTML.to_string());
    // Frames from elsewhere stay out; the app needs no other origin but
    // the font.
    out.headers.push((
        "Content-Security-Policy".into(),
        "default-src 'self'; style-src 'self' 'unsafe-inline' https://fonts.googleapis.com; \
         font-src https://fonts.gstatic.com; img-src 'self' data:; frame-ancestors 'none'; \
         base-uri 'none'; form-action 'self'"
            .into(),
    ));
    out
}

/// Whether the browser would rather read Spanish.
fn spanish(req: &Incoming) -> bool {
    req.language
        .as_deref()
        .and_then(|l| l.split(',').next())
        .is_some_and(|l| l.trim().to_ascii_lowercase().starts_with("es"))
}

/// The token the browser sent: its cookie, or a bearer (scripts, tests).
fn token_of(req: &Incoming) -> Option<String> {
    let from_cookie = req.cookie.as_deref().and_then(|c| {
        c.split(';').find_map(|kv| {
            let (k, v) = kv.trim().split_once('=')?;
            (k == COOKIE && !v.is_empty()).then(|| v.to_string())
        })
    });
    from_cookie.or_else(|| {
        req.auth
            .as_deref()?
            .strip_prefix("Bearer ")
            .map(|t| t.trim().to_string())
    })
}

/// A browser signed in.
struct Session {
    keys: Keys,
    tz: chrono_tz::Tz,
}

fn session(relay: &Relay, req: &Incoming) -> Option<Session> {
    let token = token_of(req)?;
    let hash = keys::token_hash(&token);
    let (id, scope, tz, wrapped, expires): (String, String, String, String, i64) =
        with_conn(relay, |c| {
            c.query_row(
                "SELECT id, scope, tz, access_wrapped, access_expires FROM assistants
                 WHERE access_hash = ?1 AND client = ?2",
                params![hash, WEB_CLIENT],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()
        })
        .flatten()?;
    let now_s = unix();
    if expires < now_s {
        with_conn(relay, |c| {
            c.execute("DELETE FROM assistants WHERE id = ?1", [&id])
        });
        return None;
    }
    let keys = Keys::from_secret(&keys::unwrap(&wrapped, &token)?)?;
    // A remembered browser stays signed in while it keeps coming back.
    let remembered = scope.split_whitespace().any(|s| s == REMEMBER);
    let expires = if remembered {
        now_s + REMEMBER_SECS
    } else {
        expires
    };
    with_conn(relay, |c| {
        c.execute(
            "UPDATE assistants SET last_used = ?2, access_expires = ?3 WHERE id = ?1",
            params![id, now(), expires],
        )
    });
    Some(Session {
        keys,
        tz: tz.parse().unwrap_or(chrono_tz::UTC),
    })
}

/// "Safari on iPhone": which browser, for Settings.
fn browser_name(ua: Option<&str>) -> String {
    let ua = ua.unwrap_or_default();
    let device = [
        ("iPhone", "iPhone"),
        ("iPad", "iPad"),
        ("Android", "Android"),
        ("Macintosh", "Mac"),
        ("Windows", "Windows"),
        ("CrOS", "ChromeOS"),
        ("Linux", "Linux"),
    ]
    .iter()
    .find(|(k, _)| ua.contains(k))
    .map(|(_, v)| *v);
    let browser = [
        ("Edg/", "Edge"),
        ("OPR/", "Opera"),
        ("Firefox/", "Firefox"),
        ("FxiOS", "Firefox"),
        ("CriOS", "Chrome"),
        ("Chrome/", "Chrome"),
        ("Safari/", "Safari"),
    ]
    .iter()
    .find(|(k, _)| ua.contains(k))
    .map(|(_, v)| *v);
    match (browser, device) {
        (Some(b), Some(d)) => format!("Web · {b} on {d}"),
        (Some(x), None) | (None, Some(x)) => format!("Web · {x}"),
        (None, None) => "Web".to_string(),
    }
}

struct Words {
    kicker: &'static str,
    title: &'static str,
    sub: &'static str,
    email: &'static str,
    password: &'static str,
    remember: &'static str,
    remember_sub: &'static str,
    note: &'static str,
    button: &'static str,
    no_account: &'static str,
    wrong: &'static str,
    oops: &'static str,
}

const EN: Words = Words {
    kicker: "Your calendar",
    title: "Sign in to <em>tasq</em>",
    sub: "With the account your computers sync with.",
    email: "Email",
    password: "Password",
    remember: "Remember this browser",
    remember_sub: "Stay signed in here while you keep using it (up to 90 days away).",
    note: "While you're signed in here, tasq's server reads your tasks and the calendars you share on all your devices each time this page asks, to draw them. The rest of the time your data stays end-to-end encrypted. Sign out here, or disconnect it in tasq → Settings → Sync.",
    button: "Sign in",
    no_account: "There's no tasq account for that email on this server. Turn on Sync in tasq on your computer first (Settings → Sync).",
    wrong: "That password isn't right.",
    oops: "Something went wrong. Try again.",
};

const ES: Words = Words {
    kicker: "Tu calendario",
    title: "Entra en <em>tasq</em>",
    sub: "Con la cuenta con la que sincronizan tus ordenadores.",
    email: "Email",
    password: "Contraseña",
    remember: "Recordar este navegador",
    remember_sub: "Sigue con la sesión abierta mientras lo uses (hasta 90 días sin entrar).",
    note: "Mientras tengas la sesión abierta aquí, el servidor de tasq lee tus tareas y los calendarios que compartes en todos tus dispositivos cada vez que esta página lo pide, para dibujarlos. El resto del tiempo tus datos siguen cifrados de extremo a extremo. Cierra la sesión aquí o desconéctala en tasq → Ajustes → Sync.",
    button: "Entrar",
    no_account: "No hay ninguna cuenta de tasq con ese email en este servidor. Activa primero Sync en tasq en tu ordenador (Ajustes → Sync).",
    wrong: "Esa contraseña no es correcta.",
    oops: "Algo ha fallado. Inténtalo de nuevo.",
};

fn login_page(es: bool, email: &str, error: Option<&str>) -> Outgoing {
    let w = if es { &ES } else { &EN };
    let err = error
        .map(|e| format!("<p class=\"error\">{}</p>", esc_html(e)))
        .unwrap_or_default();
    let inner = format!(
        r#"<p class="kicker">{kicker}</p><h1>{title}</h1>
<p class="sub">{sub}</p>
<form method="post" action="/login">
<input type="hidden" name="tz" id="tz" value="">
<label for="email">{email_l}</label>
<input type="email" id="email" name="email" autocomplete="username" required value="{email}">
<label for="password">{password}</label>
<input type="password" id="password" name="password" autocomplete="current-password" required minlength="10">
<label class="choice"><input type="checkbox" name="remember" value="1" checked>
<span>{remember}<small>{remember_sub}</small></span></label>
<p class="note">{note}</p>
{err}
<button type="submit">{button}</button>
</form>
<script>try{{document.getElementById('tz').value=Intl.DateTimeFormat().resolvedOptions().timeZone||''}}catch(e){{}}</script>"#,
        kicker = w.kicker,
        title = w.title,
        sub = w.sub,
        email_l = w.email,
        email = esc_html(email),
        password = w.password,
        remember = w.remember,
        remember_sub = w.remember_sub,
        note = w.note,
        button = w.button,
    );
    let html = page("tasq", &inner)
        .replacen("<html lang=\"en\">", if es { "<html lang=\"es\">" } else { "<html lang=\"en\">" }, 1)
        .replacen(
            "<link rel=\"apple-touch-icon\" href=\"/apple-touch-icon.png\">",
            "<link rel=\"apple-touch-icon\" href=\"/apple-touch-icon.png\">\
             <link rel=\"manifest\" href=\"/manifest.webmanifest\">\
             <meta name=\"apple-mobile-web-app-capable\" content=\"yes\">\
             <meta name=\"apple-mobile-web-app-title\" content=\"tasq\">",
            1,
        )
        .replace(
            "input[type=email], input[type=password] {",
            ".choice input[type=checkbox] { width:auto; } input[type=email], input[type=password] {",
        );
    html_out(if error.is_some() { 400 } else { 200 }, html)
}

fn cookie(req: &Incoming, value: &str, max_age: Option<i64>) -> (String, String) {
    let secure = base_url(req).starts_with("https://");
    let mut c = format!("{COOKIE}={value}; Path=/; HttpOnly; SameSite=Lax");
    if secure {
        c.push_str("; Secure");
    }
    if let Some(age) = max_age {
        c.push_str(&format!("; Max-Age={age}"));
    }
    ("Set-Cookie".into(), c)
}

fn login(relay: &Relay, req: &Incoming, form: &[(String, String)], es: bool) -> Outgoing {
    let w = if es { &ES } else { &EN };
    let email = field(form, "email").unwrap_or_default().trim();
    let password = field(form, "password").unwrap_or_default();
    let keys = match Keys::derive(email, password) {
        Ok(k) => k,
        Err(e) => return login_page(es, email, Some(&e.to_string())),
    };
    let stored: Option<String> = with_conn(relay, |c| {
        c.query_row(
            "SELECT auth_hash FROM accounts WHERE id = ?1",
            [&keys.account],
            |r| r.get(0),
        )
        .optional()
    })
    .flatten();
    match stored {
        None => return login_page(es, email, Some(w.no_account)),
        Some(h) if !keys.matches(&h) => return login_page(es, email, Some(w.wrong)),
        Some(_) => {}
    }
    let remember = field(form, "remember").is_some();
    let tz = field(form, "tz")
        .filter(|t| t.parse::<chrono_tz::Tz>().is_ok())
        .unwrap_or("UTC")
        .to_string();
    let Ok(t) = issue(&keys.to_secret()) else {
        return html_out(500, page_message("tasq", w.oops));
    };
    let life = if remember {
        REMEMBER_SECS
    } else {
        SESSION_SECS
    };
    let scope = if remember {
        format!("{READ} {REMEMBER}")
    } else {
        READ.to_string()
    };
    let saved = with_conn(relay, |c| {
        c.execute(
            "INSERT INTO assistants (id, account, client, name, scope, tz, access_hash, access_wrapped,
                access_expires, refresh_hash, refresh_wrapped, created_at, last_used)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12)",
            params![
                crate::core::db::new_ulid(),
                keys.account,
                WEB_CLIENT,
                browser_name(req.user_agent.as_deref()),
                scope,
                tz,
                keys::token_hash(&t.access),
                t.access_wrapped,
                unix() + life,
                // The web never refreshes: this one is never handed out.
                keys::token_hash(&t.refresh),
                t.refresh_wrapped,
                now()
            ],
        )
    });
    if saved.is_none() {
        return html_out(500, page_message("tasq", w.oops));
    }
    let mut out = redirect("/");
    out.headers
        .push(cookie(req, &t.access, remember.then_some(REMEMBER_SECS)));
    out
}

fn logout(relay: &Relay, req: &Incoming) -> Outgoing {
    if let Some(token) = token_of(req) {
        with_conn(relay, |c| {
            c.execute(
                "DELETE FROM assistants WHERE access_hash = ?1 AND client = ?2",
                params![keys::token_hash(&token), WEB_CLIENT],
            )
        });
    }
    let mut out = redirect("/login");
    out.headers.push(cookie(req, "", Some(0)));
    out
}

fn json_error(code: u16, msg: &str) -> Outgoing {
    let mut out = Outgoing::json((code, json!({ "error": msg })));
    out.headers
        .push(("Cache-Control".into(), "no-store".into()));
    out
}

fn calendar(relay: &Relay, req: &Incoming, query: &str) -> Outgoing {
    let Some(s) = session(relay, req) else {
        return json_error(401, "sign in first");
    };
    let q = parse_form(query);
    let tz: chrono_tz::Tz = field(&q, "tz").and_then(|t| t.parse().ok()).unwrap_or(s.tz);
    let local = chrono::Utc::now().with_timezone(&tz);
    let today = local.date_naive();
    let minute = local.hour() * 60 + local.minute();
    let date = |k: &str| field(&q, k).and_then(|v| NaiveDate::parse_from_str(v, "%Y-%m-%d").ok());
    let from = date("from").unwrap_or(today);
    let to = date("to").unwrap_or(from);
    if to < from || (to - from).num_days() >= MAX_DAYS {
        return json_error(400, "from..to: at most 42 days, in order");
    }
    let a = match Account::open(relay, &s.keys, today) {
        Ok(a) => a,
        Err(e) => return json_error(500, &e.to_string()),
    };
    let (occs, ext) = with_external(&a, from, to, today, tz);
    let v: Value = webcal::calendar_json(&a, &occs, &ext, from, to, today, minute, tz.name());
    let mut out = Outgoing::json((200, v));
    out.headers
        .push(("Cache-Control".into(), "no-store".into()));
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::cloud::client::{Local, Remote};
    use crate::cloud::run::sync_once;
    use crate::core::db::Db;
    use crate::todo;

    fn req(method: &str, url: &str, cookie: Option<&str>, body: &str) -> Incoming {
        Incoming {
            method: method.into(),
            url: url.into(),
            host: Some("tasq.example".into()),
            cookie: cookie.map(str::to_string),
            user_agent: Some(
                "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 \
                 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1"
                    .into(),
            ),
            language: Some("es-ES,es;q=0.9".into()),
            body: body.as_bytes().to_vec(),
            ..Default::default()
        }
    }

    fn setup(lines: &[&str], today: NaiveDate) -> (Relay, Keys) {
        let relay = Relay::in_memory().unwrap();
        let keys = Keys::derive("me@x.com", "my long password").unwrap();
        let mut remote = Local {
            relay: &relay,
            keys: keys.clone(),
        };
        remote.signup().unwrap();
        let mut mac = Db::in_memory();
        mac.set_space_color("Work", Some("#8aadf4")).unwrap();
        let mut live: Vec<_> = lines.iter().map(|l| todo::parse_line(l).unwrap()).collect();
        mac.save(Some(&mut live), None).unwrap();
        sync_once(&mut mac, &mut remote, &keys, &today.to_string()).unwrap();
        (relay, keys)
    }

    fn sign_in(relay: &Relay) -> String {
        let wrong = relay.respond(&req(
            "POST",
            "/login",
            None,
            "email=me%40x.com&password=not+the+password",
        ));
        assert_eq!(wrong.code, 400);
        assert!(String::from_utf8_lossy(&wrong.body).contains("Esa contraseña no es correcta"));
        let out = relay.respond(&req(
            "POST",
            "/login",
            None,
            "email=me%40x.com&password=my+long+password&remember=1&tz=Europe%2FMadrid",
        ));
        assert_eq!(out.code, 302);
        let set = &out
            .headers
            .iter()
            .find(|(k, _)| k == "Set-Cookie")
            .unwrap()
            .1;
        assert!(set.contains("HttpOnly") && set.contains("Secure") && set.contains("Max-Age="));
        let token = set
            .strip_prefix("tasq_web=")
            .unwrap()
            .split(';')
            .next()
            .unwrap();
        format!("{COOKIE}={token}")
    }

    #[test]
    fn signing_in_shows_the_app_and_settings_lists_the_browser() {
        let today = chrono::Utc::now()
            .with_timezone(&chrono_tz::Europe::Madrid)
            .date_naive();
        let (relay, keys) = setup(&["Buy beans"], today);
        assert_eq!(relay.respond(&req("GET", "/", None, "")).code, 302);
        let page = relay.respond(&req("GET", "/login", None, ""));
        assert!(String::from_utf8_lossy(&page.body).contains("Recordar este navegador"));
        assert_eq!(
            relay.respond(&req("GET", "/v1/calendar", None, "")).code,
            401
        );
        let c = sign_in(&relay);
        let app = relay.respond(&req("GET", "/", Some(&c), ""));
        assert_eq!(app.code, 200);
        assert!(String::from_utf8_lossy(&app.body).contains("app.js"));

        // A web sign-in isn't an MCP token.
        let token = c.split('=').nth(1).unwrap();
        let mcp = relay.respond(&Incoming {
            auth: Some(format!("Bearer {token}")),
            ..req(
                "POST",
                "/mcp",
                None,
                r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#,
            )
        });
        assert_eq!(mcp.code, 401);

        // Settings sees it, and can let it go.
        let bearer = format!("Bearer {}", keys.bearer());
        let list = relay.respond(&Incoming {
            auth: Some(bearer.clone()),
            ..req("GET", "/v1/assistants", None, "")
        });
        let v: Value = serde_json::from_slice(&list.body).unwrap();
        assert_eq!(v["assistants"][0]["kind"], "web");
        assert_eq!(v["assistants"][0]["name"], "Web · Safari on iPhone");
        relay.respond(&Incoming {
            auth: Some(bearer),
            ..req("POST", "/v1/assistants/revoke", None, r#"{"kind":"web"}"#)
        });
        assert_eq!(relay.respond(&req("GET", "/", Some(&c), "")).code, 302);
    }

    #[test]
    fn signing_out_forgets_the_browser() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 9).unwrap();
        let (relay, _) = setup(&[], today);
        let c = sign_in(&relay);
        let out = relay.respond(&req("POST", "/logout", Some(&c), ""));
        assert_eq!(out.code, 302);
        assert_eq!(
            relay
                .respond(&req("GET", "/v1/calendar", Some(&c), ""))
                .code,
            401
        );
    }

    #[test]
    fn the_calendar_comes_worked_out() {
        let today = chrono::Utc::now()
            .with_timezone(&chrono_tz::Europe::Madrid)
            .date_naive();
        let day = |n: i64| (today + chrono::Duration::days(n)).to_string();
        let (relay, _) = setup(
            &[
                &format!("Standup +Work plan:{} at:09:00 dur:1h", day(1)),
                &format!("Review +Work plan:{} at:09:30", day(1)),
                &format!("Gym plan:{} rec:+1d at:07:00 dur:45m", day(1)),
                &format!("Pay rent due:{}", day(2)),
                &format!("Trip +Travel plan:{} end:{} event:1", day(1), day(3)),
            ],
            today,
        );
        let c = sign_in(&relay);
        let url = format!(
            "/v1/calendar?from={}&to={}&tz=Europe%2FMadrid",
            day(1),
            day(3)
        );
        let out = relay.respond(&req("GET", &url, Some(&c), ""));
        assert_eq!(out.code, 200, "{}", String::from_utf8_lossy(&out.body));
        let v: Value = serde_json::from_slice(&out.body).unwrap();
        let d1 = &v["days"][0];
        let timed = d1["timed"].as_array().unwrap();
        let by = |t: &str| timed.iter().find(|i| i["title"] == t).unwrap().clone();
        assert_eq!(by("Gym")["start"], 420);
        assert_eq!(by("Gym")["end"], 465);
        assert_eq!(by("Standup")["color"], "#8aadf4");
        assert_eq!(by("Standup")["lanes"], 2);
        assert_eq!(by("Review")["lane"], 1);
        assert_eq!(by("Review")["minutes"], 30, "30 minutes with no dur:");
        // The trip runs three days across the top.
        let trip = &d1["all_day"][0];
        assert_eq!(
            (trip["title"].clone(), trip["span"].clone()),
            (json!("Trip"), json!(3))
        );
        assert_eq!(trip["kind"], "event");
        // Tomorrow's gym repeats the day after, projected.
        let gym2 = v["days"][1]["timed"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["title"] == "Gym")
            .unwrap()
            .clone();
        assert_eq!(gym2["projected"], true);
        // The deadline sits on its day.
        let rent = v["days"][1]["all_day"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["title"] == "Pay rent")
            .unwrap()
            .clone();
        assert_eq!(rent["deadline"], true);
        // Free gaps around the blocks, from 08:00.
        assert_eq!(d1["free"][0], json!([480, 540]));
        // Details once per task.
        let key = by("Standup")["key"].as_str().unwrap().to_string();
        assert_eq!(v["details"][&key]["space"], "Work");
        assert_eq!(v["timezone"], "Europe/Madrid");
        // Too wide a range is refused.
        let wide = format!("/v1/calendar?from={}&to={}", day(0), day(60));
        assert_eq!(relay.respond(&req("GET", &wide, Some(&c), "")).code, 400);
    }

    #[test]
    fn your_own_themes_come_with_your_account() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 9).unwrap();
        let (relay, keys) = setup(&[], today);
        // A computer shares a theme file; the web gets it with the days.
        let mut mac = Db::in_memory();
        let mut remote = Local {
            relay: &relay,
            keys: keys.clone(),
        };
        sync_once(&mut mac, &mut remote, &keys, "2026-11-09").unwrap();
        mac.put_shared_theme("Solarized Dark", crate::theme::tests::FULL_THEME)
            .unwrap();
        sync_once(&mut mac, &mut remote, &keys, "2026-11-09").unwrap();
        let c = sign_in(&relay);
        let out = relay.respond(&req(
            "GET",
            "/v1/calendar?from=2026-11-09&to=2026-11-09",
            Some(&c),
            "",
        ));
        let v: Value = serde_json::from_slice(&out.body).unwrap();
        assert_eq!(v["themes"][0]["name"], "Solarized Dark", "{v}");
        assert_eq!(v["themes"][0]["bg"], "#002b36");
        assert_eq!(v["themes"][0]["palette"].as_array().unwrap().len(), 16);
        // Another computer gets it too.
        let mut laptop = Db::in_memory();
        sync_once(&mut laptop, &mut remote, &keys, "2026-11-09").unwrap();
        assert_eq!(laptop.shared_themes().unwrap().len(), 1);
    }

    #[test]
    fn browsers_get_a_name() {
        assert_eq!(
            browser_name(Some(
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Safari/537.36"
            )),
            "Web · Chrome on Mac"
        );
        assert_eq!(browser_name(None), "Web");
    }
}

/// A server with a made-up account, to look at the web app (and take the
/// README's screenshots): `cargo test --lib web_demo -- --ignored`, then
/// open http://127.0.0.1:8099 and sign in as `demo@tasq.app` /
/// `demo password 123`. Its other calendar is made up too.
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod demo {
    use super::*;
    use crate::cloud::client::{Local, Remote};
    use crate::cloud::run::sync_once;
    use crate::core::db::Db;
    use crate::extcal::accounts::{CalendarChoice, ExtAccount, shared_value};
    use crate::extcal::sync::tests::Fake;
    use crate::todo;
    use chrono::Datelike;

    #[test]
    #[ignore = "serves until killed"]
    fn web_demo() {
        let tz: chrono_tz::Tz = std::env::var("TASQ_DEMO_TZ")
            .ok()
            .and_then(|t| t.parse().ok())
            .unwrap_or(chrono_tz::Europe::Madrid);
        let today = chrono::Utc::now().with_timezone(&tz).date_naive();
        let day = |n: i64| (today + chrono::Duration::days(n)).to_string();
        let relay = Relay::in_memory().unwrap();
        let keys = Keys::derive("demo@tasq.app", "demo password 123").unwrap();
        let mut remote = Local {
            relay: &relay,
            keys: keys.clone(),
        };
        remote.signup().unwrap();
        let mut db = Db::in_memory();
        db.set_space_color("Work", Some("0")).unwrap();
        db.set_space_color("Health", Some("2")).unwrap();
        db.set_space_color("Uni", Some("1")).unwrap();
        db.set_space_color("Home", Some("3")).unwrap();
        let lines = [
            format!(
                "Gym +Health plan:{} rec:+1w:mon,wed,fri at:07:00 dur:1h",
                day(-((today.weekday().num_days_from_monday()) as i64))
            ),
            format!(
                "(A) Ship the release notes +Work plan:{} at:10:00 dur:1h30m",
                day(0)
            ),
            format!("Review PR #96 +Work plan:{} at:10:30 dur:45m", day(0)),
            format!(
                "Lunch with Ana +Home plan:{} at:14:00 dur:1h event:1",
                day(0)
            ),
            format!(
                "Study algorithms +Uni/Exams plan:{} at:16:30 dur:2h notes:demo1/",
                day(0)
            ),
            format!("Pay rent +Home due:{}", day(2)),
            format!("Buy coffee beans +Home plan:{}", day(0)),
            format!("Call the bank +Home plan:{}", day(-2)),
            format!(
                "Conference trip +Work plan:{} end:{} event:1",
                day(3),
                day(5)
            ),
            format!("Read chapter 4 +Uni plan:{} at:18:00 dur:1h", day(1)),
            format!("Dentist +Health plan:{} at:17:00 dur:45m", day(2)),
            format!("Plan sprint +Work plan:{} at:09:30 dur:1h", day(1)),
            format!("Water the plants +Home plan:{} rec:+3d", day(0)),
        ];
        let mut live: Vec<_> = lines.iter().map(|l| todo::parse_line(l).unwrap()).collect();
        db.save(Some(&mut live), None).unwrap();
        db.put_note(
            "tasks/demo1/main.md",
            "# Exam prep\n\n- [x] Sorting\n- [x] Graphs\n- [ ] Dynamic programming\n\nSlides: https://example.com/algorithms\n",
        )
        .ok();
        let mut acc = ExtAccount::new("me@company.com");
        acc.shared = true;
        acc.calendars.push(CalendarChoice {
            id: "primary".into(),
            name: "Company".into(),
            primary: true,
            shown: true,
            color: None,
        });
        db.put_shared_account(&acc.email, &shared_value(&acc, "demo", None))
            .unwrap();
        sync_once(&mut db, &mut remote, &keys, &today.to_string()).unwrap();

        let at = |n: i64, hm: &str| {
            let d = today + chrono::Duration::days(n);
            let t = chrono::NaiveTime::parse_from_str(hm, "%H:%M").unwrap();
            tz.from_local_datetime(&d.and_time(t)).unwrap().to_rfc3339()
        };
        use chrono::TimeZone;
        let meetings = vec![
            json!({ "id": "m1", "summary": "Weekly sync", "start": { "dateTime": at(0, "12:00") },
                "end": { "dateTime": at(0, "12:45") }, "hangoutLink": "https://meet.google.com/abc-defg-hij",
                "htmlLink": "https://www.google.com/calendar/event?eid=bTE", "location": "Room 3 / Meet",
                "description": "Agenda:<br>1. Release status<br>2. Hiring<br>Doc: <a href=\"https://docs.google.com/document/d/1\">notes</a>",
                "attendees": [
                    { "email": "lead@company.com", "displayName": "Marta", "organizer": true, "responseStatus": "accepted" },
                    { "email": "me@company.com", "self": true, "responseStatus": "accepted" },
                    { "email": "dev@company.com", "displayName": "Luis", "responseStatus": "tentative" } ] }),
            json!({ "id": "m2", "summary": "1:1 with Marta", "start": { "dateTime": at(1, "11:00") },
                "end": { "dateTime": at(1, "11:30") }, "hangoutLink": "https://meet.google.com/xyz",
                "attendees": [ { "email": "me@company.com", "self": true, "responseStatus": "needsAction" } ] }),
            json!({ "id": "m3", "summary": "Design review", "start": { "dateTime": at(2, "15:00") },
                "end": { "dateTime": at(2, "16:00") } }),
            json!({ "id": "m4", "summary": "Team offsite", "start": { "date": day(8) }, "end": { "date": day(10) } }),
        ];
        relay.externals.set_maker(Box::new(move |_, _, _| {
            let mut f = Fake::default();
            for m in &meetings {
                f.put(m.clone());
            }
            Some(Box::new(f))
        }));
        let port = std::env::var("TASQ_DEMO_PORT").unwrap_or_else(|_| "8099".into());
        let server = tiny_http::Server::http(format!("127.0.0.1:{port}")).unwrap();
        eprintln!("web demo on http://127.0.0.1:{port} · demo@tasq.app / demo password 123");
        crate::cloud::relay::serve_on(server, relay).unwrap();
    }
}
