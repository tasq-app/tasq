# Privacy policy

_Last updated: 9 November 2026_

tasq is a to-do app that runs on your computer. It has no servers and no
accounts of its own, and it collects nothing about you: no analytics, no
telemetry, no tracking.

## Your tasks and notes

They stay on your computer, in tasq's database or in your todo.txt files.
They go nowhere unless you turn on one of the features below.

## Google Calendar (optional)

If you connect Google Calendar in Settings, tasq asks Google for:

- **`calendar.app.created`**: to create one calendar called "tasq" in your
  account and add, change and remove events in it. tasq can't see or change
  your other calendars.
- **`openid` and `email`**: to show which Google account is connected.

With that, tasq sends Google the tasks that have a date: the title, the
day and time, how long the task takes, how often it repeats, its reminders,
and the colour of its space. This goes straight from your computer to
Google's API, with no server of ours in between. Google keeps it in the
"tasq" calendar under
[Google's privacy policy](https://policies.google.com/privacy).

The access Google grants (a refresh token) is kept on your computer, in the
system keychain or in a file only your user can read. Nobody else receives
it. tasq does not share, sell or use this data for anything other than
keeping that calendar up to date. Its use of information received from
Google APIs adheres to the
[Google API Services User Data Policy](https://developers.google.com/terms/api-services-user-data-policy),
including the Limited Use requirements.

**Disconnect Google** in Settings revokes the access and deletes the token
from your computer. You can also revoke it at
[myaccount.google.com/permissions](https://myaccount.google.com/permissions).
To remove the events, delete the "tasq" calendar in Google Calendar.

## Your other calendars (optional, read-only)

You can also connect one or more Google accounts (a work account, your
personal one) so tasq shows their events next to your tasks. For each
account, tasq asks Google for:

- **`calendar.calendarlist.readonly`**: to list that account's calendars,
  so you can choose which ones to show.
- **`calendar.events.readonly`**: to read the events in the calendars you
  chose: title, time, place, description, who is invited and whether you
  said yes, and the links to the event, its video call and its attached
  files.
- **`openid` and `email`**: to show which account is connected.

This access is read-only: tasq can't create, change, answer or delete
anything in those calendars. The events are read straight from your
computer to Google's API, and kept in a local cache on your computer so
you can see them offline. (The one exception is the web calendar or Claude,
if you turn them on: see below.) They are never
turned into tasks, never written to any calendar (not even the "tasq" one)
and never sent anywhere else.

Each account's access (a refresh token) is kept on your computer like the
Google Calendar one above. If you choose to show an account on all your
devices, its token and your choice of calendars also go in tasq's
database on each of your devices and travel between them inside tasq's
sync, end-to-end encrypted: the sync server stores them sealed and can't
read them. An account set to "only this device" never leaves the computer
where you connected it.

tasq does not share, sell or use this data for advertising, and does not
use it to train AI models. Its use of information received from Google APIs
adheres to the
[Google API Services User Data Policy](https://developers.google.com/terms/api-services-user-data-policy),
including the Limited Use requirements.

**Disconnect** an account in Settings revokes its access, deletes its token
and deletes its cached events, on every device where it was shown. You can
also revoke it at
[myaccount.google.com/permissions](https://myaccount.google.com/permissions).

## Apple Calendar (optional)

If you connect Apple Calendar, you give tasq your Apple ID and an
app-specific password, which tasq keeps on your computer (in the system
keychain, or a file only your user can read). With them tasq creates
calendars named "tasq" and "tasq · <space>" in your iCloud account and
sends them the same task details as to Google. It goes straight from your
computer to Apple's CalDAV server; nobody else receives the password or
the tasks. **Disconnect Apple** deletes the password from your computer;
you can also revoke it at account.apple.com.

## Claude and other assistants (optional)

Sync is end-to-end encrypted: tasq's server stores only data it can't
read. Connecting Claude (or another assistant that speaks MCP) is the one
exception, and only if you do it. On tasq's sign-in page you type your
sync email and password; the server makes your keys from them and keeps
them sealed under the token it gives the assistant, so what it stores
can't be read without that token. Each time the assistant asks something,
the server reads your tasks and notes in memory to answer it, and writes
back what you asked for (as one more device). Nothing it reads is written
to disk, logged or used for anything else. The answers go to the
assistant, under its own privacy policy.

Disconnect it in tasq → Settings → Sync → Claude (or remove the connector
in the assistant): the server forgets the keys at once.

## The web calendar (optional)

The web calendar works the same way, for your browser instead of an
assistant: on tasq's sign-in page you type your sync email and password,
the server makes your keys and keeps them sealed under a token that only
your browser holds (a cookie it can't read from scripts). The server keeps
the sealed keys, the token's hash, the time zone, the browser's name (for
example "Safari on iPhone", from the browser's own description) and when
it was last used. Each time the page asks for some days, the server reads
your tasks, notes, spaces and shared themes in memory, works out the
calendar and sends it to your browser. It only reads: the web calendar
can't change anything. Nothing it reads is written to disk or logged.

**Your other calendars, through the server.** While the web calendar or an
assistant is connected, the server also reads the events of the Google
accounts you show on all your devices, to draw them (and to tell Claude
when you're busy). It uses the access those accounts already share inside
your sync (above), asks Google directly with the same read-only scopes,
and keeps what Google sends **only in memory**: never on disk. It asks
Google again for what changed at most every two minutes, and forgets
everything about an account half an hour after the last time you looked,
or as soon as you stop showing it on all your devices. Accounts set to
"only this device" are never read by the server. The events go to your
browser (or the assistant) and nowhere else, and are not used for anything
else.

**Your browser.** The page keeps your choices (theme, view) in the
browser's local storage, and the page itself (not your calendar) in its
cache so it opens offline.

Sign out from the page (⚙ → Sign out), or sign every browser out in
tasq → Settings → Sync → Web calendar: the server forgets the keys at
once. A browser not used for 90 days (12 hours, if you didn't ask to be
remembered) is signed out by itself.

## Update check and phone capture

At startup tasq may ask GitHub whether a newer release exists; only the
request itself is sent. Phone capture, if you turn it on, runs a server on
your own network that only you use.

## Contact

Questions: open an issue at <https://github.com/tasq-app/tasq/issues> or
write to jfgm299@gmail.com.
