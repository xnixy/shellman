//! The terminal's colours: the 16 ANSI colours, the 256-colour cube and greys, and the default
//! text, which follows shellman's theme. The default background is no colour at all: the
//! island's glass.
//!
//! The 16 come in two sets. Backgrounds, and text on a coloured background, use Campbell (Windows
//! Terminal's, which console programs were made for): white on red reads as white on red in
//! either theme. Text on the glass uses a set made for the theme's glass, each colour at least
//! 3:1 against it (WCAG's contrast; tested): for the dark glass Campbell with its dark red, blue
//! and magentas lifted; for the light glass Campbell's hues darkened, with white and black swapped
//! in role so "white" text still shows.

pub type Rgb = [u8; 3];

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Palette {
    /// The 16 for text on the glass.
    pub ansi: [Rgb; 16],
    /// The 16 for backgrounds, and for text on a coloured background.
    pub ground: [Rgb; 16],
    pub fg: Rgb,
    /// What the default background stands for when text is drawn inverse on it.
    pub bg: Rgb,
}

/// Windows Terminal's Campbell.
const CAMPBELL: [Rgb; 16] = [
    [12, 12, 12],
    [197, 15, 31],
    [19, 161, 14],
    [193, 156, 0],
    [0, 55, 218],
    [136, 23, 152],
    [58, 150, 221],
    [204, 204, 204],
    [118, 118, 118],
    [231, 72, 86],
    [22, 198, 12],
    [249, 241, 165],
    [59, 120, 255],
    [180, 0, 158],
    [97, 214, 214],
    [242, 242, 242],
];

/// Text on the dark glass: Campbell, with the colours too dark to read on it (red 2.7:1, blue
/// 2.0, magenta 2.1, bright magenta 2.7) lifted to 3.2 or more, each still darker than its bright
/// twin.
const DARK: [Rgb; 16] = [
    [12, 12, 12],
    [214, 48, 62],
    [19, 161, 14],
    [193, 156, 0],
    [52, 100, 230],
    [166, 72, 190],
    [58, 150, 221],
    [204, 204, 204],
    [118, 118, 118],
    [231, 72, 86],
    [22, 198, 12],
    [249, 241, 165],
    [59, 120, 255],
    [204, 64, 188],
    [97, 214, 214],
    [242, 242, 242],
];

/// Text on the light glass: Campbell's hues, darker where they would wash out on white (yellow,
/// cyan, the bright ones), and white and black swapped in role so "white" text still shows.
const LIGHT: [Rgb; 16] = [
    [12, 12, 12],
    [197, 15, 31],
    [16, 128, 12],
    [152, 112, 0],
    [0, 55, 218],
    [136, 23, 152],
    [14, 116, 170],
    [96, 96, 96],
    [118, 118, 118],
    [200, 40, 56],
    [14, 150, 10],
    [168, 128, 0],
    [36, 96, 230],
    [160, 0, 140],
    [10, 140, 150],
    [60, 60, 60],
];

impl Palette {
    /// The palette for the theme, with `fg` and `bg` taken from shellman's own text and ground.
    pub fn new(light: bool, fg: Rgb, bg: Rgb) -> Self {
        Self { ansi: if light { LIGHT } else { DARK }, ground: CAMPBELL, fg, bg }
    }

    /// One of the 256 indexed colours, as text on the glass.
    pub fn indexed(&self, i: u8) -> Rgb {
        self.indexed_in(i, &self.ansi)
    }

    /// One of the 256 indexed colours as a background (or text on one).
    pub fn indexed_ground(&self, i: u8) -> Rgb {
        self.indexed_in(i, &self.ground)
    }

    fn indexed_in(&self, i: u8, sixteen: &[Rgb; 16]) -> Rgb {
        match i {
            0..=15 => sixteen[i as usize],
            16..=231 => {
                let i = i - 16;
                let level = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
                [level(i / 36), level(i / 6 % 6), level(i % 6)]
            }
            _ => {
                let v = 8 + (i - 232) * 10;
                [v, v, v]
            }
        }
    }
}

/// A colour two thirds as bright (SGR 2, faint).
pub fn dim(c: Rgb) -> Rgb {
    c.map(|v| (v as u16 * 2 / 3) as u8)
}

/// WCAG's contrast ratio between two colours (1 to 21).
#[cfg(test)]
pub fn contrast(a: Rgb, b: Rgb) -> f32 {
    let lin = |v: u8| {
        let c = v as f32 / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let l = |c: Rgb| 0.2126 * lin(c[0]) + 0.7152 * lin(c[1]) + 0.0722 * lin(c[2]);
    let (x, y) = (l(a), l(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The glass as it shows behind the grid (shellman's dark and light grounds at the default
    /// glass amount, measured off screenshots).
    const DARK_GLASS: Rgb = [30, 31, 36];
    const LIGHT_GLASS: Rgb = [238, 239, 243];

    #[test]
    fn cube_and_greys() {
        let p = Palette::new(false, [255; 3], [0; 3]);
        assert_eq!(p.indexed(1), DARK[1]);
        assert_eq!(p.indexed_ground(1), CAMPBELL[1]);
        assert_eq!(p.indexed(16), [0, 0, 0]);
        assert_eq!(p.indexed(196), [255, 0, 0]);
        assert_eq!(p.indexed(231), [255, 255, 255]);
        assert_eq!(p.indexed(232), [8, 8, 8]);
        assert_eq!(p.indexed(255), [238, 238, 238]);
    }

    #[test]
    fn every_colour_reads_on_its_glass() {
        // Dark: all but black (which is meant to sit on a light background).
        for (i, c) in DARK.iter().enumerate().skip(1) {
            let r = contrast(*c, DARK_GLASS);
            assert!(r >= 3.0, "dark colour {i} {c:?} is {r:.2}:1 on the glass");
        }
        for (i, c) in LIGHT.iter().enumerate() {
            let r = contrast(*c, LIGHT_GLASS);
            assert!(r >= 3.0, "light colour {i} {c:?} is {r:.2}:1 on the glass");
        }
    }

    #[test]
    fn white_text_on_colour_is_white_in_both_themes() {
        for light in [false, true] {
            let p = Palette::new(light, [0; 3], [0; 3]);
            // Bright white on red and on blue: Campbell's, readable.
            assert_eq!(p.indexed_ground(15), [242, 242, 242]);
            assert!(contrast(p.indexed_ground(15), p.indexed_ground(1)) >= 4.5);
            assert!(contrast(p.indexed_ground(15), p.indexed_ground(4)) >= 4.5);
            // Black on white and on cyan, as status bars use them.
            assert!(contrast(p.indexed_ground(0), p.indexed_ground(7)) >= 4.5);
            assert!(contrast(p.indexed_ground(0), p.indexed_ground(14)) >= 4.5);
        }
    }

    #[test]
    fn contrast_is_wcag_s() {
        assert!((contrast([0; 3], [255; 3]) - 21.0).abs() < 0.01);
        assert!((contrast([128; 3], [128; 3]) - 1.0).abs() < 0.001);
    }
}
