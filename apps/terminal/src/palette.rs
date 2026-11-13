//! The terminal's colours: the 16 ANSI colours for dark and light, the 256-colour cube and
//! greys, and the default text, which follows shellman's theme. The default background is no
//! colour at all: the island's glass.

pub type Rgb = [u8; 3];

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Palette {
    pub ansi: [Rgb; 16],
    pub fg: Rgb,
    /// What the default background stands for when text is drawn inverse on it.
    pub bg: Rgb,
}

/// Windows Terminal's Campbell, the colours Windows' console programs were made for.
const DARK: [Rgb; 16] = [
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

/// For a light ground: Campbell's hues, darker where they would wash out on white (yellow,
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
        Self { ansi: if light { LIGHT } else { DARK }, fg, bg }
    }

    /// One of the 256 indexed colours.
    pub fn indexed(&self, i: u8) -> Rgb {
        match i {
            0..=15 => self.ansi[i as usize],
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cube_and_greys() {
        let p = Palette::new(false, [255; 3], [0; 3]);
        assert_eq!(p.indexed(1), DARK[1]);
        assert_eq!(p.indexed(16), [0, 0, 0]);
        assert_eq!(p.indexed(196), [255, 0, 0]);
        assert_eq!(p.indexed(231), [255, 255, 255]);
        assert_eq!(p.indexed(232), [8, 8, 8]);
        assert_eq!(p.indexed(255), [238, 238, 238]);
    }
}
