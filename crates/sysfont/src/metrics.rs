//! Window text: the fonts of title bars, menus, message boxes, tooltips, status bars, and desktop
//! icon labels. These are per-user settings that apply the moment they change, with no
//! administrator rights and no restart.

use crate::settings::{self, read_json, write_json};
use serde::{Deserialize, Serialize};
use windows::Win32::Graphics::Gdi::LOGFONTW;
use windows::Win32::UI::HiDpi::{SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_UNAWARE};
use windows::Win32::UI::WindowsAndMessaging::{
    SystemParametersInfoW, ICONMETRICSW, NONCLIENTMETRICSW, SPIF_SENDCHANGE, SPIF_UPDATEINIFILE,
    SPI_GETICONMETRICS, SPI_GETNONCLIENTMETRICS, SPI_SETICONMETRICS, SPI_SETNONCLIENTMETRICS,
};

/// Sizes in points, as Windows' old Advanced Appearance dialog showed them.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TextSizes {
    pub title: f32,
    pub menu: f32,
    pub message: f32,
    /// Tooltips and status bars.
    pub status: f32,
    pub icon: f32,
}

impl Default for TextSizes {
    fn default() -> Self {
        Self {
            title: 9.0,
            menu: 9.0,
            message: 9.0,
            status: 9.0,
            icon: 9.0,
        }
    }
}

pub const SIZE_MIN: f32 = 7.0;
pub const SIZE_MAX: f32 = 14.0;

impl TextSizes {
    pub fn clamped(self) -> Self {
        let c = |v: f32| (v * 2.0).round().clamp(SIZE_MIN * 2.0, SIZE_MAX * 2.0) / 2.0;
        Self {
            title: c(self.title),
            menu: c(self.menu),
            message: c(self.message),
            status: c(self.status),
            icon: c(self.icon),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Font {
    pub face: String,
    /// LOGFONT height at 96 DPI: negative for a size in pixels of the em.
    pub height: i32,
    pub weight: i32,
    pub italic: bool,
}

impl Font {
    fn from_logfont(font: &LOGFONTW) -> Self {
        let len = font
            .lfFaceName
            .iter()
            .position(|ch| *ch == 0)
            .unwrap_or(font.lfFaceName.len());
        Self {
            face: String::from_utf16_lossy(&font.lfFaceName[..len]),
            height: font.lfHeight,
            weight: font.lfWeight,
            italic: font.lfItalic != 0,
        }
    }

    fn write_to(&self, font: &mut LOGFONTW) {
        let mut name = [0u16; 32];
        for (slot, ch) in name.iter_mut().zip(self.face.encode_utf16().take(31)) {
            *slot = ch;
        }
        font.lfFaceName = name;
        font.lfHeight = self.height;
        font.lfWeight = self.weight;
        font.lfItalic = self.italic as u8;
    }

    pub fn points(&self) -> f32 {
        height_to_points(self.height)
    }
}

pub fn points_to_height(points: f32) -> i32 {
    -((points * 96.0 / 72.0).round() as i32)
}

pub fn height_to_points(height: i32) -> f32 {
    let pixels = height.unsigned_abs() as f32;
    // A positive height counts the cell, which is a little taller than the em.
    let pixels = if height > 0 { pixels * 0.8 } else { pixels };
    (pixels * 72.0 / 96.0 * 2.0).round() / 2.0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowText {
    pub caption: Font,
    pub small_caption: Font,
    pub menu: Font,
    pub status: Font,
    pub message: Font,
    pub icon: Font,
}

impl WindowText {
    pub fn faces(&self) -> Vec<&str> {
        let mut faces: Vec<&str> = [
            &self.caption,
            &self.small_caption,
            &self.menu,
            &self.status,
            &self.message,
            &self.icon,
        ]
        .iter()
        .map(|font| font.face.as_str())
        .collect();
        faces.dedup();
        faces
    }

    pub fn sizes(&self) -> TextSizes {
        TextSizes {
            title: self.caption.points(),
            menu: self.menu.points(),
            message: self.message.points(),
            status: self.status.points(),
            icon: self.icon.points(),
        }
    }
}

/// Read and write at 96 DPI, whatever the display scale: a DPI-unaware thread sees the values
/// Windows stores, and Windows scales them for each display itself.
struct Unaware(windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT);

impl Unaware {
    fn enter() -> Self {
        Self(unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_UNAWARE) })
    }
}

impl Drop for Unaware {
    fn drop(&mut self) {
        unsafe {
            SetThreadDpiAwarenessContext(self.0);
        }
    }
}

fn get_raw() -> Result<(NONCLIENTMETRICSW, ICONMETRICSW), String> {
    let _unaware = Unaware::enter();
    let mut nc = NONCLIENTMETRICSW {
        cbSize: size_of::<NONCLIENTMETRICSW>() as u32,
        ..Default::default()
    };
    let mut icon = ICONMETRICSW {
        cbSize: size_of::<ICONMETRICSW>() as u32,
        ..Default::default()
    };
    unsafe {
        SystemParametersInfoW(
            SPI_GETNONCLIENTMETRICS,
            nc.cbSize,
            Some((&raw mut nc).cast()),
            Default::default(),
        )
        .map_err(|err| format!("Could not read window text: {err}"))?;
        SystemParametersInfoW(
            SPI_GETICONMETRICS,
            icon.cbSize,
            Some((&raw mut icon).cast()),
            Default::default(),
        )
        .map_err(|err| format!("Could not read icon text: {err}"))?;
    }
    Ok((nc, icon))
}

/// During a dry run, window text lives in a file instead of in Windows.
fn dry_path() -> std::path::PathBuf {
    settings::machine_dir().join("dry-window-text.json")
}

pub fn read() -> Result<WindowText, String> {
    if settings::dry_run() {
        if let Some(text) = read_json(&dry_path()) {
            return Ok(text);
        }
    }
    let (nc, icon) = get_raw()?;
    Ok(WindowText {
        caption: Font::from_logfont(&nc.lfCaptionFont),
        small_caption: Font::from_logfont(&nc.lfSmCaptionFont),
        menu: Font::from_logfont(&nc.lfMenuFont),
        status: Font::from_logfont(&nc.lfStatusFont),
        message: Font::from_logfont(&nc.lfMessageFont),
        icon: Font::from_logfont(&icon.lfFont),
    })
}

/// Write window text and tell every open window. Apps redraw with it right away.
pub fn write(text: &WindowText) -> Result<(), String> {
    if settings::dry_run() {
        return write_json(&dry_path(), text);
    }
    // The one place window text reaches Windows. Checked again, so nothing dry ever gets here.
    crate::reg::write_allowed(settings::dry_run())?;
    let (mut nc, mut icon) = get_raw()?;
    text.caption.write_to(&mut nc.lfCaptionFont);
    text.small_caption.write_to(&mut nc.lfSmCaptionFont);
    text.menu.write_to(&mut nc.lfMenuFont);
    text.status.write_to(&mut nc.lfStatusFont);
    text.message.write_to(&mut nc.lfMessageFont);
    text.icon.write_to(&mut icon.lfFont);
    let _unaware = Unaware::enter();
    let flags = SPIF_UPDATEINIFILE | SPIF_SENDCHANGE;
    unsafe {
        SystemParametersInfoW(
            SPI_SETNONCLIENTMETRICS,
            nc.cbSize,
            Some((&raw mut nc).cast()),
            flags,
        )
        .map_err(|err| format!("Could not change window text: {err}"))?;
        SystemParametersInfoW(
            SPI_SETICONMETRICS,
            icon.cbSize,
            Some((&raw mut icon).cast()),
            flags,
        )
        .map_err(|err| format!("Could not change icon text: {err}"))?;
    }
    Ok(())
}

fn backup_path() -> std::path::PathBuf {
    settings::user_dir().join("window-text-backup.json")
}

fn applied_path() -> std::path::PathBuf {
    settings::user_dir().join("window-text.json")
}

/// What was last set (by Fontman or here), kept so it can be set again when a theme change
/// resets it.
pub fn applied() -> Option<WindowText> {
    read_json(&applied_path())
}

pub fn has_backup() -> bool {
    backup_path().exists()
}

/// Windows' window text before Fontman changed it.
pub fn backup() -> Option<WindowText> {
    read_json(&backup_path())
}

/// The window text Fontman would set: `face` at `sizes`, keeping each font's weight.
pub fn planned(face: &str, sizes: TextSizes) -> Result<WindowText, String> {
    let base = backup().map(Ok).unwrap_or_else(read)?;
    let sizes = sizes.clamped();
    let with = |font: &Font, points: f32| Font {
        face: face.to_string(),
        height: points_to_height(points),
        weight: font.weight,
        italic: font.italic,
    };
    Ok(WindowText {
        caption: with(&base.caption, sizes.title),
        small_caption: with(&base.small_caption, sizes.title),
        menu: with(&base.menu, sizes.menu),
        status: with(&base.status, sizes.status),
        message: with(&base.message, sizes.message),
        icon: with(&base.icon, sizes.icon),
    })
}

/// Use `face` for window text. The first time, Windows' own settings are saved to go back to.
pub fn apply(face: &str, sizes: TextSizes) -> Result<(), String> {
    if !has_backup() {
        write_json(&backup_path(), &read()?)?;
    }
    let text = planned(face, sizes)?;
    write(&text)?;
    write_json(&applied_path(), &text)
}

/// Put Windows' own window text back.
pub fn restore() -> Result<(), String> {
    if let Some(original) = backup() {
        write(&original)?;
    }
    let _ = std::fs::remove_file(applied_path());
    let _ = std::fs::remove_file(backup_path());
    Ok(())
}

/// A theme change resets window text. Set Fontman's again if it is gone. True if it was.
pub fn reapply_if_reset() -> Result<bool, String> {
    let Some(wanted) = applied() else {
        return Ok(false);
    };
    let now = read()?;
    if now.caption.face == wanted.caption.face && now.message.face == wanted.message.face {
        return Ok(false);
    }
    write(&wanted)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn points_and_heights_convert_both_ways() {
        assert_eq!(points_to_height(9.0), -12);
        assert_eq!(points_to_height(10.5), -14);
        assert_eq!(height_to_points(-12), 9.0);
        assert_eq!(height_to_points(-16), 12.0);
    }

    /// Writes Windows' window text back unchanged. Ignored by default because it broadcasts a
    /// settings change to every window: `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn writing_back_what_was_read_changes_nothing() {
        let before = read().unwrap();
        write(&before).unwrap();
        assert_eq!(read().unwrap(), before);
    }

    #[test]
    fn a_dry_run_keeps_window_text_in_its_stand_in() {
        // Tests are dry: this lands in the stand-in file, never in Windows.
        let font = |face: &str| Font { face: face.into(), height: -12, weight: 400, italic: false };
        let text = WindowText {
            caption: font("Stand-in"),
            small_caption: font("Stand-in"),
            menu: font("Stand-in"),
            status: font("Stand-in"),
            message: font("Stand-in"),
            icon: font("Stand-in"),
        };
        write(&text).unwrap();
        assert!(dry_path().starts_with(std::env::temp_dir()));
        assert_eq!(read().unwrap(), text);
        let _ = std::fs::remove_file(dry_path());
    }

    #[test]
    fn sizes_are_clamped_to_half_points() {
        let sizes = TextSizes {
            title: 30.0,
            menu: 2.0,
            message: 9.3,
            ..TextSizes::default()
        }
        .clamped();
        assert_eq!(sizes.title, SIZE_MAX);
        assert_eq!(sizes.menu, SIZE_MIN);
        assert_eq!(sizes.message, 9.5);
    }
}
