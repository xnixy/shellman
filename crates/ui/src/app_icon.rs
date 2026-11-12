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
    let accent = current_accent();
    let size = if large { LARGE } else { SMALL };
    let shared = cache(ctx);
    let mut cache = shared.lock().unwrap();
    if cache.accent != accent {
        cache.textures.clear();
        cache.accent = accent;
    }
    let tex = cache.textures.entry((kind, size)).or_insert_with(|| {
        let rgba = shellman_icons::render(kind, size, accent);
        let image = ColorImage::from_rgba_unmultiplied([size as usize; 2], &rgba);
        ctx.load_texture(format!("shellman-icon-{kind:?}-{size}"), image, TextureOptions::LINEAR)
    });
    (tex.id(), tex.size_vec2())
}

/// What to draw for the app at `path`: shellman's own icon in the current accent when it is one
/// of shellman's exes, otherwise the shell's image (None until that has loaded).
pub fn get(ctx: &Context, path: &Path, large: bool, thumbs: &mut Thumbs) -> Option<(TextureId, Vec2)> {
    match kind_for_exe(path) {
        Some(kind) => Some(kind_texture(ctx, kind, large)),
        None => thumbs.get(path, large).map(|t| (t.id(), t.size_vec2())),
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
    fn the_cache_follows_the_accent() {
        let ctx = Context::default();
        let first = kind_texture(&ctx, Kind::Files, false);
        assert_eq!(first, kind_texture(&ctx, Kind::Files, false), "a repeat is the same texture");
        theme::set_accent(&ctx, egui::Color32::from_rgb(0, 103, 192));
        assert_ne!(first.0, kind_texture(&ctx, Kind::Files, false).0, "a new accent is a new texture");
    }
}
