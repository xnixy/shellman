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

/// A set of 16 colours the user can pick (Settings › Terminal, `color_scheme`). shellman's own is
/// tuned by hand for the glass; the others are Windows Terminal's, kept as they are for
/// backgrounds and lifted (on dark glass) or deepened (on light) where text in them would not
/// read on the glass, so no scheme hides text.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Scheme {
    #[default]
    Shellman,
    Campbell,
    OneHalf,
    Tango,
}

impl Scheme {
    pub const ALL: [Scheme; 4] = [Scheme::Shellman, Scheme::Campbell, Scheme::OneHalf, Scheme::Tango];

    /// The name `terminal.json` keeps; unknown names are shellman's.
    pub fn from_name(name: &str) -> Self {
        Self::ALL.into_iter().find(|s| s.name().eq_ignore_ascii_case(name.trim())).unwrap_or_default()
    }

    pub fn name(self) -> &'static str {
        match self {
            Scheme::Shellman => "shellman",
            Scheme::Campbell => "campbell",
            Scheme::OneHalf => "one-half",
            Scheme::Tango => "tango",
        }
    }

    /// Its 16 as made (for a dark background).
    fn colours(self) -> [Rgb; 16] {
        match self {
            Scheme::Shellman | Scheme::Campbell => CAMPBELL,
            Scheme::OneHalf => ONE_HALF,
            Scheme::Tango => TANGO,
        }
    }
}

/// Windows Terminal's One Half Dark.
const ONE_HALF: [Rgb; 16] = [
    [40, 44, 52],
    [224, 108, 117],
    [152, 195, 121],
    [229, 192, 123],
    [97, 175, 239],
    [198, 120, 221],
    [86, 182, 194],
    [220, 223, 228],
    [90, 99, 116],
    [224, 108, 117],
    [152, 195, 121],
    [229, 192, 123],
    [97, 175, 239],
    [198, 120, 221],
    [86, 182, 194],
    [220, 223, 228],
];

/// Windows Terminal's Tango Dark.
const TANGO: [Rgb; 16] = [
    [0, 0, 0],
    [204, 0, 0],
    [78, 154, 6],
    [196, 160, 0],
    [52, 101, 164],
    [117, 80, 123],
    [6, 152, 154],
    [211, 215, 207],
    [85, 87, 83],
    [239, 41, 41],
    [138, 226, 52],
    [252, 233, 79],
    [114, 159, 207],
    [173, 127, 168],
    [52, 226, 226],
    [238, 238, 236],
];

/// The glass as it shows behind the grid (shellman's dark and light grounds at the default
/// glass amount, measured off screenshots).
pub const DARK_GLASS: Rgb = [30, 31, 36];
pub const LIGHT_GLASS: Rgb = [238, 239, 243];

/// The contrast text must have on the glass (WCAG's for large text and UI).
const READABLE: f32 = 3.0;

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

    /// The palette of `scheme` for the theme: its colours for backgrounds, and for text on the
    /// glass each moved towards white (dark) or black (light) just enough to read there. Black
    /// on the dark glass stays black (it is meant for light backgrounds).
    pub fn scheme(scheme: Scheme, light: bool, fg: Rgb, bg: Rgb) -> Self {
        if scheme == Scheme::Shellman {
            return Self::new(light, fg, bg);
        }
        let raw = scheme.colours();
        let glass = if light { LIGHT_GLASS } else { DARK_GLASS };
        let mut ansi = raw;
        for (i, c) in ansi.iter_mut().enumerate() {
            if !light && i == 0 {
                continue;
            }
            *c = readable(*c, glass, light);
        }
        Self { ansi, ground: raw, fg, bg }
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

/// `c`, or the least step of it towards black (`darken`) or white that reads on `glass`.
fn readable(c: Rgb, glass: Rgb, darken: bool) -> Rgb {
    let to = if darken { 0.0 } else { 255.0 };
    for step in 0..=20 {
        let t = step as f32 / 20.0;
        let m = c.map(|v| (v as f32 + (to - v as f32) * t).round() as u8);
        if contrast(m, glass) >= READABLE + 0.05 {
            return m;
        }
    }
    c
}

/// WCAG's contrast ratio between two colours (1 to 21).
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
    fn every_scheme_reads_on_both_glasses_and_keeps_its_backgrounds() {
        for scheme in Scheme::ALL {
            for light in [false, true] {
                let p = Palette::scheme(scheme, light, [0; 3], [0; 3]);
                let glass = if light { LIGHT_GLASS } else { DARK_GLASS };
                for (i, c) in p.ansi.iter().enumerate().skip(if light { 0 } else { 1 }) {
                    let r = contrast(*c, glass);
                    assert!(r >= 3.0, "{scheme:?} light={light} colour {i} {c:?} is {r:.2}:1");
                }
                if scheme != Scheme::Shellman {
                    assert_eq!(p.ground, scheme.colours());
                }
            }
        }
        // Colours that already read are left alone: One Half's green on the dark glass.
        assert_eq!(Palette::scheme(Scheme::OneHalf, false, [0; 3], [0; 3]).ansi[2], ONE_HALF[2]);
    }

    #[test]
    fn scheme_names() {
        for s in Scheme::ALL {
            assert_eq!(Scheme::from_name(s.name()), s);
        }
        assert_eq!(Scheme::from_name(" One-Half "), Scheme::OneHalf);
        assert_eq!(Scheme::from_name("solarized"), Scheme::Shellman);
        assert_eq!(Scheme::from_name(""), Scheme::Shellman);
        // Settings offers exactly these.
        let offered: Vec<&str> = shellman_core::terminal::file::COLOR_SCHEMES.iter().map(|(n, _)| *n).collect();
        let ours: Vec<&str> = Scheme::ALL.iter().map(|s| s.name()).collect();
        assert_eq!(offered, ours);
    }

    #[test]
    fn contrast_is_wcag_s() {
        assert!((contrast([0; 3], [255; 3]) - 21.0).abs() < 0.01);
        assert!((contrast([128; 3], [128; 3]) - 1.0).abs() < 0.001);
    }
}
