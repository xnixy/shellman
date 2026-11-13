//! Date, time and region: the time zone and its daylight saving switch, the 12 or 24 hour clock,
//! the short and long date formats and the first day of the week.
//!
//! The time zone is `EnumDynamicTimeZoneInformation` / `SetDynamicTimeZoneInformation` (which
//! needs `SeTimeZonePrivilege`; standard users hold it, switched off, so it is switched on for the
//! call). The rest are the user's own values under `HKCU\Control Panel\International`, the same
//! ones Windows' Settings writes, followed by the `WM_SETTINGCHANGE` "intl" broadcast. Writes
//! wait on that broadcast, so callers make them from a worker.

use super::region_logic::{self as logic, Zone};
use crate::registry::{Hive, Real, Registry, Value};
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_NOT_ALL_ASSIGNED, LPARAM, LUID, WPARAM};
use windows::Win32::Globalization::{
    EnumDateFormatsExEx, GetDateFormatEx, GetTimeFormatEx, DATE_LONGDATE, DATE_SHORTDATE, ENUM_DATE_FORMATS_FLAGS,
};
use windows::Win32::Security::{
    AdjustTokenPrivileges, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES, SE_PRIVILEGE_ENABLED, SE_TIME_ZONE_NAME,
    TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::Win32::System::Time::{EnumDynamicTimeZoneInformation, GetDynamicTimeZoneInformation, SetDynamicTimeZoneInformation, DYNAMIC_TIME_ZONE_INFORMATION};
use windows::Win32::UI::WindowsAndMessaging::{SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE};

const INTL: &str = "Control Panel\\International";
const ZONES: &str = "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Time Zones";

/// What is set now.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Current {
    pub key: String,
    pub observes_dst: bool,
    /// "Adjust for daylight saving time automatically" is off.
    pub dst_disabled: bool,
}

/// The user-locale values the page edits.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Intl {
    pub short_time: String,
    pub long_time: String,
    pub short_date: String,
    pub long_date: String,
    /// 0 Monday to 6 Sunday.
    pub first_day: u32,
}

impl Intl {
    pub fn is_24h(&self) -> bool {
        logic::is_24h(&self.short_time)
    }
}

/// Why a write did not happen.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Refused {
    /// This account does not hold the "change the time zone" right.
    NoPrivilege,
    Failed(String),
}

impl Refused {
    pub fn line(&self) -> String {
        match self {
            Refused::NoPrivilege => "Windows refused: this account may not change the time zone (it needs administrator rights).".to_owned(),
            Refused::Failed(why) => format!("Windows could not apply it: {why}."),
        }
    }
}

fn wide_to_string(buf: &[u16]) -> String {
    let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

fn zone_of(info: &DYNAMIC_TIME_ZONE_INFORMATION) -> Zone {
    let key = wide_to_string(&info.TimeZoneKeyName);
    let display = Real
        .get(Hive::Hklm, &format!("{ZONES}\\{key}"), "Display")
        .and_then(|v| v.text().map(str::to_owned))
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| format!("{} {}", logic::utc_label(info.Bias), wide_to_string(&info.StandardName)));
    Zone { key, display, bias: info.Bias, observes_dst: info.DaylightDate.wMonth != 0 }
}

/// Every time zone Windows knows, west to east.
pub fn zones() -> Vec<Zone> {
    let mut out = Vec::new();
    for i in 0..1024 {
        let mut info = DYNAMIC_TIME_ZONE_INFORMATION::default();
        // SAFETY: the structure is the one this call fills; it returns an error code at the end.
        if unsafe { EnumDynamicTimeZoneInformation(i, &mut info) } != 0 {
            break;
        }
        out.push(zone_of(&info));
    }
    logic::sort_zones(&mut out);
    out
}

pub fn current() -> Option<Current> {
    let mut info = DYNAMIC_TIME_ZONE_INFORMATION::default();
    // SAFETY: out-parameter.
    if unsafe { GetDynamicTimeZoneInformation(&mut info) } == u32::MAX {
        return None;
    }
    Some(Current {
        key: wide_to_string(&info.TimeZoneKeyName),
        observes_dst: info.DaylightDate.wMonth != 0,
        dst_disabled: info.DynamicDaylightTimeDisabled,
    })
}

/// Switch `SeTimeZonePrivilege` on for this process.
fn enable_time_zone_privilege() -> bool {
    // SAFETY: plain FFI; the token handle is closed after use.
    unsafe {
        let mut token = Default::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut luid = LUID::default();
        let mut ok = LookupPrivilegeValueW(PCWSTR::null(), SE_TIME_ZONE_NAME, &mut luid).is_ok();
        if ok {
            let wanted = TOKEN_PRIVILEGES {
                PrivilegeCount: 1,
                Privileges: [LUID_AND_ATTRIBUTES { Luid: luid, Attributes: SE_PRIVILEGE_ENABLED }],
            };
            ok = AdjustTokenPrivileges(token, false, Some(&wanted), 0, None, None).is_ok() && GetLastError() != ERROR_NOT_ALL_ASSIGNED;
        }
        let _ = CloseHandle(token);
        ok
    }
}

fn set_info(info: &DYNAMIC_TIME_ZONE_INFORMATION) -> Result<(), Refused> {
    if !enable_time_zone_privilege() {
        return Err(Refused::NoPrivilege);
    }
    // SAFETY: `info` is a complete structure.
    unsafe { SetDynamicTimeZoneInformation(info) }.map_err(|e| {
        // ERROR_PRIVILEGE_NOT_HELD arrives as 0x80070522.
        if e.code().0 as u32 == 0x8007_0522 {
            Refused::NoPrivilege
        } else {
            Refused::Failed(e.message())
        }
    })
}

/// Make the zone named `key` the time zone, keeping the daylight saving switch as it is.
pub fn set_zone(key: &str) -> Result<(), Refused> {
    let keep_off = current().is_some_and(|c| c.dst_disabled);
    for i in 0..1024 {
        let mut info = DYNAMIC_TIME_ZONE_INFORMATION::default();
        // SAFETY: as in `zones`.
        if unsafe { EnumDynamicTimeZoneInformation(i, &mut info) } != 0 {
            break;
        }
        if wide_to_string(&info.TimeZoneKeyName) == key {
            info.DynamicDaylightTimeDisabled = keep_off;
            return set_info(&info);
        }
    }
    Err(Refused::Failed("that time zone is not in Windows' list".to_owned()))
}

/// Turn "adjust for daylight saving time automatically" on or off for the current zone.
pub fn set_dst_adjust(on: bool) -> Result<(), Refused> {
    let mut info = DYNAMIC_TIME_ZONE_INFORMATION::default();
    // SAFETY: out-parameter.
    if unsafe { GetDynamicTimeZoneInformation(&mut info) } == u32::MAX {
        return Err(Refused::Failed("the time zone cannot be read".to_owned()));
    }
    info.DynamicDaylightTimeDisabled = !on;
    set_info(&info)
}

fn text(name: &str) -> String {
    match Real.get(Hive::Hkcu, INTL, name) {
        Some(Value::Text(s)) | Some(Value::Expand(s)) => s,
        _ => String::new(),
    }
}

pub fn read_intl() -> Intl {
    Intl {
        short_time: text("sShortTime"),
        long_time: text("sTimeFormat"),
        short_date: text("sShortDate"),
        long_date: text("sLongDate"),
        first_day: text("iFirstDayOfWeek").trim().parse().unwrap_or(0).min(6),
    }
}

/// Tell every window the user's regional settings changed.
fn broadcast_intl() {
    let intl = HSTRING::from("intl");
    // SAFETY: the string outlives the call, which waits for the answers (or a second each).
    unsafe {
        let _ = SendMessageTimeoutW(HWND_BROADCAST, WM_SETTINGCHANGE, WPARAM(0), LPARAM(intl.as_ptr() as isize), SMTO_ABORTIFHUNG, 1000, None);
    }
}

fn put(items: &[(&str, &str)]) -> Result<(), Refused> {
    for (name, value) in items {
        Real.set_text(Hive::Hkcu, INTL, name, value).map_err(Refused::Failed)?;
    }
    broadcast_intl();
    Ok(())
}

/// Switch the clock between 12 and 24 hours: the short and long time patterns, and `iTime`.
pub fn set_24h(on: bool) -> Result<(), Refused> {
    let now = read_intl();
    let (short, long) = if on { (logic::to_24h(&now.short_time), logic::to_24h(&now.long_time)) } else { (logic::to_12h(&now.short_time), logic::to_12h(&now.long_time)) };
    put(&[("iTime", if on { "1" } else { "0" }), ("sShortTime", &short), ("sTimeFormat", &long)])
}

pub fn set_short_date(pattern: &str) -> Result<(), Refused> {
    put(&[("sShortDate", pattern)])
}

pub fn set_long_date(pattern: &str) -> Result<(), Refused> {
    put(&[("sLongDate", pattern)])
}

/// 0 Monday to 6 Sunday.
pub fn set_first_day(day: u32) -> Result<(), Refused> {
    put(&[("iFirstDayOfWeek", &day.min(6).to_string())])
}

unsafe extern "system" fn collect(pattern: PCWSTR, _id: u32, lparam: LPARAM) -> windows::core::BOOL {
    // SAFETY: `lparam` is the `Vec` handed to `EnumDateFormatsExEx`, alive for the call.
    let list = unsafe { &mut *(lparam.0 as *mut Vec<String>) };
    list.push(unsafe { pattern.to_string() }.unwrap_or_default());
    true.into()
}

/// The date patterns the user's locale offers, short or long.
pub fn date_formats(short: bool) -> Vec<String> {
    let mut list: Vec<String> = Vec::new();
    let flags = if short { DATE_SHORTDATE } else { DATE_LONGDATE };
    // SAFETY: the callback only pushes onto `list`, which outlives the call.
    unsafe {
        let _ = EnumDateFormatsExEx(Some(collect), PCWSTR::null(), ENUM_DATE_FORMATS_FLAGS(flags.0), LPARAM(&mut list as *mut _ as isize));
    }
    list.retain(|p| !p.is_empty());
    list
}

/// Today formatted with `pattern`, for a preview.
pub fn sample_date(pattern: &str) -> String {
    let p = HSTRING::from(pattern);
    let mut buf = [0u16; 128];
    // SAFETY: valid buffers; no date means now.
    let n = unsafe { GetDateFormatEx(PCWSTR::null(), Default::default(), None, &p, Some(&mut buf), PCWSTR::null()) };
    wide_to_string(&buf[..(n.max(0) as usize).min(buf.len())])
}

/// The time now formatted with `pattern`, for a preview.
pub fn sample_time(pattern: &str) -> String {
    let p = HSTRING::from(pattern);
    let mut buf = [0u16; 128];
    // SAFETY: valid buffers; no time means now.
    let n = unsafe { GetTimeFormatEx(PCWSTR::null(), Default::default(), None, &p, Some(&mut buf)) };
    wide_to_string(&buf[..(n.max(0) as usize).min(buf.len())])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_zone_list_has_the_current_zone() {
        let list = zones();
        assert!(list.len() > 50);
        if let Some(c) = current() {
            assert!(list.iter().any(|z| z.key == c.key), "{}", c.key);
        }
        assert!(list.windows(2).all(|w| w[0].bias >= w[1].bias));
    }

    #[test]
    fn the_locale_offers_date_patterns() {
        assert!(!date_formats(true).is_empty());
        assert!(!date_formats(false).is_empty());
        assert!(!sample_date("yyyy").is_empty());
    }

    #[test]
    fn a_refusal_is_told_plainly() {
        assert!(Refused::NoPrivilege.line().contains("administrator"));
        assert!(Refused::Failed("nope".into()).line().contains("nope"));
    }

    /// Changes each setting, reads it back, and puts it back; prints what Windows said.
    /// Touches the real system: `cargo test -p shellman-core region::tests::every -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn every_setter_takes_and_restores() {
        let zone = current().unwrap();
        let intl = read_intl();
        let mut log = Vec::new();
        // The zone: another one, then back.
        let other = zones().into_iter().find(|z| z.key != zone.key && z.observes_dst == zone.observes_dst && z.key.contains("Eastern")).map(|z| z.key).unwrap_or("UTC".into());
        let a = set_zone(&other);
        let seen = current().map(|c| c.key);
        let b = set_zone(&zone.key);
        log.push(format!("zone -> {other}: {a:?} read {seen:?}; restore {} -> {b:?} read {:?}", zone.key, current().map(|c| c.key)));
        // Daylight saving.
        let c = set_dst_adjust(zone.dst_disabled);
        let seen = current().map(|c| c.dst_disabled);
        let d = set_dst_adjust(!zone.dst_disabled);
        log.push(format!("dst flip {c:?} read {seen:?}; restore {d:?} read {:?}", current().map(|c| c.dst_disabled)));
        // The clock.
        let e = set_24h(!intl.is_24h());
        let seen = read_intl();
        let f = set_24h(intl.is_24h());
        let _ = put(&[("sShortTime", &intl.short_time), ("sTimeFormat", &intl.long_time)]);
        log.push(format!("24h {e:?} read {} / {}; restore {f:?}", seen.short_time, seen.long_time));
        // Date formats and the first day.
        let g = set_short_date("yyyy-MM-dd");
        let h = set_long_date("d MMMM yyyy");
        let i = set_first_day((intl.first_day + 1) % 7);
        let seen = read_intl();
        log.push(format!("dates {g:?} {h:?} {i:?} read {} | {} | {}", seen.short_date, seen.long_date, seen.first_day));
        let _ = set_short_date(&intl.short_date);
        let _ = set_long_date(&intl.long_date);
        let _ = set_first_day(intl.first_day);
        println!("{}", log.join("\n"));
        assert_eq!(read_intl(), intl);
        assert_eq!(current(), Some(zone));
    }
}
