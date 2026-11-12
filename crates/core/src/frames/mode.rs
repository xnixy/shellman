//! Whether Windows' apps are in light or dark mode.

use windows::core::w;
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};

/// `SHELLMAN_FORCE_LIGHT`: look at the light palette without changing Windows' setting.
pub fn force_light() -> bool {
    std::env::var_os("SHELLMAN_FORCE_LIGHT").is_some()
}

/// Windows' app mode (the value the other shellman apps follow), or the forced light.
pub fn system_is_light() -> bool {
    if force_light() {
        return true;
    }
    let mut value: u32 = 0;
    let mut size = size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut value).cast()),
            Some(&mut size),
        )
    };
    status.is_ok() && value != 0
}
