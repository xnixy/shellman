//! The colours a frame gets, and which border a window wears. Pure.

use super::settings::FramesSettings;

/// `DWMWA_COLOR_DEFAULT`: give the colour back to Windows.
pub const COLOR_DEFAULT: u32 = 0xFFFF_FFFF;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rgb(pub u8, pub u8, pub u8);

/// DWM wants a COLORREF: 0x00BBGGRR.
pub fn colorref(c: Rgb) -> u32 {
    (c.2 as u32) << 16 | (c.1 as u32) << 8 | c.0 as u32
}

/// What the frames are painted with, from the theme (the host fills it in).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Palette {
    pub dark: bool,
    /// The title bar: the islands' glass tone.
    pub caption: Rgb,
    pub text: Rgb,
    /// The border of the window in front.
    pub accent: Rgb,
    /// The border of every other window: the theme's hairline.
    pub dim: Rgb,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Border {
    Accent,
    Dim,
    /// Windows' own.
    Default,
}

/// Which border a window gets: accent when it is in front and the setting allows, else dim.
pub fn border_for(is_front: bool, settings: &FramesSettings) -> Border {
    if is_front && settings.accent_border {
        Border::Accent
    } else {
        Border::Dim
    }
}

impl Border {
    pub fn colorref(self, p: &Palette) -> u32 {
        match self {
            Border::Accent => colorref(p.accent),
            Border::Dim => colorref(p.dim),
            Border::Default => COLOR_DEFAULT,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colorref_is_blue_green_red() {
        assert_eq!(colorref(Rgb(0x16, 0x1A, 0x26)), 0x0026_1A16);
        assert_eq!(colorref(Rgb(0xFF, 0, 0)), 0x0000_00FF);
    }

    #[test]
    fn only_the_front_window_gets_the_accent() {
        let on = FramesSettings { accent_border: true, ..FramesSettings::default() };
        assert_eq!(border_for(true, &on), Border::Accent);
        assert_eq!(border_for(false, &on), Border::Dim);
        let off = FramesSettings { accent_border: false, ..on };
        assert_eq!(border_for(true, &off), Border::Dim);
    }

    #[test]
    fn border_colours() {
        let p = Palette { dark: true, caption: Rgb(0, 0, 0), text: Rgb(0, 0, 0), accent: Rgb(1, 2, 3), dim: Rgb(4, 5, 6) };
        assert_eq!(Border::Accent.colorref(&p), 0x0003_0201);
        assert_eq!(Border::Dim.colorref(&p), 0x0006_0504);
        assert_eq!(Border::Default.colorref(&p), COLOR_DEFAULT);
    }
}
