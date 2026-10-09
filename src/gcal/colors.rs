//! Google Calendar's eleven event colours, and the one closest to a space's
//! colour. An event takes its own colour (`colorId`), so one "tasq"
//! calendar can show every space in its colour — as near as eleven allow;
//! spaces that land on the same one share it.

/// Google's event colours: `colorId` and how Google draws it.
pub const GOOGLE_COLORS: [(&str, &str, (u8, u8, u8)); 11] = [
    ("1", "Lavender", (0x79, 0x86, 0xcb)),
    ("2", "Sage", (0x33, 0xb6, 0x79)),
    ("3", "Grape", (0x8e, 0x24, 0xaa)),
    ("4", "Flamingo", (0xe6, 0x7c, 0x73)),
    ("5", "Banana", (0xf6, 0xbf, 0x26)),
    ("6", "Tangerine", (0xf4, 0x51, 0x1e)),
    ("7", "Peacock", (0x03, 0x9b, 0xe5)),
    ("8", "Graphite", (0x61, 0x61, 0x61)),
    ("9", "Blueberry", (0x3f, 0x51, 0xb5)),
    ("10", "Basil", (0x0b, 0x80, 0x43)),
    ("11", "Tomato", (0xd5, 0x00, 0x00)),
];

/// Hue (0–360), saturation and lightness (0–1) of an RGB colour.
fn hsl((r, g, b): (u8, u8, u8)) -> (f32, f32, f32) {
    let (r, g, b) = (
        f32::from(r) / 255.0,
        f32::from(g) / 255.0,
        f32::from(b) / 255.0,
    );
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;
    if d == 0.0 {
        return (0.0, 0.0, l);
    }
    let s = d / (1.0 - (2.0 * l - 1.0).abs()).max(f32::EPSILON);
    let h = if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    (h, s, l)
}

/// The Google `colorId` closest to `rgb`. Hue decides (theme colours are
/// often pale; Google's are vivid), lightness breaks near ties; a grey goes
/// to Graphite.
pub fn nearest(rgb: (u8, u8, u8)) -> &'static str {
    let (h, s, l) = hsl(rgb);
    if s < 0.12 {
        return "8";
    }
    let hue_gap = |a: f32, b: f32| {
        let d = (a - b).abs() % 360.0;
        d.min(360.0 - d)
    };
    GOOGLE_COLORS
        .iter()
        .filter(|(id, _, _)| *id != "8")
        .min_by(|a, b| {
            let score = |c: &(u8, u8, u8)| {
                let (ch, _, cl) = hsl(*c);
                hue_gap(h, ch) + (l - cl).abs() * 40.0
            };
            score(&a.2).total_cmp(&score(&b.2))
        })
        .map_or("8", |(id, _, _)| id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_space_colour_finds_its_google_colour() {
        // Catppuccin's pastels land on the colour you'd pick by eye.
        assert_eq!(nearest((0x8a, 0xad, 0xf4)), "1", "blue: lavender");
        assert_eq!(nearest((0xa6, 0xda, 0x95)), "2", "green");
        assert_eq!(nearest((0xf5, 0xa9, 0x7f)), "6", "peach");
        assert_eq!(nearest((0xee, 0xd4, 0x9f)), "5", "yellow");
        assert_eq!(nearest((0xc6, 0xa0, 0xf6)), "3", "mauve");
        assert_eq!(nearest((0x91, 0xd7, 0xe3)), "7", "sky");
        assert_eq!(nearest((0x80, 0x80, 0x80)), "8", "grey");
    }
}
