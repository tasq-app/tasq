//! Toasts: short messages that slide in at the top right, stay a moment and
//! slide out again — "✓ Done · Ejercicio AII". They replace the message
//! that used to sit in the status bar.

use std::time::{Duration, Instant};

/// How long a toast takes to slide in, and to slide out.
pub const SLIDE: Duration = Duration::from_millis(180);
/// How long a toast stays fully in view.
pub const STAY: Duration = Duration::from_millis(2400);
/// At most this many toasts show at once; older ones leave early.
const MAX: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    /// Something got done: completed, added, saved.
    Done,
    Info,
    /// Something went wrong.
    Error,
}

impl ToastKind {
    /// A guess from the wording of a plain message.
    pub fn of(msg: &str) -> Self {
        let m = msg.to_lowercase();
        if ["failed", "invalid", "error", "can't", "cannot", "not found"]
            .iter()
            .any(|w| m.contains(w))
        {
            ToastKind::Error
        } else if [
            "completed",
            "added",
            "saved",
            "created",
            "removed",
            "restored",
            "copied",
        ]
        .iter()
        .any(|w| m.starts_with(w))
        {
            ToastKind::Done
        } else {
            ToastKind::Info
        }
    }
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub kind: ToastKind,
    pub title: String,
    pub detail: Option<String>,
    born: Instant,
}

impl Toast {
    fn life(&self) -> Duration {
        SLIDE + STAY + SLIDE
    }

    /// How far out of view it is, from 0.0 (fully in) to 1.0 (fully out),
    /// or `None` once it's gone.
    pub fn offset(&self, now: Instant) -> Option<f32> {
        let t = now.saturating_duration_since(self.born);
        if t >= self.life() {
            return None;
        }
        let ease = |x: f32| 1.0 - (1.0 - x).powi(3);
        Some(if t < SLIDE {
            1.0 - ease(t.as_secs_f32() / SLIDE.as_secs_f32())
        } else if t < SLIDE + STAY {
            0.0
        } else {
            let out = (t - SLIDE - STAY).as_secs_f32() / SLIDE.as_secs_f32();
            out * out
        })
    }

    fn moving(&self, now: Instant) -> bool {
        let t = now.saturating_duration_since(self.born);
        t < SLIDE || (t >= SLIDE + STAY && t < self.life())
    }
}

#[derive(Debug, Default, Clone)]
pub struct Toasts {
    items: Vec<Toast>,
}

impl Toasts {
    /// Show a toast. The same text again while it's in view just keeps it
    /// there a little longer instead of stacking a copy.
    pub fn push(&mut self, kind: ToastKind, title: String, detail: Option<String>, now: Instant) {
        if let Some(last) = self.items.last_mut()
            && last.title == title
            && last.detail == detail
            && last.offset(now).is_some()
        {
            last.born = now.checked_sub(SLIDE).unwrap_or(now);
            return;
        }
        // A plain message replaces the previous plain one in place: a
        // picker stepping through spaces shouldn't stack a toast a step.
        if kind == ToastKind::Info
            && let Some(last) = self.items.last_mut()
            && last.kind == ToastKind::Info
            && last.offset(now).is_some_and(|o| o == 0.0)
        {
            last.title = title;
            last.detail = detail;
            last.born = now.checked_sub(SLIDE).unwrap_or(now);
            return;
        }
        self.items.push(Toast {
            kind,
            title,
            detail,
            born: now,
        });
        if self.items.len() > MAX {
            self.items.remove(0);
        }
    }

    /// The toasts in view, newest first, with how far out each one is.
    pub fn visible(&self, now: Instant) -> Vec<(&Toast, f32)> {
        self.items
            .iter()
            .rev()
            .filter_map(|t| Some((t, t.offset(now)?)))
            .collect()
    }

    /// Drop the ones that have left. Returns whether any did.
    pub fn sweep(&mut self, now: Instant) -> bool {
        let before = self.items.len();
        self.items.retain(|t| t.offset(now).is_some());
        self.items.len() != before
    }

    /// When the screen next needs drawing for the toasts: soon while one
    /// is sliding, else when the next one starts leaving.
    pub fn next_wake(&self, now: Instant) -> Option<Duration> {
        if self.items.iter().any(|t| t.moving(now)) {
            return Some(Duration::from_millis(16));
        }
        self.items
            .iter()
            .map(|t| (t.born + SLIDE + STAY).saturating_duration_since(now))
            .min()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_toast_slides_in_stays_and_slides_out() {
        let t0 = Instant::now();
        let mut ts = Toasts::default();
        ts.push(ToastKind::Done, "Done".into(), Some("Gym".into()), t0);
        let off = |t: Duration| ts.visible(t0 + t).first().map(|(_, o)| *o);
        assert_eq!(off(Duration::ZERO), Some(1.0));
        assert_eq!(off(SLIDE + Duration::from_millis(10)), Some(0.0));
        assert!(off(SLIDE + STAY + SLIDE / 2).is_some_and(|o| o > 0.0 && o < 1.0));
        assert_eq!(off(SLIDE + STAY + SLIDE), None);
        assert_eq!(ts.next_wake(t0), Some(Duration::from_millis(16)));
        assert!(ts.sweep(t0 + SLIDE * 3 + STAY));
        assert!(ts.is_empty());
    }

    #[test]
    fn plain_messages_replace_each_other_and_kinds_are_guessed() {
        let t0 = Instant::now();
        let later = t0 + SLIDE * 2;
        let mut ts = Toasts::default();
        ts.push(ToastKind::Info, "Uni (1/3)".into(), None, t0);
        ts.push(ToastKind::Info, "Work (2/3)".into(), None, later);
        let v = ts.visible(later);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].0.title, "Work (2/3)");
        assert_eq!(ToastKind::of("completed +next"), ToastKind::Done);
        assert_eq!(ToastKind::of("rename failed: x"), ToastKind::Error);
        assert_eq!(ToastKind::of("Uni hidden from the views"), ToastKind::Info);
    }
}
