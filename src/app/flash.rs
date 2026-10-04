use std::time::Instant;

use super::App;
use super::types::FLASH_TTL;

#[derive(Debug, Default, Clone)]
pub struct Flash {
    current: Option<(String, Instant)>,
}

impl Flash {
    pub fn set(&mut self, msg: impl Into<String>) {
        self.current = Some((msg.into(), Instant::now()));
    }

    pub fn clear(&mut self) {
        self.current = None;
    }

    pub fn active(&self) -> Option<&str> {
        self.current
            .as_ref()
            .filter(|(_, t)| t.elapsed() < FLASH_TTL)
            .map(|(m, _)| m.as_str())
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.current.as_ref().map(|(_, t)| *t + FLASH_TTL)
    }

    pub fn should_clear(&self) -> bool {
        self.current
            .as_ref()
            .map(|(_, t)| t.elapsed() >= FLASH_TTL)
            .unwrap_or(false)
    }
}

impl App {
    /// Show a message: a toast at the top right, its kind guessed from the
    /// wording.
    pub fn flash(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        let kind = super::ToastKind::of(&msg);
        self.toasts.push(kind, msg.clone(), None, Instant::now());
        self.flash_state.set(msg);
    }

    /// A toast with a title and a detail, e.g. "Done · Gym"; `flash` is the
    /// plain message kept for anything that reads it back.
    pub fn toast(
        &mut self,
        kind: super::ToastKind,
        title: impl Into<String>,
        detail: Option<String>,
        flash: impl Into<String>,
    ) {
        self.toasts.push(kind, title.into(), detail, Instant::now());
        self.flash_state.set(flash);
    }

    pub fn clear_flash(&mut self) {
        self.flash_state.clear();
    }

    pub fn flash_active(&self) -> Option<&str> {
        self.flash_state.active()
    }

    pub fn flash_deadline(&self) -> Option<Instant> {
        self.flash_state.deadline()
    }

    pub fn flash_should_clear(&self) -> bool {
        self.flash_state.should_clear()
    }
}
