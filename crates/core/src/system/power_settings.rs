//! Reading and writing the active power plan: the screen and sleep timeouts, the lid and power
//! button actions (on battery and plugged in), the plan itself, the power mode (the "overlay"
//! Windows 11 shows as Best power efficiency / Balanced / Best performance), and the battery.
//!
//! Everything here is `powrprof`: reads need nothing special; a write lands in the user's own
//! power-scheme settings and is made current with `PowerSetActiveScheme`. A refusal (an
//! organisation's policy, say) comes back as an error the page shows in one line.
//! `power_logic` decides what applies; this module only talks to Windows. A write can touch
//! the registry and notify the power service, so callers make it from a worker.

use super::power_logic::{Caps, Key, Status};
use windows::core::GUID;
use windows::Win32::Foundation::{HLOCAL, LocalFree, ERROR_ACCESS_DENIED};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::System::Power::{
    CallNtPowerInformation, GetSystemPowerStatus, PowerEnumerate, PowerGetActiveScheme, PowerReadACValueIndex,
    PowerReadDCValueIndex, PowerReadFriendlyName, PowerSetActiveScheme, PowerWriteACValueIndex,
    PowerWriteDCValueIndex, ACCESS_SCHEME, SystemPowerCapabilities, SYSTEM_POWER_CAPABILITIES, SYSTEM_POWER_STATUS,
};

const SUB_VIDEO: GUID = GUID::from_u128(0x7516b95f_f776_4464_8c53_06167f40cc99);
const VIDEOIDLE: GUID = GUID::from_u128(0x3c0bc021_c8a8_4e07_a973_6b14cbcb2b7e);
const SUB_SLEEP: GUID = GUID::from_u128(0x238c9fa8_0aad_41ed_83f4_97be242c8f20);
const STANDBYIDLE: GUID = GUID::from_u128(0x29f6c1db_86da_48c5_9fdb_f2b67b1f44da);
const SUB_BUTTONS: GUID = GUID::from_u128(0x4f971e89_eebd_4455_a8de_9e59040e7347);
const LIDACTION: GUID = GUID::from_u128(0x5ca83367_6e45_459f_a27b_476b1d01c936);
const PBUTTONACTION: GUID = GUID::from_u128(0x7648efa3_dd9c_4e3e_b566_50f929386280);

/// Which column of a setting: plugged in, or on battery.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Source {
    Ac,
    Dc,
}

/// The power mode Windows 11 offers beside the plan.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Efficiency,
    Balanced,
    Performance,
}

impl Mode {
    pub const ALL: [Mode; 3] = [Mode::Efficiency, Mode::Balanced, Mode::Performance];

    pub fn label(self) -> &'static str {
        match self {
            Mode::Efficiency => "Best power efficiency",
            Mode::Balanced => "Balanced",
            Mode::Performance => "Best performance",
        }
    }

    fn guid(self) -> GUID {
        match self {
            Mode::Efficiency => GUID::from_u128(0x961cc777_2547_4f9d_8174_7d86181b8a7a),
            Mode::Balanced => GUID::from_u128(0),
            Mode::Performance => GUID::from_u128(0x3af9b8d9_7c97_431d_ad78_34a8bfea439f),
        }
    }

    fn from_guid(g: &GUID) -> Option<Mode> {
        Mode::ALL.into_iter().find(|m| m.guid() == *g)
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Plan {
    pub guid: GUID,
    pub name: String,
}

/// A setting's two values; a side is `None` when Windows does not have it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Pair {
    pub ac: Option<u32>,
    pub dc: Option<u32>,
}

/// Everything the page shows, read in one go.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Snapshot {
    pub caps: Caps,
    pub plans: Vec<Plan>,
    pub active: Option<GUID>,
    pub mode: Option<Mode>,
    pub screen_off: Pair,
    pub sleep: Pair,
    pub lid: Pair,
    pub power_button: Pair,
}

impl Snapshot {
    pub fn pair(&self, key: Key) -> Pair {
        match key {
            Key::ScreenOff => self.screen_off,
            Key::Sleep => self.sleep,
            Key::LidClose => self.lid,
            Key::PowerButton => self.power_button,
        }
    }

    pub fn active_name(&self) -> String {
        self.plans.iter().find(|p| Some(p.guid) == self.active).map(|p| p.name.clone()).unwrap_or_else(|| "Unknown plan".to_owned())
    }
}

fn ids(key: Key) -> (GUID, GUID) {
    match key {
        Key::ScreenOff => (SUB_VIDEO, VIDEOIDLE),
        Key::Sleep => (SUB_SLEEP, STANDBYIDLE),
        Key::LidClose => (SUB_BUTTONS, LIDACTION),
        Key::PowerButton => (SUB_BUTTONS, PBUTTONACTION),
    }
}

pub fn capabilities() -> Caps {
    let mut c = SYSTEM_POWER_CAPABILITIES::default();
    // SAFETY: the output buffer is the structure this level fills.
    let status = unsafe {
        CallNtPowerInformation(
            SystemPowerCapabilities,
            None,
            0,
            Some(&mut c as *mut _ as *mut _),
            std::mem::size_of::<SYSTEM_POWER_CAPABILITIES>() as u32,
        )
    };
    if status.0 != 0 {
        return Caps::default();
    }
    Caps {
        battery: c.SystemBatteriesPresent,
        lid: c.LidPresent,
        sleep: c.SystemS1 || c.SystemS2 || c.SystemS3 || c.AoAc,
        hibernate: c.HiberFilePresent,
        power_button: c.PowerButtonPresent,
    }
}

/// The battery and the power line now. `None` on a machine Windows cannot ask.
pub fn status() -> Option<Status> {
    let mut s = SYSTEM_POWER_STATUS::default();
    // SAFETY: plain out-parameter.
    unsafe { GetSystemPowerStatus(&mut s) }.ok()?;
    Some(Status {
        ac_line: s.ACLineStatus,
        charging: s.BatteryFlag != 255 && s.BatteryFlag & 8 != 0,
        percent: s.BatteryLifePercent,
        seconds_left: s.BatteryLifeTime,
        saver_on: s.SystemStatusFlag == 1,
    })
}

/// Whether the machine has a battery to report on (the status flag says "no system battery").
pub fn has_battery(s: &Status, caps: &Caps) -> bool {
    caps.battery && s.percent != 255
}

pub fn active_scheme() -> Option<GUID> {
    let mut p: *mut GUID = std::ptr::null_mut();
    // SAFETY: Windows allocates the GUID; it is copied out and freed.
    unsafe {
        if PowerGetActiveScheme(None, &mut p).0 != 0 || p.is_null() {
            return None;
        }
        let g = *p;
        let _ = LocalFree(Some(HLOCAL(p as *mut _)));
        Some(g)
    }
}

fn friendly_name(scheme: &GUID) -> String {
    let mut size = 0u32;
    // SAFETY: first call asks for the size, second fills a buffer of that size.
    unsafe {
        let _ = PowerReadFriendlyName(None, Some(scheme), None, None, None, &mut size);
        if size < 2 {
            return String::new();
        }
        let mut buf = vec![0u8; size as usize];
        if PowerReadFriendlyName(None, Some(scheme), None, None, Some(buf.as_mut_ptr()), &mut size).0 != 0 {
            return String::new();
        }
        let wide: Vec<u16> = buf.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|c| *c != 0).collect();
        String::from_utf16_lossy(&wide)
    }
}

pub fn plans() -> Vec<Plan> {
    let mut out = Vec::new();
    for index in 0.. {
        let mut g = GUID::zeroed();
        let mut size = std::mem::size_of::<GUID>() as u32;
        // SAFETY: the buffer is one GUID, as ACCESS_SCHEME enumerates.
        let r = unsafe { PowerEnumerate(None, None, None, ACCESS_SCHEME, index, Some(&mut g as *mut _ as *mut u8), &mut size) };
        if r.0 != 0 || index > 64 {
            break;
        }
        let name = friendly_name(&g);
        out.push(Plan { guid: g, name: if name.is_empty() { "Custom plan".to_owned() } else { name } });
    }
    out
}

fn read_pair(scheme: &GUID, key: Key) -> Pair {
    let (sub, set) = ids(key);
    let (mut ac, mut dc) = (0u32, 0u32);
    // SAFETY: out-parameters on the stack.
    unsafe {
        let a = PowerReadACValueIndex(None, Some(scheme), Some(&sub), Some(&set), &mut ac).0 == 0;
        let d = PowerReadDCValueIndex(None, Some(scheme), Some(&sub), Some(&set), &mut dc) == 0;
        Pair { ac: a.then_some(ac), dc: d.then_some(dc) }
    }
}

type GetOverlay = unsafe extern "system" fn(*mut GUID) -> u32;
type SetOverlay = unsafe extern "system" fn(GUID) -> u32;

/// `powrprof`'s overlay calls are not in the SDK headers, so they are looked up by name.
fn overlay_fn(name: &[u8]) -> Option<*const ()> {
    let wide: Vec<u16> = "powrprof.dll".encode_utf16().chain([0]).collect();
    // SAFETY: the name is NUL-terminated; powrprof stays loaded.
    unsafe {
        let m = LoadLibraryW(windows::core::PCWSTR(wide.as_ptr())).ok()?;
        let mut n = name.to_vec();
        n.push(0);
        GetProcAddress(m, windows::core::PCSTR(n.as_ptr())).map(|f| f as *const ())
    }
}

/// The power mode in force, or `None` when this Windows has none.
pub fn mode() -> Option<Mode> {
    let f = overlay_fn(b"PowerGetEffectiveOverlayScheme")?;
    // SAFETY: the signature is `ULONG PowerGetEffectiveOverlayScheme(GUID*)`.
    let f: GetOverlay = unsafe { std::mem::transmute(f) };
    let mut g = GUID::zeroed();
    // SAFETY: out-parameter.
    if unsafe { f(&mut g) } != 0 {
        return None;
    }
    // An overlay this page does not name (Max performance) reads as Balanced's neighbour: shown
    // as nothing chosen, so it is not changed by looking.
    Mode::from_guid(&g)
}

pub fn read() -> Snapshot {
    let caps = capabilities();
    let active = active_scheme();
    let pair = |k| active.as_ref().map(|s| read_pair(s, k)).unwrap_or_default();
    Snapshot {
        caps,
        plans: plans(),
        active,
        mode: mode(),
        screen_off: pair(Key::ScreenOff),
        sleep: pair(Key::Sleep),
        lid: pair(Key::LidClose),
        power_button: pair(Key::PowerButton),
    }
}

/// Why a write did not happen.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Refused {
    /// Windows said access denied: policy, or this account.
    Denied,
    Failed(u32),
}

impl Refused {
    pub fn line(&self) -> String {
        match self {
            Refused::Denied => "Windows refused: this change needs administrator rights or is set by your organisation.".to_owned(),
            Refused::Failed(code) => format!("Windows could not apply it (error {code})."),
        }
    }

    fn from(code: u32) -> Refused {
        if code == ERROR_ACCESS_DENIED.0 {
            Refused::Denied
        } else {
            Refused::Failed(code)
        }
    }
}

/// Set one value of the active plan and make the plan current again so it takes hold.
pub fn write(key: Key, source: Source, value: u32) -> Result<(), Refused> {
    let scheme = active_scheme().ok_or(Refused::Failed(0))?;
    let (sub, set) = ids(key);
    // SAFETY: GUID pointers are to locals that outlive the calls.
    unsafe {
        let r = match source {
            Source::Ac => PowerWriteACValueIndex(None, &scheme, Some(&sub), Some(&set), value).0,
            Source::Dc => PowerWriteDCValueIndex(None, &scheme, Some(&sub), Some(&set), value),
        };
        if r != 0 {
            return Err(Refused::from(r));
        }
        let r = PowerSetActiveScheme(None, Some(&scheme)).0;
        if r != 0 {
            return Err(Refused::from(r));
        }
    }
    Ok(())
}

/// Make another plan the active one.
pub fn set_plan(guid: &GUID) -> Result<(), Refused> {
    // SAFETY: pointer to a caller-owned GUID.
    let r = unsafe { PowerSetActiveScheme(None, Some(guid)) }.0;
    if r == 0 {
        Ok(())
    } else {
        Err(Refused::from(r))
    }
}

pub fn set_mode(mode: Mode) -> Result<(), Refused> {
    let f = overlay_fn(b"PowerSetActiveOverlayScheme").ok_or(Refused::Failed(1))?;
    // SAFETY: the signature is `ULONG PowerSetActiveOverlayScheme(GUID)`.
    let f: SetOverlay = unsafe { std::mem::transmute(f) };
    // SAFETY: plain call.
    match unsafe { f(mode.guid()) } {
        0 => Ok(()),
        code => Err(Refused::from(code)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_round_trip_their_guids() {
        for m in Mode::ALL {
            assert_eq!(Mode::from_guid(&m.guid()), Some(m));
        }
        assert_eq!(Mode::from_guid(&GUID::from_u128(7)), None);
    }

    #[test]
    fn a_refusal_is_told_plainly() {
        assert_eq!(Refused::from(5), Refused::Denied);
        assert!(Refused::Denied.line().contains("administrator"));
        assert!(Refused::from(87).line().contains("87"));
    }

    /// Changes each kind of setting, reads it back, and puts it back, printing what Windows said
    /// to each write (unelevated). Touches the real plan: run by hand with `--ignored --nocapture`.
    #[test]
    #[ignore]
    fn every_setter_takes_and_restores() {
        let orig = read();
        let mut log = Vec::new();
        for (key, src) in [(Key::ScreenOff, Source::Ac), (Key::ScreenOff, Source::Dc), (Key::Sleep, Source::Ac), (Key::Sleep, Source::Dc), (Key::PowerButton, Source::Ac), (Key::LidClose, Source::Dc)] {
            let was = match src {
                Source::Ac => orig.pair(key).ac,
                Source::Dc => orig.pair(key).dc,
            }
            .unwrap();
            let other = if key.is_timeout() { if was == 1200 { 1500 } else { 1200 } } else if was == 0 { 3 } else { 0 };
            let a = write(key, src, other);
            let now = read().pair(key);
            let seen = if src == Source::Ac { now.ac } else { now.dc };
            let b = write(key, src, was);
            let after = read().pair(key);
            let seen_back = if src == Source::Ac { after.ac } else { after.dc };
            log.push(format!("{key:?}/{src:?}: set {other} -> {a:?}, read {seen:?}; restore {was} -> {b:?}, read {seen_back:?}"));
            assert_eq!(seen_back, Some(was));
        }
        if let Some(m) = orig.mode {
            let other = if m == Mode::Efficiency { Mode::Balanced } else { Mode::Efficiency };
            let a = set_mode(other);
            let seen = mode();
            let b = set_mode(m);
            log.push(format!("mode {other:?} -> {a:?}, read {seen:?}; restore {m:?} -> {b:?}, read {:?}", mode()));
            assert_eq!(mode(), Some(m));
        }
        if let Some(active) = orig.active {
            if let Some(other) = orig.plans.iter().find(|p| p.guid != active) {
                let a = set_plan(&other.guid);
                let seen = active_scheme();
                let b = set_plan(&active);
                log.push(format!("plan {} -> {a:?}, now {:?}; restore -> {b:?}", other.name, seen == Some(other.guid)));
                assert_eq!(active_scheme(), Some(active));
            }
        }
        println!("{}", log.join("\n"));
        assert_eq!(read(), orig);
    }

    /// Reads the machine's real settings: `cargo test -p shellman-core power_settings -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn read_this_machine() {
        let s = read();
        println!("{s:#?}\n{:?}", status());
    }
}
