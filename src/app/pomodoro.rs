//! A focus timer (`P`): 25 minutes on a task, then a 5-minute break. It
//! sits pinned at the top right while it runs, with the other notices
//! stacking under it. `P` pauses and resumes; the menu's timer page starts
//! a break or stops it.

use std::time::{Duration, Instant};

use super::{App, ToastKind};

pub const FOCUS: Duration = Duration::from_secs(25 * 60);
pub const BREAK: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Focus,
    Break,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pomodoro {
    pub phase: Phase,
    /// What you're focusing on.
    pub task: Option<String>,
    pub length: Duration,
    /// Time already run before the current stretch (pauses split it).
    pub done_before: Duration,
    /// When the current stretch started; `None` while paused.
    pub running_since: Option<Instant>,
    /// Focus rounds finished.
    pub rounds: u32,
}

impl Pomodoro {
    fn new(phase: Phase, task: Option<String>, rounds: u32, now: Instant) -> Self {
        Self {
            phase,
            task,
            length: match phase {
                Phase::Focus => FOCUS,
                Phase::Break => BREAK,
            },
            done_before: Duration::ZERO,
            running_since: Some(now),
            rounds,
        }
    }

    pub fn elapsed(&self, now: Instant) -> Duration {
        self.done_before
            + self
                .running_since
                .map_or(Duration::ZERO, |s| now.saturating_duration_since(s))
    }

    pub fn left(&self, now: Instant) -> Duration {
        self.length.saturating_sub(self.elapsed(now))
    }

    /// How far along, 0 to 1.
    pub fn progress(&self, now: Instant) -> f32 {
        (self.elapsed(now).as_secs_f32() / self.length.as_secs_f32()).min(1.0)
    }

    pub fn paused(&self) -> bool {
        self.running_since.is_none()
    }

    /// `mm:ss` left.
    pub fn clock(&self, now: Instant) -> String {
        let s = self.left(now).as_secs();
        format!("{:02}:{:02}", s / 60, s % 60)
    }
}

impl App {
    /// `P`: start focusing on the current task, or pause / resume.
    pub fn pomodoro_toggle(&mut self) {
        let now = Instant::now();
        match self.pomodoro.as_mut() {
            None => {
                let task = self.cur_abs().and_then(|a| self.task_title(a));
                self.pomodoro = Some(Pomodoro::new(Phase::Focus, task, 0, now));
                self.toast(
                    ToastKind::Info,
                    "Focus",
                    Some("25 min · P pauses · ^X stops".into()),
                    "focus",
                );
            }
            Some(p) => match p.running_since.take() {
                Some(since) => p.done_before += now.saturating_duration_since(since),
                None => p.running_since = Some(now),
            },
        }
    }

    /// Start a break now.
    pub fn pomodoro_break(&mut self) {
        let (task, rounds) = self
            .pomodoro
            .as_ref()
            .map_or((None, 0), |p| (p.task.clone(), p.rounds));
        self.pomodoro = Some(Pomodoro::new(Phase::Break, task, rounds, Instant::now()));
    }

    pub fn pomodoro_stop(&mut self) {
        if self.pomodoro.take().is_some() {
            self.flash("timer stopped");
        }
    }

    /// Move on when time's up: focus becomes a break, a break ends.
    /// True when something changed on screen (it runs every tick).
    pub fn pomodoro_tick(&mut self, now: Instant) -> bool {
        let Some(p) = self.pomodoro.as_ref() else {
            return false;
        };
        if p.paused() {
            return false;
        }
        if p.left(now) > Duration::ZERO {
            return true;
        }
        match p.phase {
            Phase::Focus => {
                let (task, rounds) = (p.task.clone(), p.rounds + 1);
                let detail = task
                    .clone()
                    .map_or_else(|| "take 5".to_string(), |t| format!("{t} · take 5"));
                self.pomodoro = Some(Pomodoro::new(Phase::Break, task, rounds, now));
                self.toast(ToastKind::Done, "Focus done", Some(detail), "focus done");
            }
            Phase::Break => {
                self.pomodoro = None;
                self.toast(
                    ToastKind::Info,
                    "Break over",
                    Some("P for another round".into()),
                    "break over",
                );
            }
        }
        true
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::app::test_support::build_app;

    #[test]
    fn a_round_runs_pauses_and_turns_into_a_break() {
        let mut app = build_app("Teoría AII\n");
        app.pomodoro_toggle();
        let p = app.pomodoro.clone().unwrap();
        assert_eq!(p.task.as_deref(), Some("Teoría AII"));
        assert_eq!(p.phase, Phase::Focus);
        let t0 = p.running_since.unwrap();
        assert_eq!(p.clock(t0), "25:00");
        assert_eq!(p.clock(t0 + Duration::from_secs(61)), "23:59");

        // Paused, the clock stands still.
        app.pomodoro_toggle();
        assert!(app.pomodoro.as_ref().unwrap().paused());
        app.pomodoro_toggle();

        // Time's up: a break, with the round counted.
        let later = Instant::now() + FOCUS + Duration::from_secs(1);
        assert!(app.pomodoro_tick(later));
        let p = app.pomodoro.clone().unwrap();
        assert_eq!((p.phase, p.rounds), (Phase::Break, 1));
        assert!(app.pomodoro_tick(later + BREAK + Duration::from_secs(1)));
        assert!(app.pomodoro.is_none());
    }
}
