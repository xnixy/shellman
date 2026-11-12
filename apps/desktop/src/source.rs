//! What the user's wallpaper currently is: a picture path per monitor, the fit mode, and the
//! background colour.
//!
//! The first choice is `IDesktopWallpaper`, which knows each monitor's own picture. If that COM
//! object cannot be created (it is normally served by Explorer), the older settings are used: the
//! path from `SystemParametersInfo` and the `Control Panel\Desktop` registry values. Runs on the
//! worker thread with COM already initialized.

use crate::fit::{Area, Fit};
use std::path::PathBuf;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::RECT;
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_INPROC_SERVER, CLSCTX_LOCAL_SERVER};
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};
use windows::Win32::UI::Shell::{
    DesktopWallpaper, IDesktopWallpaper, DESKTOP_WALLPAPER_POSITION, DWPOS_CENTER, DWPOS_FIT,
    DWPOS_SPAN, DWPOS_STRETCH, DWPOS_TILE,
};
use windows::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETDESKWALLPAPER, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS};

/// The wallpaper of one monitor.
#[derive(Clone, Debug)]
pub struct Setting {
    /// None when the monitor has no picture, only the colour.
    pub path: Option<PathBuf>,
    pub fit: Fit,
    /// A Win32 COLORREF (0x00BBGGRR).
    pub color: u32,
}

/// Settings for each of `monitors`, in the same order.
pub fn read(monitors: &[Area]) -> Vec<Setting> {
    from_com(monitors).unwrap_or_else(|| {
        let one = from_registry();
        vec![one; monitors.len()]
    })
}

/// Ask `IDesktopWallpaper`. None if it is unavailable or reports nothing usable.
fn from_com(monitors: &[Area]) -> Option<Vec<Setting>> {
    unsafe {
        let dw: IDesktopWallpaper =
            CoCreateInstance(&DesktopWallpaper, None, CLSCTX_LOCAL_SERVER | CLSCTX_INPROC_SERVER).ok()?;
        let fit = fit_from_position(dw.GetPosition().ok()?);
        let color = dw.GetBackgroundColor().map(|c| c.0).unwrap_or(0);
        // Each COM monitor id with its rectangle and picture.
        let mut known: Vec<(Area, Option<PathBuf>)> = Vec::new();
        for i in 0..dw.GetMonitorDevicePathCount().ok()? {
            let Ok(id) = dw.GetMonitorDevicePathAt(i) else { continue };
            let rect = dw.GetMonitorRECT(PCWSTR(id.0));
            let path = dw.GetWallpaper(PCWSTR(id.0));
            let path = path.ok().map(|p| {
                let text = p.to_string().unwrap_or_default();
                CoTaskMemFree(Some(p.0 as _));
                text
            });
            CoTaskMemFree(Some(id.0 as _));
            if let Ok(r) = rect {
                let path = path.filter(|p| !p.is_empty()).map(PathBuf::from);
                known.push((area(r), path));
            }
        }
        if known.is_empty() {
            return None;
        }
        Some(
            monitors
                .iter()
                .enumerate()
                .map(|(i, m)| {
                    // By rectangle; by position in the list if the rectangles disagree.
                    let hit = known.iter().find(|(a, _)| a == m).or_else(|| known.get(i));
                    Setting { path: hit.and_then(|(_, p)| p.clone()), fit, color }
                })
                .collect(),
        )
    }
}

fn area(r: RECT) -> Area {
    Area::new(r.left, r.top, r.right - r.left, r.bottom - r.top)
}

fn fit_from_position(p: DESKTOP_WALLPAPER_POSITION) -> Fit {
    match p {
        DWPOS_CENTER => Fit::Center,
        DWPOS_TILE => Fit::Tile,
        DWPOS_STRETCH => Fit::Stretch,
        DWPOS_FIT => Fit::Letterbox,
        DWPOS_SPAN => Fit::Span,
        _ => Fit::Fill,
    }
}

/// The classic settings: one picture for every monitor.
fn from_registry() -> Setting {
    let path = system_wallpaper_path().or_else(transcoded_path);
    let style = reg_string(w!("Control Panel\\Desktop"), w!("WallpaperStyle")).unwrap_or_default();
    let tile = reg_string(w!("Control Panel\\Desktop"), w!("TileWallpaper")).unwrap_or_default();
    let color = reg_string(w!("Control Panel\\Colors"), w!("Background")).map(|s| parse_color(&s)).unwrap_or(0);
    Setting { path, fit: fit_from_registry(&style, &tile), color }
}

/// `WallpaperStyle` and `TileWallpaper` as Windows writes them.
fn fit_from_registry(style: &str, tile: &str) -> Fit {
    if tile.trim() == "1" {
        return Fit::Tile;
    }
    match style.trim() {
        "0" => Fit::Center,
        "2" => Fit::Stretch,
        "6" => Fit::Letterbox,
        "22" => Fit::Span,
        _ => Fit::Fill,
    }
}

/// "r g b" (0-255 each) to a COLORREF.
fn parse_color(text: &str) -> u32 {
    let mut c = text.split_whitespace().map(|n| n.parse::<u32>().unwrap_or(0).min(255));
    let (r, g, b) = (c.next().unwrap_or(0), c.next().unwrap_or(0), c.next().unwrap_or(0));
    r | g << 8 | b << 16
}

fn system_wallpaper_path() -> Option<PathBuf> {
    let mut buf = [0u16; 520];
    unsafe {
        SystemParametersInfoW(
            SPI_GETDESKWALLPAPER,
            buf.len() as u32,
            Some(buf.as_mut_ptr().cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
        .ok()?;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    (len > 0).then(|| PathBuf::from(String::from_utf16_lossy(&buf[..len])))
}

/// Windows keeps a ready-made copy of the current picture here (no file extension).
fn transcoded_path() -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var_os("APPDATA")?).join(r"Microsoft\Windows\Themes\TranscodedWallpaper");
    path.is_file().then_some(path)
}

fn reg_string(key: PCWSTR, value: PCWSTR) -> Option<String> {
    let mut buf = [0u16; 260];
    let mut bytes = (buf.len() * 2) as u32;
    unsafe {
        RegGetValueW(HKEY_CURRENT_USER, key, value, RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr().cast()), Some(&mut bytes))
            .ok()
            .ok()?;
    }
    let len = (bytes as usize / 2).min(buf.len());
    let text = String::from_utf16_lossy(&buf[..len]);
    Some(text.trim_end_matches('\0').to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_styles_map_to_fits() {
        assert_eq!(fit_from_registry("10", "0"), Fit::Fill);
        assert_eq!(fit_from_registry("6", "0"), Fit::Letterbox);
        assert_eq!(fit_from_registry("2", "0"), Fit::Stretch);
        assert_eq!(fit_from_registry("0", "0"), Fit::Center);
        assert_eq!(fit_from_registry("0", "1"), Fit::Tile);
        assert_eq!(fit_from_registry("22", "0"), Fit::Span);
        assert_eq!(fit_from_registry("", ""), Fit::Fill);
    }

    #[test]
    fn colours_are_read_as_rgb_triples() {
        assert_eq!(parse_color("0 0 0"), 0);
        assert_eq!(parse_color("255 128 1"), 0x0001_80FF);
        assert_eq!(parse_color("garbage"), 0);
        assert_eq!(parse_color("300 0 0"), 0xFF);
    }

    /// Run by hand with `--ignored --nocapture` to see what this machine reports.
    #[test]
    #[ignore]
    fn show_what_this_machine_has() {
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let why = unsafe {
            CoCreateInstance::<_, IDesktopWallpaper>(&DesktopWallpaper, None, CLSCTX_LOCAL_SERVER | CLSCTX_INPROC_SERVER).err()
        };
        println!("create:   {why:?}");
        let m = [Area::new(0, 0, 1920, 1080)];
        println!("com:      {:?}", from_com(&m));
        println!("registry: {:?}", from_registry());
    }
}
