# Seeing tasq in Google Calendar and Apple Calendar

A study for releases 7 (*Calendar, local*) and 10 (*Connected calendars*)
in [DESIGN.md](../DESIGN.md#7-releases): what can be done, what each way
costs, and an order to do it in. Nothing here is built yet.

The design already fixes the shape: **one-way** (tasq → calendar), and
**one calendar per published space** (Uni › Exams), since neither Google
nor Apple nests calendars.

## What a task becomes

Every way below ends in the same iCalendar (RFC 5545) data, so the mapping
is written once.

| tasq | iCalendar |
| --- | --- |
| task id (database) | `UID` — stable, so an edit updates the event instead of adding one. A todo.txt opened directly has no id; a hash of the line would change on every edit, so feeds need the database. |
| `plan:` + `at:` + `dur:` | `DTSTART` with a time + `DURATION` (30 min when there's none) |
| `plan:` without `at:` | all-day: `DTSTART;VALUE=DATE` |
| `end:` (several days) | `DTEND;VALUE=DATE`, the day *after* the last one (it's exclusive) |
| `rec:+1w:mon,wed` | `RRULE:FREQ=WEEKLY;BYDAY=MO,WE` |
| `until:` / `times:` | `UNTIL=` / `COUNT=` in the rule |
| `skip:` | `EXDATE` for each date |
| `remind:15m` | `VALARM` with `TRIGGER:-PT15M` |
| `due:` only (a deadline) | an all-day event "◷ title" on that day: Google ignores `VTODO`, so to-dos as such would disappear there |
| done | left out of the feed (or kept, struck through in the title, as a setting) |
| the note | `DESCRIPTION`, first lines only |

The tasq repeat rules already follow RRULE (DESIGN.md §2), so the rule
translates directly. The one gap is business days (`rec:3b`). They have no
exact RRULE; the feed would list those dates one by one instead.

## The ways in

### 1. An `.ics` file (export)

`tasq export --ics [--space Uni/Exams] > uni-exams.ics`, then *File →
Import* in either app.

- **Works with:** Google, Apple, Outlook, anything.
- **Needs:** nothing. It can be done now.
- **But:** it's a one-off copy. Nothing updates, and importing twice
  duplicates events in Google.
- **Use:** moving to another app, a backup, or sharing an exam timetable.

### 2. A feed the calendar subscribes to (`webcal://`)

tasq serves each published space as an `.ics` URL. The calendar app
fetches it now and then and shows it as a read-only calendar. This is
release 7's "feed from your machine".

The existing share server (`s`, `src/serve/`) is the natural home. It
already binds a port on the local network and gates every route behind a
token (`/t/<token>/…`). A route `/t/<token>/cal/<space>.ics` is a small
addition.

- **Apple Calendar on the Mac:** *File → New Calendar Subscription* with
  `webcal://127.0.0.1:<port>/t/<token>/cal/uni-exams.ics`. Refresh can be
  set as often as every 5 minutes. This works fully with no server of
  ours. The catch: the Mac only refreshes while tasq (or its server) is
  running.
- **iPhone:** it fetches the feed itself, so it needs the Mac's address.
  That works on the same Wi-Fi only, and not when the Mac is asleep.
- **Google Calendar:** *Other calendars → From URL* needs an address on
  the public internet, because Google's servers fetch it, not your
  browser. A local address won't work. Google also refreshes subscriptions
  on its own schedule, roughly every few hours up to a day, and it can't
  be sped up.
- **Reaching the internet without our server:** the user can expose the
  feed with a tunnel (Tailscale Funnel, Cloudflare Tunnel). That's fine as
  a documented "advanced" option, not as the default.

**Verdict:** this is the free tier as designed. It's excellent for Apple on
the Mac and acceptable on the iPhone at home. Google works only with a
tunnel, and is slow even then.

### 3. tasq writes the events itself

Instead of being fetched, tasq (or later the server) creates and updates
the events. Changes show in seconds, and the calendar has no feed to
reach.

**Google: the Calendar API.**
- OAuth "installed app" flow: the browser opens, you allow, and tasq
  listens on `localhost` for the answer. No server of ours is needed for
  the sign-in.
- The scope to ask for is **`calendar.app.created`**. With it, tasq can
  create its own calendars and touch only those, never your other events.
  It's the narrowest scope that fits "one calendar per space" exactly.
- Google rates calendar scopes as *sensitive*. Until the app passes
  Google's verification, the consent screen shows a warning, and it's
  limited to 100 test users. Verification is a review of a privacy policy
  and a short video, with no security audit. It's paperwork to plan for
  before a public release.
- Updates: tasq keeps the event id per task and occurrence, and sends
  insert, patch or delete on change. Rate limits are generous for one
  person's tasks.

**Apple: iCloud over CalDAV.**
- iCloud speaks CalDAV (`caldav.icloud.com`). With an app-specific
  password (made at account.apple.com), any program can create a calendar
  (`MKCALENDAR`) and put events in it (`PUT` of `.ics`).
- It reuses the same iCalendar mapping, it works from Linux or the server
  too, and it reaches every Apple device through iCloud.
- The catch is setup: the user has to make the app-specific password by
  hand. There's no OAuth for iCloud.
- DESIGN.md lists "Apple calendar creation from the server (CalDAV)" as an
  open question. The answer is **yes, with an app-specific password**. A
  first test against a real account should confirm `MKCALENDAR` on iCloud
  before we rely on it.

**Apple: EventKit (macOS only).**
- This is the native framework, writing into Calendar.app's own store,
  which iCloud then syncs.
- A plain terminal binary can't easily get the Calendar permission (the
  prompt and its entitlements belong to signed app bundles). It would need
  a small signed helper app.
- It only works on a Mac, and CalDAV already covers this case.
  **Not worth it.**

### 4. Two-way

This means an event moved in Google moving the task.

- **Google:** possible through the API's sync tokens (polling) or push
  notifications (which need a public HTTPS endpoint, so the server).
- **iCloud:** possible through CalDAV's `ctag` and `etag` polling.
- **The hard part isn't transport, it's meaning.** A moved occurrence of a
  repeat, an event edited on both sides, a task deleted in the calendar.
  It's the same "only this one / from here on" logic tasq now has (`skip:`,
  `until:`), but with conflicts.

DESIGN.md parks it. That still looks right.

## Recommended order

1. **Now, small (part of release 7):**
   - `tasq export --ics` with `--space`, and the iCalendar mapping above
     with its tests. Everything after reuses it.
2. **Release 7 proper:**
   - A token-gated feed per published space in the share server.
   - A *Publish to calendar* toggle on a space (sidebar `P`, or the space's
     settings), with the subscription URL and a QR code to scan on the
     phone.
   - Documentation for Apple on the Mac and the iPhone, plus the tunnel
     option for Google.
3. **Release 10 (with accounts and the server):**
   - The server keeps each published space's events up to date in:
     - **Google**, through the API with `calendar.app.created`;
     - **iCloud**, through CalDAV with an app-specific password.
   - Both are set up from the profile. Changes show within seconds, also
     while the computer is off.
   - Start Google's verification ahead of the release.
4. **Later, maybe:** two-way, starting with Google, if people ask for it.

## Open points to settle

- Should done tasks stay in the calendar? (Proposed: no, with a setting.)
- What should deadlines look like? (Proposed: an all-day "◷ title" event;
  later perhaps a separate "Deadlines" calendar per space.)
- Should a published space include its sub-spaces? (Proposed: yes, like the
  sidebar's counts.)
- Should a published space get its own calendar colour? Google and iCloud
  both accept one, so the space's colour should be the default.
