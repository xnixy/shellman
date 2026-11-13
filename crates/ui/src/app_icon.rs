//! Shellman's own apps wear the user's accent. Their exe icons are fixed at build time, so wherever
//! shellman shows one of its own apps it draws the icon itself (`crates/icons`) in the current
//! accent, and falls back to the shell's image for every other program.
//!
//! The textures are cached per icon and size and thrown away when the accent changes, so a
//! frame never draws anything; the redraw (about 10 ms) happens once per accent.

pub use shellman_icons::Kind;

use crate::theme;
use egui::{ColorImage, Context, Id, TextureHandle, TextureId, TextureOptions, Vec2};
use shellman_core::thumbs::Thumbs;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

/// Pixel sizes of a row icon and a tile icon, like the shell's small and large images.
const SMALL: u32 = 32;
const LARGE: u32 = 96;
/// The size handed to the window as its title bar / taskbar icon.
const WINDOW: u32 = 64;

/// Which of shellman's icons an exe gets, or `None` for any program that is not shellman's:
/// `shellman-files.exe` is Files, `shellman-settings.exe` is Settings, any other `shellman-*.exe`
/// is the common shellman icon.
pub fn kind_for_exe(path: &Path) -> Option<Kind> {
    let name = path.file_name()?.to_str()?.to_ascii_lowercase();
    match name.strip_prefix("shellman-")?.strip_suffix(".exe")? {
        "" => None,
        "files" => Some(Kind::Files),
        "settings" => Some(Kind::Settings),
        _ => Some(Kind::Shellman),
    }
}

/// The accent now in use, as the icons want it.
fn current_accent() -> [u8; 3] {
    let c = theme::colors().accent;
    [c.r(), c.g(), c.b()]
}

#[derive(Default)]
struct Cache {
    accent: [u8; 3],
    textures: HashMap<(Kind, u32), TextureHandle>,
}

type Shared = Arc<Mutex<Cache>>;

fn cache(ctx: &Context) -> Shared {
    ctx.data_mut(|d| d.get_temp_mut_or_insert_with::<Shared>(Id::new("shellman_app_icons"), Default::default).clone())
}

/// The icon of `kind` in the current accent, as a texture and its size in pixels.
pub fn kind_texture(ctx: &Context, kind: Kind, large: bool) -> (TextureId, Vec2) {
    texture(ctx, kind, if large { LARGE } else { SMALL })
}

/// The icon of `kind` in the current accent, drawn with at least `px` pixels a side (up to 256),
/// for an icon shown bigger than a tile: a 56 pt dock icon at 300% is 168 px. Sizes go in steps
/// of 32, so a few are cached and each is drawn once per accent.
pub fn kind_texture_px(ctx: &Context, kind: Kind, px: f32) -> (TextureId, Vec2) {
    texture(ctx, kind, step_px(px))
}

fn step_px(px: f32) -> u32 {
    let px = px.ceil().max(0.0) as u32;
    if px <= LARGE {
        LARGE
    } else {
        px.div_ceil(32).saturating_mul(32).min(256)
    }
}

fn texture(ctx: &Context, kind: Kind, size: u32) -> (TextureId, Vec2) {
    let accent = current_accent();
    let shared = cache(ctx);
    let mut cache = shared.lock().unwrap();
    if cache.accent != accent {
        cache.textures.clear();
        cache.accent = accent;
    }
    let tex = cache.textures.entry((kind, size)).or_insert_with(|| {
        let rgba = shellman_icons::render(kind, size, accent);
        let image = ColorImage::from_rgba_unmultiplied([size as usize; 2], &rgba);
        // Mipmapped: the same texture is drawn at several sizes (a tile, a smaller dock icon).
        let options = TextureOptions { mipmap_mode: Some(egui::TextureFilter::Linear), ..TextureOptions::LINEAR };
        ctx.load_texture(format!("shellman-icon-{kind:?}-{size}"), image, options)
    });
    (tex.id(), tex.size_vec2())
}

/// What to draw for the app at `path`: for one of shellman's exes, the icon pack's tile for it
/// when the pack has one (Islands does), else its own icon in the current accent; for any other
/// program the shell's image or the pack's (None until that has loaded).
pub fn get(ctx: &Context, path: &Path, large: bool, thumbs: &mut Thumbs) -> Option<(TextureId, Vec2)> {
    match kind_for_exe(path) {
        Some(kind) => match own_from_pack(path) {
            Some(image) => thumbs.get(&image, large).map(|t| (t.id(), t.size_vec2())),
            None => Some(kind_texture(ctx, kind, large)),
        },
        None => thumbs.get(path, large).map(|t| (t.id(), t.size_vec2())),
    }
}

/// The icon pack's image for one of shellman's own exes (`shellman-files.exe` is the pack's
/// `shellman-files`), when the chosen pack has one.
pub fn own_from_pack(path: &Path) -> Option<std::path::PathBuf> {
    kind_for_exe(path)?;
    shellman_core::icon_pack::resolve_path(path, None)
}

/// `get` for shellman's own app known only by its exe's name (`shellman-launcher.exe`), at `px`
/// pixels: the pack's tile, or the accent icon of `kind`.
pub fn own_texture_px(ctx: &Context, exe: &str, kind: Kind, px: f32, thumbs: &mut Thumbs) -> Option<(TextureId, Vec2)> {
    match own_from_pack(Path::new(exe)) {
        Some(image) => thumbs.get(&image, true).map(|t| (t.id(), t.size_vec2())),
        None => Some(kind_texture_px(ctx, kind, px)),
    }
}

/// Keeps the window's own icon (title bar, Alt+Tab, taskbar) in the current accent. Cheap enough
/// to call every frame: it only sends the new icon on the first call and after the accent changes.
pub fn sync_window_icon(ctx: &Context, kind: Kind) {
    let accent = current_accent();
    let id = Id::new(("shellman_window_icon", kind as u8));
    if ctx.data(|d| d.get_temp::<[u8; 3]>(id)) == Some(accent) {
        return;
    }
    ctx.data_mut(|d| d.insert_temp(id, accent));
    let rgba = shellman_icons::render(kind, WINDOW, accent);
    let icon = egui::IconData { rgba, width: WINDOW, height: WINDOW };
    ctx.send_viewport_cmd(egui::ViewportCommand::Icon(Some(Arc::new(icon))));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(name: &str) -> Option<Kind> {
        kind_for_exe(Path::new(name))
    }

    #[test]
    fn shellman_exes_map_to_their_icons() {
        assert_eq!(kind(r"C:\x\target\debug\shellman-files.exe"), Some(Kind::Files));
        assert_eq!(kind("shellman-settings.exe"), Some(Kind::Settings));
        assert_eq!(kind("SHELLMAN-Settings.EXE"), Some(Kind::Settings));
        for other in ["shellman-bar.exe", "shellman-launcher.exe", "shellman-switcher.exe", "shellman-desktop.exe"] {
            assert_eq!(kind(other), Some(Kind::Shellman), "{other}");
        }
    }

    #[test]
    fn other_programs_have_no_shellman_icon() {
        for other in ["notepad.exe", r"C:\Windows\explorer.exe", "shellman-files.lnk", "shellman-.exe", "shellman.exe", "my-shellman-files.exe", ""] {
            assert_eq!(kind(other), None, "{other}");
        }
    }

    #[test]
    fn another_accent_gives_other_pixels() {
        let a = shellman_icons::render(Kind::Files, SMALL, [196, 54, 78]);
        let b = shellman_icons::render(Kind::Files, SMALL, [0, 103, 192]);
        assert_ne!(a, b);
    }

    #[test]
    fn big_icons_are_drawn_in_steps() {
        let steps = [10.0, 96.0, 97.0, 168.0, 900.0].map(step_px);
        assert_eq!(steps, [96, 96, 128, 192, 256]);
    }

    #[test]
    fn the_cache_follows_the_accent() {
        let ctx = Context::default();
        let first = kind_texture(&ctx, Kind::Files, false);
        assert_eq!(first, kind_texture(&ctx, Kind::Files, false), "a repeat is the same texture");
        theme::set_accent(&ctx, egui::Color32::from_rgb(0, 103, 192));
        assert_ne!(first.0, kind_texture(&ctx, Kind::Files, false).0, "a new accent is a new texture");
    }
}
