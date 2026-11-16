//! Accessibility settings: what Windows' Settings shows under Accessibility, read and written the
//! supported way. Writes are `SystemParametersInfoW` with `SPIF_UPDATEINIFILE | SPIF_SENDCHANGE`
//! (persisted, and every window hears `WM_SETTINGCHANGE`), except transparency effects, which is
//! the user's `EnableTransparency` value followed by the `ImmersiveColorSet` broadcast Windows'
//! own Settings sends (shellman-session republishes DWM's transparency state on it). A struct
//! setting (sticky keys, sound sentry...) is read fresh, one bit flipped, and written back whole.
//!
//! What Windows has no supported setter for (text size, pointer size and colour, colour filters,
//! contrast themes, mono audio, scroll bars) is only read here; the page links to Windows'
//! Settings for it. Narrator, Magnifier, the on-screen keyboard and Live captions are their own
//! programs: "on" is the program running, and turning one on is starting it.
//!
//! Every write blocks while windows answer the broadcast, so callers make it from a worker.

use crate::registry::{Hive, Real, Registry, Value};
use windows::core::HSTRING;
use windows::Win32::Foundation::{CloseHandle, LPARAM, WPARAM};
use windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS};
use windows::Win32::UI::Accessibility::{FILTERKEYS, HIGHCONTRASTW, MOUSEKEYS, SOUNDSENTRYW, STICKYKEYS, TOGGLEKEYS};
use windows::Win32::UI::WindowsAndMessaging::{
    SendMessageTimeoutW, SystemParametersInfoW, HWND_BROADCAST, SMTO_ABORTIFHUNG, SPIF_SENDCHANGE, SPIF_UPDATEINIFILE,
    SYSTEM_PARAMETERS_INFO_ACTION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, WM_SETTINGCHANGE,
};

// The actions, by number (winuser.h), so the names do not depend on the bindings' spelling.
const SPI_GETFILTERKEYS: u32 = 0x0032;
const SPI_SETFILTERKEYS: u32 = 0x0033;
const SPI_GETTOGGLEKEYS: u32 = 0x0034;
const SPI_SETTOGGLEKEYS: u32 = 0x0035;
const SPI_GETMOUSEKEYS: u32 = 0x0036;
const SPI_SETMOUSEKEYS: u32 = 0x0037;
const SPI_GETSTICKYKEYS: u32 = 0x003A;
const SPI_SETSTICKYKEYS: u32 = 0x003B;
const SPI_GETSOUNDSENTRY: u32 = 0x0040;
const SPI_SETSOUNDSENTRY: u32 = 0x0041;
const SPI_GETHIGHCONTRAST: u32 = 0x0042;
const SPI_GETKEYBOARDCUES: u32 = 0x100A;
const SPI_SETKEYBOARDCUES: u32 = 0x100B;
const SPI_GETCLIENTAREAANIMATION: u32 = 0x1042;
const SPI_SETCLIENTAREAANIMATION: u32 = 0x1043;
const SPI_GETCARETWIDTH: u32 = 0x2006;
const SPI_SETCARETWIDTH: u32 = 0x2007;
const SPI_GETMESSAGEDURATION: u32 = 0x2016;
const SPI_SETMESSAGEDURATION: u32 = 0x2017;
const SPI_GETCONTACTVISUALIZATION: u32 = 0x2018;
const SPI_SETCONTACTVISUALIZATION: u32 = 0x2019;

/// The "on" bit of sticky, filter, toggle and mouse keys (`SKF_STICKYKEYSON`, `FKF_FILTERKEYSON`,
/// `TKF_TOGGLEKEYSON`, `MKF_MOUSEKEYSON` are all 1).
pub const ON: u32 = 0x1;
/// The bit that lets the keyboard shortcut turn the feature on (`*_HOTKEYACTIVE`, all 4).
pub const HOTKEY: u32 = 0x4;
/// `HCF_HIGHCONTRASTON`.
const HCF_ON: u32 = 0x1;
/// `SSF_SOUNDSENTRYON`.
const SSF_ON: u32 = 0x1;

/// The text cursor thickness Windows' Settings offers, in pixels.
pub const CARET_RANGE: (u32, u32) = (1, 20);
/// "Dismiss notifications after this amount of time", in seconds, as Windows' Settings lists it.
pub const MESSAGE_SECONDS: [u32; 6] = [5, 7, 15, 30, 60, 300];

/// Where the text size and the pointer live (read only: Windows' Settings applies them with
/// calls of its own).
const ACCESSIBILITY: &str = r"Software\Microsoft\Accessibility";
const PERSONALIZE: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";
const CP_ACCESSIBILITY: &str = r"Control Panel\Accessibility";
const CURSORS: &str = r"Control Panel\Cursors";
const COLOR_FILTERING: &str = r"Software\Microsoft\ColorFiltering";
const AUDIO: &str = r"Software\Microsoft\Multimedia\Audio";

/// The touch indicator (`SPI_*CONTACTVISUALIZATION`): off, on, or darker and larger.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Touch {
    Off,
    On,
    Larger,
}

impl Touch {
    /// `CONTACTVISUALIZATION_OFF / _ON / _PRESENTATIONMODE`.
    pub fn from_raw(v: u32) -> Touch {
        match v {
            0 => Touch::Off,
            2 => Touch::Larger,
            _ => Touch::On,
        }
    }

    pub fn raw(self) -> u32 {
        match self {
            Touch::Off => 0,
            Touch::On => 1,
            Touch::Larger => 2,
        }
    }
}

/// "Flash my screen during audio notifications": what sound sentry flashes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Flash {
    Off,
    /// The active window's title bar (`SSWF_TITLE`).
    Title,
    /// The active window (`SSWF_WINDOW`).
    Window,
    /// The whole screen (`SSWF_DISPLAY`).
    Screen,
}

impl Flash {
    pub const ALL: [Flash; 4] = [Flash::Off, Flash::Title, Flash::Window, Flash::Screen];

    pub fn label(self) -> &'static str {
        match self {
            Flash::Off => "Off",
            Flash::Title => "Title bar",
            Flash::Window => "Active window",
            Flash::Screen => "Whole screen",
        }
    }

    /// What sound sentry's flags and window effect say.
    pub fn from_raw(flags: u32, effect: u32) -> Flash {
        if flags & SSF_ON == 0 {
            return Flash::Off;
        }
        match effect {
            1 => Flash::Title,
            2 => Flash::Window,
            3 => Flash::Screen,
            _ => Flash::Off,
        }
    }

    /// The flags and window effect to write, keeping the flags' other bits. Off clears only the
    /// on bit and keeps the effect, as Windows does.
    pub fn to_raw(self, flags: u32, effect: u32) -> (u32, u32) {
        match self {
            Flash::Off => (flags & !SSF_ON, effect),
            Flash::Title => (flags | SSF_ON, 1),
            Flash::Window => (flags | SSF_ON, 2),
            Flash::Screen => (flags | SSF_ON, 3),
        }
    }
}

/// A feature with an on bit and a shortcut bit: sticky, filter, toggle or mouse keys.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Keys {
    Sticky,
    Filter,
    Toggle,
    Mouse,
}

impl Keys {
    fn actions(self) -> (u32, u32) {
        match self {
            Keys::Sticky => (SPI_GETSTICKYKEYS, SPI_SETSTICKYKEYS),
            Keys::Filter => (SPI_GETFILTERKEYS, SPI_SETFILTERKEYS),
            Keys::Toggle => (SPI_GETTOGGLEKEYS, SPI_SETTOGGLEKEYS),
            Keys::Mouse => (SPI_GETMOUSEKEYS, SPI_SETMOUSEKEYS),
        }
    }
}

/// Windows' own accessibility programs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Program {
    Narrator,
    Magnifier,
    Keyboard,
    LiveCaptions,
}

impl Program {
    /// The exe in System32.
    pub fn exe(self) -> &'static str {
        match self {
            Program::Narrator => "Narrator.exe",
            Program::Magnifier => "Magnify.exe",
            Program::Keyboard => "osk.exe",
            Program::LiveCaptions => "LiveCaptions.exe",
        }
    }
}

/// Everything the page shows, as Windows holds it. `None` where Windows did not answer.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Access {
    pub animations: Option<bool>,
    pub transparency: bool,
    pub caret_width: Option<u32>,
    pub message_seconds: Option<u32>,
    pub touch: Option<u32>,
    pub keyboard_cues: Option<bool>,
    /// The raw flags of sticky, filter, toggle and mouse keys.
    pub sticky: Option<u32>,
    pub filter: Option<u32>,
    pub toggle: Option<u32>,
    pub mouse_keys: Option<u32>,
    /// Sound sentry's flags and window effect.
    pub sound_sentry: Option<(u32, u32)>,
    // Read only.
    pub high_contrast: Option<HighContrast>,
    pub look: Look,
    /// Which programs run now.
    pub narrator: bool,
    pub magnifier: bool,
    pub keyboard: bool,
    pub live_captions: bool,
    /// Whether Live captions is on this machine (Windows 11 22H2 and later).
    pub has_live_captions: bool,
}

/// The contrast theme: whether one is on, its name, and whether its shortcut works.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct HighContrast {
    pub on: bool,
    pub hotkey: bool,
    pub theme: String,
}

/// The values the page shows but leaves to Windows' Settings, straight from the registry.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Look {
    /// `TextScaleFactor`, 100 to 225; 100 when missing.
    pub text_scale: u32,
    /// "Always show scroll bars" (`DynamicScrollbars` = 0).
    pub scrollbars_always: bool,
    /// The pointer's size step, 1 to 15, from `CursorBaseSize` (32 + 16 a step).
    pub pointer_size: u32,
    /// `CursorType`: 0 white, 1 black, 2 inverted, 6 custom colour.
    pub pointer_style: u32,
    pub color_filter: bool,
    /// `FilterType`: 0 greyscale, 1 inverted, 2 greyscale inverted, 3 red-green (green weak),
    /// 4 red-green (red weak), 5 blue-yellow.
    pub color_filter_kind: u32,
    pub color_filter_hotkey: bool,
    pub mono_audio: bool,
}

/// One change to make.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Change {
    Animations(bool),
    Transparency(bool),
    CaretWidth(u32),
    MessageSeconds(u32),
    Touch(Touch),
    KeyboardCues(bool),
    /// Turn a feature on or off.
    KeysOn(Keys, bool),
    /// Let its shortcut turn it on, or not.
    KeysHotkey(Keys, bool),
    Flash(Flash),
}

/// `flags` with `bit` set or cleared, every other bit kept.
pub fn with_bit(flags: u32, bit: u32, on: bool) -> u32 {
    if on {
        flags | bit
    } else {
        flags & !bit
    }
}

/// Whether `bit` is set in `flags` (`false` when Windows did not answer).
pub fn has(flags: Option<u32>, bit: u32) -> bool {
    flags.is_some_and(|f| f & bit != 0)
}

/// The pointer's size step from `CursorBaseSize`: 32 pixels is 1, each step 16 more, up to 15.
pub fn pointer_step(base: u32) -> u32 {
    (base.saturating_sub(32) / 16 + 1).clamp(1, 15)
}

pub fn pointer_style_label(v: u32) -> &'static str {
    match v {
        0 => "White",
        1 => "Black",
        2 => "Inverted",
        6 => "Custom colour",
        _ => "Custom",
    }
}

pub fn filter_label(v: u32) -> &'static str {
    match v {
        0 => "Greyscale",
        1 => "Inverted",
        2 => "Greyscale inverted",
        3 => "Red-green (green weak)",
        4 => "Red-green (red weak)",
        5 => "Blue-yellow",
        _ => "A colour filter",
    }
}

/// "5 seconds", "1 minute", "5 minutes".
pub fn seconds_label(s: u32) -> String {
    match s {
        60 => "1 minute".to_owned(),
        s if s % 60 == 0 => format!("{} minutes", s / 60),
        s => format!("{s} seconds"),
    }
}

/// The value Windows' list holds nearest `s` (a value set elsewhere may be between two).
pub fn nearest_seconds(s: u32) -> u32 {
    *MESSAGE_SECONDS.iter().min_by_key(|v| v.abs_diff(s)).unwrap_or(&5)
}

fn dword(reg: &dyn Registry, key: &str, name: &str) -> Option<u32> {
    match reg.get(Hive::Hkcu, key, name)? {
        Value::Dword(v) => Some(v),
        Value::Text(t) | Value::Expand(t) => t.trim().parse().ok(),
        Value::Binary(_) => None,
    }
}

/// The values of [`Look`] from `reg`, Windows' defaults where one is missing.
pub fn read_look(reg: &dyn Registry) -> Look {
    let filter = |name| dword(reg, COLOR_FILTERING, name).unwrap_or(0);
    Look {
        text_scale: dword(reg, ACCESSIBILITY, "TextScaleFactor").unwrap_or(100).clamp(100, 225),
        scrollbars_always: dword(reg, CP_ACCESSIBILITY, "DynamicScrollbars") == Some(0),
        pointer_size: pointer_step(dword(reg, CURSORS, "CursorBaseSize").unwrap_or(32)),
        pointer_style: dword(reg, ACCESSIBILITY, "CursorType").unwrap_or(0),
        color_filter: filter("Active") != 0,
        color_filter_kind: filter("FilterType"),
        color_filter_hotkey: filter("HotkeyEnabled") != 0,
        mono_audio: dword(reg, AUDIO, "AccessibilityMonoMixState").unwrap_or(0) != 0,
    }
}

/// Whether transparency effects are on: missing is on (Windows' default).
pub fn read_transparency(reg: &dyn Registry) -> bool {
    dword(reg, PERSONALIZE, "EnableTransparency") != Some(0)
}

fn flags() -> SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS {
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(SPIF_UPDATEINIFILE.0 | SPIF_SENDCHANGE.0)
}

/// A `SystemParametersInfoW` that reads into `out`.
fn get<T>(action: u32, out: &mut T, size: u32) -> bool {
    // SAFETY: `out` is the struct or integer this action writes, and outlives the call.
    unsafe { SystemParametersInfoW(SYSTEM_PARAMETERS_INFO_ACTION(action), size, Some(out as *mut T as *mut _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0)) }.is_ok()
}

/// [`get`] of a struct, with its size in `uiParam`.
fn get_struct<T>(action: u32, out: &mut T) -> bool {
    get(action, out, size_of::<T>() as u32)
}

fn get_u32(action: u32) -> Option<u32> {
    let mut v = 0u32;
    get(action, &mut v, 0).then_some(v)
}

fn get_bool(action: u32) -> Option<bool> {
    let mut v = windows::core::BOOL(0);
    get(action, &mut v, 0).then_some(v.as_bool())
}

/// A write whose value travels in the pointer argument itself.
fn set_value(action: u32, value: usize) -> bool {
    // SAFETY: no memory is passed; the value is the pointer.
    unsafe { SystemParametersInfoW(SYSTEM_PARAMETERS_INFO_ACTION(action), 0, Some(value as *mut _), flags()) }.is_ok()
}

/// A write of a struct the action takes by pointer, with its size in `uiParam`.
fn set_struct<T>(action: u32, v: &mut T) -> bool {
    let size = size_of::<T>() as u32;
    // SAFETY: `v` is the struct this action reads, and outlives the call.
    unsafe { SystemParametersInfoW(SYSTEM_PARAMETERS_INFO_ACTION(action), size, Some(v as *mut T as *mut _), flags()) }.is_ok()
}

fn read_keys(which: Keys) -> Option<u32> {
    let (action, _) = which.actions();
    match which {
        Keys::Sticky => {
            let mut s = STICKYKEYS { cbSize: size_of::<STICKYKEYS>() as u32, ..Default::default() };
            get_struct(action, &mut s).then_some(s.dwFlags.0)
        }
        Keys::Filter => {
            let mut s = FILTERKEYS { cbSize: size_of::<FILTERKEYS>() as u32, ..Default::default() };
            get_struct(action, &mut s).then_some(s.dwFlags)
        }
        Keys::Toggle => {
            let mut s = TOGGLEKEYS { cbSize: size_of::<TOGGLEKEYS>() as u32, ..Default::default() };
            get_struct(action, &mut s).then_some(s.dwFlags)
        }
        Keys::Mouse => {
            let mut s = MOUSEKEYS { cbSize: size_of::<MOUSEKEYS>() as u32, ..Default::default() };
            get_struct(action, &mut s).then_some(s.dwFlags)
        }
    }
}

/// Read the feature's struct, set or clear `bit`, and write the struct back.
fn write_keys(which: Keys, bit: u32, on: bool) -> bool {
    let (get_a, set_a) = which.actions();
    match which {
        Keys::Sticky => {
            let mut s = STICKYKEYS { cbSize: size_of::<STICKYKEYS>() as u32, ..Default::default() };
            get_struct(get_a, &mut s) && {
                s.dwFlags.0 = with_bit(s.dwFlags.0, bit, on);
                set_struct(set_a, &mut s)
            }
        }
        Keys::Filter => {
            let mut s = FILTERKEYS { cbSize: size_of::<FILTERKEYS>() as u32, ..Default::default() };
            get_struct(get_a, &mut s) && {
                s.dwFlags = with_bit(s.dwFlags, bit, on);
                set_struct(set_a, &mut s)
            }
        }
        Keys::Toggle => {
            let mut s = TOGGLEKEYS { cbSize: size_of::<TOGGLEKEYS>() as u32, ..Default::default() };
            get_struct(get_a, &mut s) && {
                s.dwFlags = with_bit(s.dwFlags, bit, on);
                set_struct(set_a, &mut s)
            }
        }
        Keys::Mouse => {
            let mut s = MOUSEKEYS { cbSize: size_of::<MOUSEKEYS>() as u32, ..Default::default() };
            get_struct(get_a, &mut s) && {
                s.dwFlags = with_bit(s.dwFlags, bit, on);
                set_struct(set_a, &mut s)
            }
        }
    }
}

fn read_sound_sentry() -> Option<(u32, u32)> {
    let mut s = SOUNDSENTRYW { cbSize: size_of::<SOUNDSENTRYW>() as u32, ..Default::default() };
    get_struct(SPI_GETSOUNDSENTRY, &mut s).then_some((s.dwFlags.0, s.iWindowsEffect.0))
}

fn write_flash(flash: Flash) -> bool {
    let mut s = SOUNDSENTRYW { cbSize: size_of::<SOUNDSENTRYW>() as u32, ..Default::default() };
    if !get_struct(SPI_GETSOUNDSENTRY, &mut s) {
        return false;
    }
    let (f, effect) = flash.to_raw(s.dwFlags.0, s.iWindowsEffect.0);
    s.dwFlags.0 = f;
    s.iWindowsEffect.0 = effect;
    // Reserved, must be null (the read may hand back a pointer into Windows' own memory).
    s.lpszWindowsEffectDLL = windows::core::PWSTR::null();
    set_struct(SPI_SETSOUNDSENTRY, &mut s)
}

fn read_high_contrast() -> Option<HighContrast> {
    let mut h = HIGHCONTRASTW { cbSize: size_of::<HIGHCONTRASTW>() as u32, ..Default::default() };
    if !get_struct(SPI_GETHIGHCONTRAST, &mut h) {
        return None;
    }
    // SAFETY: Windows points the scheme name at a string of its own, or leaves it null.
    let theme = if h.lpszDefaultScheme.is_null() { String::new() } else { unsafe { h.lpszDefaultScheme.to_string() }.unwrap_or_default() };
    Some(HighContrast { on: h.dwFlags.0 & HCF_ON != 0, hotkey: h.dwFlags.0 & HOTKEY != 0, theme })
}

/// The lower-case exe names of this user's processes, to tell which programs run.
fn running() -> Vec<String> {
    let mut out = Vec::new();
    // SAFETY: a snapshot walk with a correctly sized entry; the handle is closed after.
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
        let mut entry = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut ok = Process32FirstW(snap, &mut entry).is_ok();
        while ok {
            let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
            out.push(String::from_utf16_lossy(&entry.szExeFile[..len]).to_lowercase());
            ok = Process32NextW(snap, &mut entry).is_ok();
        }
        let _ = CloseHandle(snap);
    }
    out
}

fn system32(exe: &str) -> std::path::PathBuf {
    let windir = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    std::path::Path::new(&windir).join("System32").join(exe)
}

/// Read everything from Windows now.
pub fn read() -> Access {
    let procs = running();
    let runs = |p: Program| procs.iter().any(|n| n.eq_ignore_ascii_case(p.exe()));
    Access {
        animations: get_bool(SPI_GETCLIENTAREAANIMATION),
        transparency: read_transparency(&Real),
        caret_width: get_u32(SPI_GETCARETWIDTH),
        message_seconds: get_u32(SPI_GETMESSAGEDURATION),
        touch: get_u32(SPI_GETCONTACTVISUALIZATION),
        keyboard_cues: get_bool(SPI_GETKEYBOARDCUES),
        sticky: read_keys(Keys::Sticky),
        filter: read_keys(Keys::Filter),
        toggle: read_keys(Keys::Toggle),
        mouse_keys: read_keys(Keys::Mouse),
        sound_sentry: read_sound_sentry(),
        high_contrast: read_high_contrast(),
        look: read_look(&Real),
        narrator: runs(Program::Narrator),
        magnifier: runs(Program::Magnifier),
        keyboard: runs(Program::Keyboard),
        live_captions: runs(Program::LiveCaptions),
        has_live_captions: system32(Program::LiveCaptions.exe()).exists(),
    }
}

/// Tell every window a setting changed (`WM_SETTINGCHANGE` with `area`), as Windows' Settings
/// does after writing a value of its own.
fn broadcast(area: &str) {
    let area = HSTRING::from(area);
    // SAFETY: the string outlives the call, which waits for the answers (or a second each).
    unsafe {
        let _ = SendMessageTimeoutW(HWND_BROADCAST, WM_SETTINGCHANGE, WPARAM(0), LPARAM(area.as_ptr() as isize), SMTO_ABORTIFHUNG, 1000, None);
    }
}

/// Make one change. True when Windows took it.
pub fn apply(change: Change) -> bool {
    match change {
        Change::Animations(on) => set_value(SPI_SETCLIENTAREAANIMATION, on as usize),
        Change::Transparency(on) => {
            let ok = Real.set_dword(Hive::Hkcu, PERSONALIZE, "EnableTransparency", on as u32).is_ok();
            broadcast("ImmersiveColorSet");
            ok
        }
        Change::CaretWidth(w) => set_value(SPI_SETCARETWIDTH, w.clamp(CARET_RANGE.0, CARET_RANGE.1) as usize),
        Change::MessageSeconds(s) => set_value(SPI_SETMESSAGEDURATION, s.max(1) as usize),
        Change::Touch(t) => set_value(SPI_SETCONTACTVISUALIZATION, t.raw() as usize),
        Change::KeyboardCues(on) => set_value(SPI_SETKEYBOARDCUES, on as usize),
        Change::KeysOn(k, on) => write_keys(k, ON, on),
        Change::KeysHotkey(k, on) => write_keys(k, HOTKEY, on),
        Change::Flash(f) => write_flash(f),
    }
}

/// Start one of Windows' accessibility programs from System32. True when it started. Through
/// the shell: Narrator, Magnifier and the on-screen keyboard ask for UI access in their manifests,
/// which `CreateProcess` refuses (`ERROR_ELEVATION_REQUIRED`) and ShellExecute grants.
pub fn start(program: Program) -> bool {
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let path = HSTRING::from(system32(program.exe()).as_os_str());
    // SAFETY: the strings outlive the call.
    let r = unsafe { ShellExecuteW(None, &HSTRING::from("open"), &path, None, None, SW_SHOWNORMAL) };
    // Success is a value above 32.
    r.0 as isize > 32
}

/// [`Access`] with `change` made, as the page shows it before Windows has answered.
pub fn expect(mut a: Access, change: Change) -> Access {
    match change {
        Change::Animations(on) => a.animations = Some(on),
        Change::Transparency(on) => a.transparency = on,
        Change::CaretWidth(w) => a.caret_width = Some(w),
        Change::MessageSeconds(s) => a.message_seconds = Some(s),
        Change::Touch(t) => a.touch = Some(t.raw()),
        Change::KeyboardCues(on) => a.keyboard_cues = Some(on),
        Change::KeysOn(k, on) => {
            let slot = a.keys_mut(k);
            *slot = slot.map(|f| with_bit(f, ON, on));
        }
        Change::KeysHotkey(k, on) => {
            let slot = a.keys_mut(k);
            *slot = slot.map(|f| with_bit(f, HOTKEY, on));
        }
        Change::Flash(f) => {
            let (flags, effect) = a.sound_sentry.unwrap_or((0, 0));
            a.sound_sentry = Some(f.to_raw(flags, effect));
        }
    }
    a
}

impl Access {
    /// The raw flags of one keyboard feature.
    pub fn keys(&self, k: Keys) -> Option<u32> {
        match k {
            Keys::Sticky => self.sticky,
            Keys::Filter => self.filter,
            Keys::Toggle => self.toggle,
            Keys::Mouse => self.mouse_keys,
        }
    }

    fn keys_mut(&mut self, k: Keys) -> &mut Option<u32> {
        match k {
            Keys::Sticky => &mut self.sticky,
            Keys::Filter => &mut self.filter,
            Keys::Toggle => &mut self.toggle,
            Keys::Mouse => &mut self.mouse_keys,
        }
    }

    /// The sound sentry choice.
    pub fn flash(&self) -> Flash {
        self.sound_sentry.map_or(Flash::Off, |(f, e)| Flash::from_raw(f, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::fake::Fake;

    #[test]
    fn a_bit_flip_keeps_every_other_bit() {
        // 510 is Windows' default sticky keys flags (available, hotkey, sounds...).
        assert_eq!(with_bit(510, ON, true), 511);
        assert_eq!(with_bit(511, ON, false), 510);
        assert_eq!(with_bit(510, HOTKEY, false), 506);
        assert_eq!(with_bit(506, HOTKEY, true), 510);
        assert!(has(Some(510), HOTKEY));
        assert!(!has(Some(510), ON));
        assert!(!has(None, ON));
    }

    #[test]
    fn flash_maps_to_sound_sentry() {
        assert_eq!(Flash::from_raw(0, 3), Flash::Off, "the on bit decides");
        assert_eq!(Flash::from_raw(1, 1), Flash::Title);
        assert_eq!(Flash::from_raw(1, 2), Flash::Window);
        assert_eq!(Flash::from_raw(1, 3), Flash::Screen);
        assert_eq!(Flash::from_raw(1, 4), Flash::Off, "a custom effect is not one of the choices");
        assert_eq!(Flash::Screen.to_raw(0x10, 0), (0x11, 3));
        assert_eq!(Flash::Off.to_raw(0x11, 3), (0x10, 3), "off keeps the effect");
        for f in Flash::ALL {
            let (flags, effect) = f.to_raw(0, 0);
            assert_eq!(Flash::from_raw(flags, effect), f);
        }
    }

    #[test]
    fn touch_round_trips() {
        for t in [Touch::Off, Touch::On, Touch::Larger] {
            assert_eq!(Touch::from_raw(t.raw()), t);
        }
    }

    #[test]
    fn pointer_steps_are_sixteen_pixels() {
        assert_eq!(pointer_step(32), 1);
        assert_eq!(pointer_step(48), 2);
        assert_eq!(pointer_step(256), 15);
        assert_eq!(pointer_step(0), 1);
        assert_eq!(pointer_step(999), 15);
    }

    #[test]
    fn seconds() {
        assert_eq!(seconds_label(5), "5 seconds");
        assert_eq!(seconds_label(60), "1 minute");
        assert_eq!(seconds_label(300), "5 minutes");
        assert_eq!(nearest_seconds(6), 5);
        assert_eq!(nearest_seconds(20), 15);
        assert_eq!(nearest_seconds(1000), 300);
    }

    #[test]
    fn missing_values_are_windows_defaults() {
        let reg = Fake::default();
        let look = read_look(&reg);
        assert_eq!(look.text_scale, 100);
        assert!(!look.scrollbars_always, "scroll bars hide by default");
        assert_eq!(look.pointer_size, 1);
        assert!(!look.color_filter && !look.mono_audio);
        assert!(read_transparency(&reg), "transparency is on by default");
    }

    #[test]
    fn values_are_read_whatever_their_type() {
        let reg = Fake::default();
        reg.put(Hive::Hkcu, ACCESSIBILITY, "TextScaleFactor", Value::Dword(150));
        reg.put(Hive::Hkcu, CP_ACCESSIBILITY, "DynamicScrollbars", Value::Dword(0));
        reg.put(Hive::Hkcu, CURSORS, "CursorBaseSize", Value::Dword(64));
        reg.put(Hive::Hkcu, COLOR_FILTERING, "Active", Value::Dword(1));
        reg.put(Hive::Hkcu, COLOR_FILTERING, "FilterType", Value::Dword(5));
        reg.put(Hive::Hkcu, AUDIO, "AccessibilityMonoMixState", Value::Text("1".into()));
        reg.put(Hive::Hkcu, PERSONALIZE, "EnableTransparency", Value::Dword(0));
        let look = read_look(&reg);
        assert_eq!(look.text_scale, 150);
        assert!(look.scrollbars_always);
        assert_eq!(look.pointer_size, 3);
        assert!(look.color_filter && look.mono_audio);
        assert_eq!(filter_label(look.color_filter_kind), "Blue-yellow");
        assert!(!read_transparency(&reg));
    }

    #[test]
    fn the_page_shows_a_change_before_windows_answers() {
        let a = Access { sticky: Some(510), sound_sentry: Some((0, 0)), ..Default::default() };
        let b = expect(a.clone(), Change::KeysOn(Keys::Sticky, true));
        assert_eq!(b.sticky, Some(511));
        let c = expect(b, Change::KeysHotkey(Keys::Sticky, false));
        assert_eq!(c.sticky, Some(507));
        assert_eq!(expect(a.clone(), Change::KeysOn(Keys::Filter, true)).filter, None, "unknown stays unknown");
        assert_eq!(expect(a.clone(), Change::Flash(Flash::Window)).sound_sentry, Some((1, 2)));
        assert_eq!(expect(a, Change::Touch(Touch::Larger)).touch, Some(2));
    }

    /// Turns transparency effects off and on again the way the page does, and reads DWM's
    /// transparency state (which shellman-session publishes when it hears the broadcast) after
    /// each. Run by hand outside any packaged app's registry view, with shellman as the shell.
    #[test]
    #[ignore]
    fn transparency_reaches_dwm() {
        use crate::system::transparency;
        let orig = read_transparency(&Real);
        let at = |label: &str| {
            std::thread::sleep(std::time::Duration::from_secs(2));
            let r = transparency::read();
            println!("{label}: setting {} state {r:?}", read_transparency(&Real));
            r.ok().and_then(|r| r.allowed)
        };
        let start = at("start");
        apply(Change::Transparency(!orig));
        let flipped = at("flipped");
        apply(Change::Transparency(orig));
        let back = at("back");
        assert_eq!(flipped, Some(!orig));
        assert_eq!(back, Some(orig));
        assert_eq!(back, start);
    }

    /// Changes every SPI value, reads it back, and puts the original back. Touches the real
    /// system settings, so it is run by hand, outside any packaged app's registry view:
    /// `cargo test -p shellman-core access -- --ignored`.
    #[test]
    #[ignore]
    fn every_setter_takes_and_restores() {
        let orig = read();
        let ks = [Keys::Sticky, Keys::Filter, Keys::Toggle, Keys::Mouse];
        let flags = |a: &Access, k: Keys| a.keys(k);
        let mut changes = vec![
            Change::Animations(!orig.animations.unwrap()),
            Change::CaretWidth(if orig.caret_width == Some(3) { 4 } else { 3 }),
            Change::MessageSeconds(if orig.message_seconds == Some(7) { 15 } else { 7 }),
            Change::Touch(if orig.touch == Some(2) { Touch::On } else { Touch::Larger }),
            Change::KeyboardCues(!orig.keyboard_cues.unwrap()),
            Change::Flash(if Flash::from_raw(orig.sound_sentry.unwrap().0, orig.sound_sentry.unwrap().1) == Flash::Title { Flash::Window } else { Flash::Title }),
            Change::Transparency(!orig.transparency),
        ];
        for k in ks {
            // Only the shortcut bit: turning mouse or filter keys on would change how the
            // keyboard behaves while the test runs.
            changes.push(Change::KeysHotkey(k, !has(flags(&orig, k), HOTKEY)));
        }
        let took: Vec<bool> = changes.iter().map(|c| apply(*c)).collect();
        let changed = read();
        let mut want = orig.clone();
        for c in &changes {
            want = expect(want, *c);
        }
        let restore = vec![
            Change::Animations(orig.animations.unwrap()),
            Change::CaretWidth(orig.caret_width.unwrap()),
            Change::MessageSeconds(orig.message_seconds.unwrap()),
            Change::Touch(Touch::from_raw(orig.touch.unwrap())),
            Change::KeyboardCues(orig.keyboard_cues.unwrap()),
            Change::Transparency(orig.transparency),
        ];
        let mut back: Vec<bool> = restore.into_iter().map(apply).collect();
        // Sound sentry: put the raw flags back exactly, not just the choice.
        back.push(write_flash(Flash::from_raw(orig.sound_sentry.unwrap().0, orig.sound_sentry.unwrap().1)));
        for k in ks {
            back.push(apply(Change::KeysHotkey(k, has(flags(&orig, k), HOTKEY))));
        }
        let restored = read();
        println!("orig {orig:#?}\nchanged {changed:#?}\ntook {took:?}\nback {back:?}");
        assert_eq!(changed.animations, want.animations);
        assert_eq!(changed.caret_width, want.caret_width);
        assert_eq!(changed.message_seconds, want.message_seconds);
        assert_eq!(changed.touch, want.touch);
        assert_eq!(changed.keyboard_cues, want.keyboard_cues);
        assert_eq!(changed.transparency, want.transparency);
        assert_eq!(Flash::from_raw(changed.sound_sentry.unwrap().0, changed.sound_sentry.unwrap().1), Flash::from_raw(want.sound_sentry.unwrap().0, want.sound_sentry.unwrap().1));
        for k in ks {
            assert_eq!(flags(&changed, k), flags(&want, k), "{k:?}");
        }
        assert_eq!(restored.animations, orig.animations);
        assert_eq!(restored.caret_width, orig.caret_width);
        assert_eq!(restored.message_seconds, orig.message_seconds);
        assert_eq!(restored.touch, orig.touch);
        assert_eq!(restored.keyboard_cues, orig.keyboard_cues);
        assert_eq!(restored.transparency, orig.transparency);
        assert_eq!(restored.sticky, orig.sticky);
        assert_eq!(restored.filter, orig.filter);
        assert_eq!(restored.toggle, orig.toggle);
        assert_eq!(restored.mouse_keys, orig.mouse_keys);
        assert_eq!(restored.sound_sentry, orig.sound_sentry);
    }
}
