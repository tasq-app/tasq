# Privacy policy

_Last updated: 9 October 2026_

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

## Update check and phone capture

At startup tasq may ask GitHub whether a newer release exists; only the
request itself is sent. Phone capture, if you turn it on, runs a server on
your own network that only you use.

## Contact

Questions: open an issue at <https://github.com/tasq-app/tasq/issues> or
write to jfgm299@gmail.com.
