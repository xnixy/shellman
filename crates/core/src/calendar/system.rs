//! The system's side of [`Clock`]: the viewer's zone from Windows (with its DST history), and
//! zone names a calendar uses without defining from the ICU that ships with Windows (IANA names
//! such as `Europe/London`, and Windows names such as `W. Europe Standard Time`).

use super::time::{civil_from_days, days_from_civil, DAY};
use super::zone::Clock;
use windows::core::PCSTR;
use windows::Win32::Foundation::SYSTEMTIME;
use windows::Win32::Globalization::{
    ucal_close, ucal_get, ucal_getTimeZoneIDForWindowsID, ucal_open, ucal_setMillis, UCAL_DST_OFFSET, UCAL_GREGORIAN,
    UCAL_ZONE_OFFSET, U_ZERO_ERROR,
};
use windows::Win32::System::Time::{GetDynamicTimeZoneInformation, SystemTimeToTzSpecificLocalTimeEx, DYNAMIC_TIME_ZONE_INFORMATION};

pub struct SystemClock {
    zone: DYNAMIC_TIME_ZONE_INFORMATION,
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemClock {
    pub fn new() -> Self {
        let mut zone = DYNAMIC_TIME_ZONE_INFORMATION::default();
        unsafe { GetDynamicTimeZoneInformation(&mut zone) };
        Self { zone }
    }
}

fn to_system(utc: i64) -> SYSTEMTIME {
    let (y, m, d) = civil_from_days(utc.div_euclid(DAY));
    let t = utc.rem_euclid(DAY);
    SYSTEMTIME {
        wYear: y.clamp(1601, 30827) as u16,
        wMonth: m as u16,
        wDay: d as u16,
        wHour: (t / 3600) as u16,
        wMinute: (t / 60 % 60) as u16,
        wSecond: (t % 60) as u16,
        ..Default::default()
    }
}

fn from_system(s: &SYSTEMTIME) -> i64 {
    days_from_civil(s.wYear as i64, s.wMonth as u32, s.wDay as u32) * DAY + s.wHour as i64 * 3600 + s.wMinute as i64 * 60 + s.wSecond as i64
}

/// A zone's offset from ICU, by IANA or Windows name.
fn icu_offset(tzid: &str, utc: i64) -> Option<i64> {
    let mut id: Vec<u16> = tzid.encode_utf16().collect();
    unsafe {
        let mut status = U_ZERO_ERROR;
        // A Windows name ("GMT Standard Time") first becomes its IANA name.
        if !tzid.contains('/') {
            let mut out = [0u16; 128];
            let n = ucal_getTimeZoneIDForWindowsID(id.as_ptr(), id.len() as i32, PCSTR::null(), out.as_mut_ptr(), out.len() as i32, &mut status);
            if status.0 <= 0 && n > 0 {
                id = out[..n as usize].to_vec();
            } else if !tzid.eq_ignore_ascii_case("UTC") && !tzid.eq_ignore_ascii_case("GMT") {
                return None;
            }
        }
        let mut status = U_ZERO_ERROR;
        let cal = ucal_open(id.as_ptr(), id.len() as i32, PCSTR(c"en_US".as_ptr().cast()), UCAL_GREGORIAN, &mut status);
        if cal.is_null() || status.0 > 0 {
            if !cal.is_null() {
                ucal_close(cal);
            }
            return None;
        }
        ucal_setMillis(cal, utc as f64 * 1000.0, &mut status);
        let zone = ucal_get(cal as *const _, UCAL_ZONE_OFFSET, &mut status);
        let dst = ucal_get(cal as *const _, UCAL_DST_OFFSET, &mut status);
        ucal_close(cal);
        // ICU answers an unknown name with GMT and a warning; take only real answers.
        (status.0 <= 0).then_some((zone as i64 + dst as i64) / 1000)
    }
}

impl Clock for SystemClock {
    fn local_offset(&self, utc: i64) -> i64 {
        let u = to_system(utc);
        let mut local = SYSTEMTIME::default();
        match unsafe { SystemTimeToTzSpecificLocalTimeEx(Some(&self.zone), &u, &mut local) } {
            Ok(()) => from_system(&local) - from_system(&u),
            Err(_) => 0,
        }
    }

    fn zone_offset(&self, tzid: &str, utc: i64) -> Option<i64> {
        icu_offset(tzid, utc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::zone::wall_at;

    #[test]
    fn icu_knows_the_common_zones() {
        let c = SystemClock::new();
        // Windows ships ICU from 1903; on older systems these are simply unknown.
        if let Some(o) = c.zone_offset("Europe/London", wall_at(2026, 7, 1, 12, 0)) {
            assert_eq!(o, 3600);
            assert_eq!(c.zone_offset("Europe/London", wall_at(2026, 12, 1, 12, 0)), Some(0));
            assert_eq!(c.zone_offset("America/New_York", wall_at(2026, 12, 1, 12, 0)), Some(-5 * 3600));
            assert_eq!(c.zone_offset("GMT Standard Time", wall_at(2026, 7, 1, 12, 0)), Some(3600));
        }
        let local = c.local_offset(wall_at(2026, 7, 1, 12, 0));
        assert!(local.abs() <= 14 * 3600);
    }
}
