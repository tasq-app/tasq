//! The colour picker for a space (`c` on it): the palette as a small grid
//! to walk with the arrows or click, `c` again for a hex of your own.

use super::App;
use crate::core::spaces::{PALETTE_SLOTS, SpaceColor};

/// Swatches per row of the grid.
pub const PICK_COLS: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorPick {
    /// The space being painted.
    pub path: String,
    /// The swatch under the cursor.
    pub cursor: usize,
    /// Typing a custom colour: the hex so far, without `#`.
    pub hex: Option<String>,
}

impl App {
    /// `c` on a space: the picker, on its current colour.
    pub fn open_color_pick(&mut self) {
        let Some(path) = self.filter.project.clone() else {
            return;
        };
        let cursor = match self.store.space_color(&path) {
            SpaceColor::Slot(n) => n,
            SpaceColor::Rgb(..) => 0,
        };
        self.color_pick = Some(ColorPick {
            path,
            cursor,
            hex: None,
        });
    }

    /// Move the cursor by columns and rows, wrapping.
    pub fn color_pick_move(&mut self, dx: i32, dy: i32) {
        let Some(p) = self.color_pick.as_mut() else {
            return;
        };
        let n = PALETTE_SLOTS as i32;
        let cols = PICK_COLS as i32;
        let i = p.cursor as i32 + dx + dy * cols;
        p.cursor = i.rem_euclid(n) as usize;
    }

    /// `c` inside the picker: type a hex instead.
    pub fn color_pick_begin_hex(&mut self) {
        if let Some(p) = self.color_pick.as_mut() {
            p.hex = Some(String::new());
        }
    }

    /// A key while typing the hex: kept when it's a hex digit.
    pub fn color_pick_type(&mut self, c: char) {
        if let Some(hex) = self.color_pick.as_mut().and_then(|p| p.hex.as_mut())
            && c.is_ascii_hexdigit()
            && hex.len() < 6
        {
            hex.push(c.to_ascii_lowercase());
        }
    }

    pub fn color_pick_backspace(&mut self) {
        if let Some(hex) = self.color_pick.as_mut().and_then(|p| p.hex.as_mut()) {
            hex.pop();
        }
    }

    /// The colour being typed, once it's whole.
    pub fn color_pick_typed(&self) -> Option<SpaceColor> {
        let hex = self.color_pick.as_ref()?.hex.as_ref()?;
        SpaceColor::parse(&format!("#{hex}"))
    }

    /// Enter: the swatch (or the typed hex) becomes the space's colour.
    pub fn color_pick_accept(&mut self) {
        let Some(p) = self.color_pick.clone() else {
            return;
        };
        let color = if p.hex.is_some() {
            match self.color_pick_typed() {
                Some(c) => c,
                None => {
                    self.flash("six hex digits, like 8aadf4");
                    return;
                }
            }
        } else {
            SpaceColor::Slot(p.cursor)
        };
        self.color_pick = None;
        self.set_space_color_to(&p.path, Some(color));
    }

    /// `C`: back to the automatic colour.
    pub fn color_pick_auto(&mut self) {
        if let Some(p) = self.color_pick.take() {
            self.set_space_color_to(&p.path, None);
        }
    }

    /// A click on swatch `i`: that colour.
    pub fn color_pick_click(&mut self, i: usize) {
        if let Some(p) = self.color_pick.as_mut() {
            p.cursor = i;
            p.hex = None;
        }
        self.color_pick_accept();
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::app::test_support::build_app;

    #[test]
    fn the_picker_walks_the_grid_and_takes_a_hex() {
        let mut app = build_app("task +Uni\n");
        app.filter.project = Some("Uni".into());
        app.open_color_pick();
        let start = app.color_pick.as_ref().unwrap().cursor;
        app.color_pick_move(0, 1);
        assert_eq!(
            app.color_pick.as_ref().unwrap().cursor,
            (start + PICK_COLS) % PALETTE_SLOTS
        );
        app.color_pick_move(-1, 0);
        app.color_pick_move(1, 0);
        assert_eq!(
            app.color_pick.as_ref().unwrap().cursor,
            (start + PICK_COLS) % PALETTE_SLOTS
        );

        app.color_pick_begin_hex();
        for c in "#8AaDf4zz".chars() {
            app.color_pick_type(c);
        }
        assert_eq!(
            app.color_pick.as_ref().unwrap().hex.as_deref(),
            Some("8aadf4")
        );
        assert_eq!(
            app.color_pick_typed(),
            Some(SpaceColor::Rgb(0x8a, 0xad, 0xf4))
        );
        app.color_pick_backspace();
        assert_eq!(app.color_pick_typed(), None);
    }
}
