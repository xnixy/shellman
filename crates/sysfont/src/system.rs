//! Odds and ends from Windows: the time, and when the PC last started. (Fontman's restart and
//! link opening are left out: shellman never restarts the PC from here.)

use windows::Win32::System::SystemInformation::GetTickCount64;

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// When Windows last started, in Unix seconds.
pub fn boot_time() -> u64 {
    let up = unsafe { GetTickCount64() } / 1000;
    now().saturating_sub(up)
}

/// "Oct 21, 14:02" in local time.
pub fn local_stamp(unix: u64) -> String {
    use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
    let ticks = (unix + 11_644_473_600) * 10_000_000;
    let file = FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    let mut utc = SYSTEMTIME::default();
    let mut local = SYSTEMTIME::default();
    unsafe {
        if FileTimeToSystemTime(&file, &mut utc).is_err()
            || SystemTimeToTzSpecificLocalTime(None, &utc, &mut local).is_err()
        {
            return String::new();
        }
    }
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let month = MONTHS
        .get(local.wMonth.saturating_sub(1) as usize)
        .unwrap_or(&"");
    format!(
        "{month} {}, {:02}:{:02}",
        local.wDay, local.wHour, local.wMinute
    )
}
