//! Islands, the icon pack built into shellman and its default: the dock tiles of the design
//! mockups. Its SVGs are compiled into every app (`build.rs` reads them from
//! `assets/icon-packs/islands`), so it needs no files beside the exes and nothing written anywhere.
//!
//! Its icons are known by paths that exist nowhere on disk (`PREFIX` + name + `.svg`). They go
//! through the same index, lookups and thumbnail workers as a pack on disk; only the drawing
//! (`raster::svg_file`) asks here for the bytes, and draws the small variant for small sizes.

use super::index::PackIndex;
use std::path::{Path, PathBuf};

include!(concat!(env!("OUT_DIR"), "/islands.rs"));

/// The built-in pack's name, as `icons.json` stores it. No folder under `icon-packs` can take it
/// (see `import`).
pub const NAME: &str = "Islands";

/// What its icons' paths start with: not a path any file can have.
const PREFIX: &str = "shellman-builtin:/islands/";

/// At or under this many pixels the small variant is drawn: a heavier, larger glyph that holds
/// at 16 to 24 px (a list row's icon is asked for at 64 px and shown at 24 points).
pub const SMALL_MAX_PX: u32 = 64;

/// Is this the built-in pack's name?
pub fn is_builtin(pack: &str) -> bool {
    pack.eq_ignore_ascii_case(NAME)
}

/// The path an icon of the pack is known by.
pub fn path(name: &str) -> PathBuf {
    PathBuf::from(format!("{PREFIX}{name}.svg"))
}

/// The SVG for a built-in icon's path, at a size: the small variant up to `SMALL_MAX_PX`.
pub fn bytes(path: &Path, px: u32) -> Option<&'static [u8]> {
    let text = path.to_str()?.replace('\\', "/");
    let name = text.strip_prefix(PREFIX)?.strip_suffix(".svg")?;
    let (_, big, small) = FILES.iter().find(|(n, _, _)| *n == name)?;
    Some(if px <= SMALL_MAX_PX { small } else { big })
}

/// The pack as an index, the way a pack on disk is read.
pub fn index() -> PackIndex {
    PackIndex::from_names(FILES.iter().map(|(n, _, _)| (n.to_string(), path(n))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::icon_pack::names::{candidates, AppRef};
    use crate::icon_pack::raster;

    #[test]
    fn every_icon_parses_and_draws_at_both_sizes() {
        assert!(FILES.len() >= 40, "{} icons", FILES.len());
        for (name, big, small) in FILES {
            for (data, px) in [(big, 256), (small, 32)] {
                let img = raster::svg_data(data, px).unwrap_or_else(|| panic!("{name} does not parse"));
                // A full tile: the centre and the middle of each edge are opaque.
                let at = |x: usize, y: usize| img.pixels[y * px as usize + x].a();
                let (mid, last) = (px as usize / 2, px as usize - 1);
                assert_eq!(at(mid, mid), 255, "{name}");
                assert!(at(mid, 0) > 200 && at(0, mid) > 200 && at(mid, last) > 200, "{name} is not a full tile");
                // And a white (or, on a light tile, dark) glyph is on it: not one colour all over.
                let first = img.pixels[mid * px as usize + mid / 4];
                let far = |p: &egui::Color32| {
                    (p.r() as i32 - first.r() as i32).abs() + (p.g() as i32 - first.g() as i32).abs() + (p.b() as i32 - first.b() as i32).abs()
                };
                assert!(img.pixels.iter().any(|p| p.a() == 255 && far(p) > 150), "{name} has no glyph");
            }
        }
    }

    #[test]
    fn paths_give_the_right_variant() {
        let p = path("firefox");
        let (_, big, small) = FILES.iter().find(|(n, _, _)| *n == "firefox").unwrap();
        assert_eq!(bytes(&p, 256), Some(*big));
        assert_eq!(bytes(&p, 64), Some(*small));
        assert_eq!(bytes(&path("no-such-icon"), 64), None);
        assert_eq!(bytes(Path::new(r"C:\x\firefox.svg"), 64), None);
        assert!(raster::is_svg(&p));
    }

    #[test]
    fn the_index_lists_every_icon_and_only_those() {
        let index = index();
        assert_eq!(index.len(), FILES.len());
        for (name, _, _) in FILES {
            let p = index.get(name).unwrap();
            assert!(bytes(&p, 256).is_some(), "{name}");
        }
    }

    fn find(exe: Option<&str>, id: &str, name: Option<&str>) -> Option<String> {
        let c = candidates(&AppRef { id, exe: exe.map(Path::new), name });
        index().lookup(&c).map(|p| p.file_stem().unwrap().to_string_lossy().to_string())
    }

    #[test]
    fn this_machines_apps_find_their_tiles() {
        let exe = |e: &str| find(Some(e), "", None);
        assert_eq!(exe(r"C:\Program Files\Mozilla Firefox\firefox.exe").as_deref(), Some("firefox"));
        assert_eq!(exe(r"C:\Users\u\AppData\Local\Discord\app-1.0.9268\Discord.exe").as_deref(), Some("discord"));
        assert_eq!(find(Some(r"C:\Steam\bin\cef\cef.win64\steamwebhelper.exe"), "Valve.Steam.Client", None).as_deref(), Some("steam"));
        assert_eq!(exe(r"C:\x\explorer.exe").as_deref(), Some("system-file-manager"));
        assert_eq!(exe(r"C:\x\taskmgr.exe").as_deref(), Some("utilities-system-monitor"));
        assert_eq!(exe(r"C:\x\cmd.exe").as_deref(), Some("utilities-terminal"));
        assert_eq!(exe(r"C:\x\7zFM.exe").as_deref(), Some("archive-manager"));
        assert_eq!(exe(r"C:\x\zed.exe").as_deref(), Some("zed"));
        assert_eq!(exe(r"C:\x\git-bash.exe").as_deref(), Some("git"));
        let id = |i: &str, n: &str| find(None, i, Some(n));
        assert_eq!(id("Microsoft.WindowsTerminal_8wekyb3d8bbwe!App", "Terminal").as_deref(), Some("utilities-terminal"));
        assert_eq!(id("Microsoft.WindowsCalculator_8wekyb3d8bbwe!App", "Calculator").as_deref(), Some("accessories-calculator"));
        assert_eq!(id("Microsoft.WindowsNotepad_8wekyb3d8bbwe!App", "Notepad").as_deref(), Some("accessories-text-editor"));
        assert_eq!(id("Microsoft.Paint_8wekyb3d8bbwe!App", "Paint").as_deref(), Some("accessories-drawing"));
        assert_eq!(id("Microsoft.Windows.Photos_8wekyb3d8bbwe!App", "Photos").as_deref(), Some("multimedia-photo-manager"));
        assert_eq!(id("Microsoft.ScreenSketch_8wekyb3d8bbwe!App", "Snipping Tool").as_deref(), Some("accessories-screenshot-tool"));
        assert_eq!(id("Microsoft.WindowsStore_8wekyb3d8bbwe!App", "Microsoft Store").as_deref(), Some("software-store"));
        assert_eq!(id("Microsoft.GamingApp_8wekyb3d8bbwe!Microsoft.Xbox.App", "XBOX").as_deref(), Some("xbox"));
        assert_eq!(id("Microsoft.XboxGamingOverlay_8wekyb3d8bbwe!App", "Game Bar").as_deref(), Some("applications-games"));
        assert_eq!(id("Agilebits.1Password_amwd9z03whsfe!Agilebits.OnePassword", "1Password").as_deref(), Some("1password"));
        assert_eq!(id("Microsoft.SecHealthUI_8wekyb3d8bbwe!SecHealthUI", "Windows Security").as_deref(), Some("preferences-system-privacy"));
        assert_eq!(
            id("windows.immersivecontrolpanel_cw5n1h2txyewy!microsoft.windows.immersivecontrolpanel", "Settings").as_deref(),
            Some("preferences-system")
        );
        let lnk = |n: &str| find(Some(&format!(r"C:\Start\{n}.lnk")), "", Some(n));
        assert_eq!(lnk("Helium").as_deref(), Some("web-browser"));
        assert_eq!(lnk("Call of Duty®").as_deref(), Some("applications-games"));
        assert_eq!(lnk("Tailscale").as_deref(), Some("tailscale"));
        assert_eq!(lnk("Remote Desktop Connection").as_deref(), Some("preferences-desktop-remote-desktop"));
        // An app the pack has nothing for keeps its own icon.
        assert_eq!(lnk("Superposition Benchmark"), None);
    }

    #[test]
    fn brands_without_a_tile_fall_back_to_their_kind() {
        let exe = |e: &str| find(Some(e), "", None);
        assert_eq!(exe(r"C:\x\opera.exe").as_deref(), Some("web-browser"));
        assert_eq!(exe(r"C:\x\Signal.exe").as_deref(), Some("internet-chat"));
        assert_eq!(exe(r"C:\x\EpicGamesLauncher.exe").as_deref(), Some("applications-games"));
        assert_eq!(exe(r"C:\x\KeePassXC.exe").as_deref(), Some("password-manager"));
        assert_eq!(exe(r"C:\x\VirtualBox.exe").as_deref(), Some("virtual-machine"));
        assert_eq!(exe(r"C:\x\WINWORD.EXE").as_deref(), Some("x-office-document"));
        assert_eq!(exe(r"C:\x\GitHubDesktop.exe").as_deref(), Some("github-desktop"));
    }
}
