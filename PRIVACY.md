# Privacy policy

_Last updated: 8 November 2026_

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

## Update check and phone capture

At startup tasq may ask GitHub whether a newer release exists; only the
request itself is sent. Phone capture, if you turn it on, runs a server on
your own network that only you use.

## Contact

Questions: open an issue at <https://github.com/tasq-app/tasq/issues> or
write to jfgm299@gmail.com.
