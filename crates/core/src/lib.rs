//! Shellman's Win32 shell plumbing: file operations, thumbnails, the shell menu, folder and
//! window watching, the app-window list, live window previews, and config.

pub mod app_host;
pub mod apps_index;
pub mod calendar;
pub mod config;
pub mod credentials;
pub mod desktop_aside;
pub mod desktop_menu;
pub mod dismiss;
pub mod elevate;
pub mod foreground;
pub mod frames;
pub mod icon_pack;
pub mod fsops;
pub mod installed;
pub mod live_thumb;
pub mod monitors;
pub mod net;
pub mod notify;
pub mod open_with;
pub mod outside;
pub mod places;
pub mod places_reg;
pub mod procs;
pub mod recycle_bin;
pub mod registry;
pub mod services;
pub mod settings;
pub mod shell_menu;
pub mod sibling;
pub mod startup;
pub mod summon;
pub mod system;
pub mod taskbar_list;
pub mod terminal;
pub mod thumbs;
pub mod timing;
pub mod transfer;
pub mod warm;
pub mod watch;
pub mod weather;
pub mod winlist;
pub mod wsl;

/// The temp folder in its long form, for comparing a path with one Windows returns (tests):
/// `%TEMP%` can be a short 8.3 path (GitHub's runners: `C:\Users\RUNNER~1\...`), while the
/// shell and file ids give back the long one.
pub fn long_temp_dir() -> std::path::PathBuf {
    use windows::core::HSTRING;
    use windows::Win32::Storage::FileSystem::GetLongPathNameW;
    let short = std::env::temp_dir();
    let mut buf = vec![0u16; 1024];
    let n = unsafe { GetLongPathNameW(&HSTRING::from(short.as_os_str()), Some(&mut buf)) } as usize;
    if n == 0 || n > buf.len() {
        return short;
    }
    std::path::PathBuf::from(String::from_utf16_lossy(&buf[..n]))
}
