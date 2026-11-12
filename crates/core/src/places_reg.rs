//! The Windows half of `places`: reading the registry (read only, never a write), resolving names
//! and known folders, loading an entry's icon, and noticing when the registrations change.

use crate::places::{self, ClassData, Host, Place, Snapshot, SyncRootData};
use egui::ColorImage;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HLOCAL, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ,
};
use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows::Win32::Security::{GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER};
use windows::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, COINIT_APARTMENTTHREADED};
use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegGetValueW, RegNotifyChangeKeyValue, RegOpenKeyExW, HKEY,
    HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_NOTIFY, KEY_READ, REG_NOTIFY_CHANGE_LAST_SET,
    REG_NOTIFY_CHANGE_NAME, RRF_NOEXPAND, RRF_RT_REG_DWORD, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ,
};
use windows::Win32::System::Threading::{
    CreateEventW, GetCurrentProcess, OpenProcessToken, ResetEvent, SetEvent,
    WaitForMultipleObjects, INFINITE,
};
use windows::Win32::UI::Shell::{SHGetKnownFolderPath, SHLoadIndirectString, KF_FLAG_DEFAULT};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyIcon, DrawIconEx, PrivateExtractIconsW, DI_NORMAL, HICON,
};

const EXPLORER: &str = r"Microsoft\Windows\CurrentVersion\Explorer";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

// ------------------------------------------------------------------ registry reading

/// A string value, unexpanded. `sub` is a path below `root`; `name` is empty for the default.
fn get_string(root: HKEY, sub: &str, name: &str) -> Option<String> {
    let (sub, name_w) = (wide(sub), wide(name));
    let name_p = if name.is_empty() {
        PCWSTR::null()
    } else {
        PCWSTR(name_w.as_ptr())
    };
    let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ | RRF_NOEXPAND;
    let mut bytes = 0u32;
    unsafe {
        RegGetValueW(root, PCWSTR(sub.as_ptr()), name_p, flags, None, None, Some(&mut bytes))
            .ok()
            .ok()?;
        let mut buf = vec![0u16; bytes as usize / 2 + 1];
        RegGetValueW(
            root,
            PCWSTR(sub.as_ptr()),
            name_p,
            flags,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut bytes),
        )
        .ok()
        .ok()?;
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        let s = String::from_utf16_lossy(&buf[..len]);
        (!s.trim().is_empty()).then_some(s)
    }
}

fn get_dword(root: HKEY, sub: &str, name: &str) -> Option<u32> {
    let (sub, name) = (wide(sub), wide(name));
    let mut v = 0u32;
    let mut bytes = 4u32;
    unsafe {
        RegGetValueW(
            root,
            PCWSTR(sub.as_ptr()),
            PCWSTR(name.as_ptr()),
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut v).cast()),
            Some(&mut bytes),
        )
        .ok()
        .ok()?;
    }
    Some(v)
}

/// The names of a key's subkeys; empty when the key is missing.
fn subkeys(root: HKEY, sub: &str) -> Vec<String> {
    let mut out = Vec::new();
    let sub = wide(sub);
    unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(root, PCWSTR(sub.as_ptr()), Some(0), KEY_READ, &mut key).is_err() {
            return out;
        }
        for i in 0.. {
            let mut name = [0u16; 256];
            let mut len = name.len() as u32;
            if RegEnumKeyExW(key, i, Some(PWSTR(name.as_mut_ptr())), &mut len, None, None, None, None)
                .is_err()
            {
                break;
            }
            out.push(String::from_utf16_lossy(&name[..len as usize]));
        }
        let _ = RegCloseKey(key);
    }
    out
}

fn read_class(root: HKEY, classes: &str, id: &str) -> Option<ClassData> {
    let base = format!(r"{classes}\CLSID\{id}");
    let bag = format!(r"{base}\Instance\InitPropertyBag");
    let class = ClassData {
        name: get_string(root, &base, ""),
        pinned: get_dword(root, &base, "System.IsPinnedToNamespaceTree"),
        order: get_dword(root, &base, "SortOrderIndex"),
        icon: get_string(root, &format!(r"{base}\DefaultIcon"), ""),
        target_path: get_string(root, &bag, "TargetFolderPath"),
        known_folder: get_string(root, &bag, "TargetKnownFolder"),
    };
    (class != ClassData::default()).then_some(class)
}

/// The current user's SID as text, which names the value under `UserSyncRoots`.
fn user_sid() -> Option<String> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).ok()?;
        let mut need = 0u32;
        let _ = GetTokenInformation(token, TokenUser, None, 0, &mut need);
        let mut buf = vec![0u8; need as usize];
        let got = GetTokenInformation(token, TokenUser, Some(buf.as_mut_ptr().cast()), need, &mut need);
        let _ = CloseHandle(token);
        got.ok()?;
        let user = &*(buf.as_ptr() as *const TOKEN_USER);
        let mut s = PWSTR::null();
        ConvertSidToStringSidW(user.User.Sid, &mut s).ok()?;
        let text = s.to_string().ok();
        windows::Win32::Foundation::LocalFree(Some(HLOCAL(s.0.cast())));
        text
    }
}

/// Everything the registrations say, read now. Quick: a few dozen small reads.
pub fn read_snapshot() -> Snapshot {
    let ns = format!(r"Software\{EXPLORER}\Desktop\NameSpace");
    let mut snap = Snapshot {
        hkcu_namespace: subkeys(HKEY_CURRENT_USER, &ns),
        hklm_namespace: subkeys(HKEY_LOCAL_MACHINE, &format!(r"SOFTWARE\{EXPLORER}\Desktop\NameSpace")),
        ..Default::default()
    };
    let ids: Vec<String> = snap
        .hkcu_namespace
        .iter()
        .chain(&snap.hklm_namespace)
        .filter(|id| id.starts_with('{'))
        .cloned()
        .collect();
    for id in ids {
        let k = id.to_ascii_uppercase();
        if let Some(c) = read_class(HKEY_CURRENT_USER, r"Software\Classes", &id) {
            snap.hkcu_classes.insert(k.clone(), c);
        }
        if let Some(c) = read_class(HKEY_LOCAL_MACHINE, r"SOFTWARE\Classes", &id) {
            snap.hklm_classes.insert(k, c);
        }
    }
    let sid = user_sid();
    let roots = format!(r"SOFTWARE\{EXPLORER}\SyncRootManager");
    for key in subkeys(HKEY_LOCAL_MACHINE, &roots) {
        let base = format!(r"{roots}\{key}");
        snap.sync_roots.push(SyncRootData {
            name: get_string(HKEY_LOCAL_MACHINE, &base, "DisplayNameResource"),
            icon: get_string(HKEY_LOCAL_MACHINE, &base, "IconResource"),
            path: sid
                .as_deref()
                .and_then(|s| get_string(HKEY_LOCAL_MACHINE, &format!(r"{base}\UserSyncRoots"), s)),
            key,
        });
    }
    snap
}

// ------------------------------------------------------------------ Windows services for `build`

pub struct WinHost;

fn parse_guid(s: &str) -> Option<windows::core::GUID> {
    let s = s.trim().trim_start_matches('{').trim_end_matches('}');
    let p: Vec<&str> = s.split('-').collect();
    if p.len() != 5 {
        return None;
    }
    let tail = format!("{}{}", p[3], p[4]);
    if tail.len() != 16 {
        return None;
    }
    let mut d4 = [0u8; 8];
    for (i, b) in d4.iter_mut().enumerate() {
        *b = u8::from_str_radix(&tail[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(windows::core::GUID::from_values(
        u32::from_str_radix(p[0], 16).ok()?,
        u16::from_str_radix(p[1], 16).ok()?,
        u16::from_str_radix(p[2], 16).ok()?,
        d4,
    ))
}

impl Host for WinHost {
    fn expand(&self, s: &str) -> String {
        use windows::Win32::System::Environment::ExpandEnvironmentStringsW;
        let src = wide(s);
        unsafe {
            let n = ExpandEnvironmentStringsW(PCWSTR(src.as_ptr()), None);
            if n == 0 {
                return s.to_string();
            }
            let mut buf = vec![0u16; n as usize];
            ExpandEnvironmentStringsW(PCWSTR(src.as_ptr()), Some(&mut buf));
            let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
            String::from_utf16_lossy(&buf[..len])
        }
    }

    fn known_folder(&self, id: &str) -> Option<PathBuf> {
        let guid = parse_guid(id)?;
        unsafe {
            let p = SHGetKnownFolderPath(&guid, KF_FLAG_DEFAULT, None).ok()?;
            let s = p.to_string().ok();
            CoTaskMemFree(Some(p.0.cast()));
            s.map(PathBuf::from)
        }
    }

    fn indirect(&self, s: &str) -> Option<String> {
        let src = wide(s);
        let mut out = [0u16; 512];
        unsafe { SHLoadIndirectString(PCWSTR(src.as_ptr()), &mut out, None) }.ok()?;
        let len = out.iter().position(|&c| c == 0).unwrap_or(out.len());
        Some(String::from_utf16_lossy(&out[..len]))
    }

    fn is_dir(&self, p: &Path) -> bool {
        p.is_dir()
    }
}

/// The places as of now. Touches folders (cloud ones can be slow), so not for the UI thread.
pub fn read_places() -> Vec<Place> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    places::build(&read_snapshot(), &WinHost)
}

// ------------------------------------------------------------------ icons

/// An entry's icon at `px` by `px` pixels, from `path,index` or `@dll,-id`. Windows picks the
/// closest size inside the file and scales from there, so a multi-size .ico stays crisp at 150%
/// and 300%.
pub fn load_icon(spec: &str, px: i32) -> Option<ColorImage> {
    let (path, index) = places::split_icon(spec);
    let path = path.trim_start_matches('@');
    let mut file = [0u16; 260];
    let w: Vec<u16> = path.encode_utf16().collect();
    if w.len() >= file.len() {
        return None;
    }
    file[..w.len()].copy_from_slice(&w);
    let mut icon = [HICON::default()];
    let n = unsafe { PrivateExtractIconsW(&file, index, px, px, Some(&mut icon), None, 0) };
    if n == 0 || icon[0].is_invalid() {
        return None;
    }
    let image = unsafe { draw_icon(icon[0], px) };
    unsafe {
        let _ = DestroyIcon(icon[0]);
    }
    image
}

/// Draw the icon over black and over white and work the alpha out from the difference. That
/// handles icons with a real alpha channel and old mask-only ones alike, which one DrawIconEx
/// onto a bitmap cannot.
unsafe fn draw_icon(icon: HICON, px: i32) -> Option<ColorImage> {
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: px,
            biHeight: -px,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let dc = CreateCompatibleDC(None);
    let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
    let bmp = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
    let old = SelectObject(dc, HGDIOBJ(bmp.0));
    let len = (px * px * 4) as usize;
    let pass = |fill: u8| -> Vec<u8> {
        std::slice::from_raw_parts_mut(bits.cast::<u8>(), len).fill(fill);
        let _ = DrawIconEx(dc, 0, 0, icon, px, px, 0, None, DI_NORMAL);
        std::slice::from_raw_parts(bits.cast::<u8>(), len).to_vec()
    };
    let on_black = pass(0);
    let on_white = pass(255);
    SelectObject(dc, old);
    let _ = DeleteObject(HGDIOBJ(bmp.0));
    let _ = DeleteDC(dc);
    let mut pixels = Vec::with_capacity((px * px) as usize);
    let (black, white) = (on_black.as_chunks::<4>().0, on_white.as_chunks::<4>().0);
    for (b, w) in black.iter().zip(white) {
        // White shows through by (1 - alpha); take the mean over the three channels.
        let gap = (u32::from(w[0].saturating_sub(b[0]))
            + u32::from(w[1].saturating_sub(b[1]))
            + u32::from(w[2].saturating_sub(b[2])))
            / 3;
        let a = 255u32.saturating_sub(gap) as u8;
        // `b` is the colour already multiplied by alpha (BGRA order).
        pixels.push(egui::Color32::from_rgba_premultiplied(b[2], b[1], b[0], a));
    }
    if pixels.iter().all(|p| p.a() == 0) {
        return None;
    }
    Some(ColorImage {
        size: [px as usize, px as usize],
        pixels,
    })
}

// ------------------------------------------------------------------ change notification

/// How long the registry must stay quiet before the places are read again: installers write
/// many values in a burst.
const QUIET_MS: u32 = 500;

/// Where the registrations live, as (root, path below it, whole subtree).
fn watch_targets() -> Vec<(HKEY, String, bool)> {
    vec![
        (HKEY_CURRENT_USER, format!(r"Software\{EXPLORER}\Desktop\NameSpace"), false),
        (HKEY_LOCAL_MACHINE, format!(r"SOFTWARE\{EXPLORER}\Desktop\NameSpace"), false),
        (HKEY_CURRENT_USER, r"Software\Classes\CLSID".to_string(), true),
        (HKEY_LOCAL_MACHINE, r"SOFTWARE\Classes\CLSID".to_string(), true),
        (HKEY_LOCAL_MACHINE, format!(r"SOFTWARE\{EXPLORER}\SyncRootManager"), true),
    ]
}

/// Open a key, or when it does not exist yet its nearest existing parent, which reports the
/// key being created.
fn open_or_parent(root: HKEY, path: &str, subtree: bool) -> Option<(HKEY, bool)> {
    let mut path = path.to_string();
    let mut sub = subtree;
    loop {
        let w = wide(&path);
        let mut key = HKEY::default();
        if unsafe { RegOpenKeyExW(root, PCWSTR(w.as_ptr()), Some(0), KEY_NOTIFY, &mut key) }.is_ok() {
            return Some((key, sub));
        }
        sub = false;
        match path.rsplit_once('\\') {
            Some((parent, _)) => path = parent.to_string(),
            None => return None,
        }
    }
}

struct Shared {
    latest: Mutex<Option<Vec<Place>>>,
    stop: AtomicBool,
}

/// Follows the registrations on a worker thread, with `RegNotifyChangeKeyValue` rather than
/// polling. `poll` hands over the new list whenever it has changed.
pub struct PlacesWatcher {
    shared: Arc<Shared>,
    stop_event: isize,
    thread: Option<JoinHandle<()>>,
}

impl PlacesWatcher {
    pub fn new(ctx: egui::Context) -> Self {
        let shared = Arc::new(Shared {
            latest: Mutex::new(None),
            stop: AtomicBool::new(false),
        });
        let stop_event = unsafe { CreateEventW(None, true, false, None) }.map_or(0, |h| h.0 as isize);
        let thread = (stop_event != 0).then(|| {
            let shared = shared.clone();
            std::thread::spawn(move || run(&shared, stop_event, &ctx))
        });
        Self { shared, stop_event, thread }
    }

    /// The new list, once, after the first read and after each change that altered it.
    pub fn poll(&self) -> Option<Vec<Place>> {
        self.shared.latest.lock().unwrap().take()
    }
}

impl Drop for PlacesWatcher {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        let h = HANDLE(self.stop_event as *mut _);
        if let Some(t) = self.thread.take() {
            unsafe {
                let _ = SetEvent(h);
            }
            let _ = t.join();
        }
        if self.stop_event != 0 {
            unsafe {
                let _ = CloseHandle(h);
            }
        }
    }
}

fn run(shared: &Shared, stop_event: isize, ctx: &egui::Context) {
    let stop = HANDLE(stop_event as *mut _);
    let events: Vec<HANDLE> = watch_targets()
        .iter()
        .filter_map(|_| unsafe { CreateEventW(None, true, false, None) }.ok())
        .collect();
    let mut last: Option<Vec<Place>> = None;
    // Armed before the first read, so a change during it is not missed.
    let mut keys = arm(&events);
    'outer: loop {
        let places = read_places();
        if last.as_ref() != Some(&places) {
            last = Some(places.clone());
            *shared.latest.lock().unwrap() = Some(places);
            ctx.request_repaint();
        }
        // Wait for a change, then until the registry has been quiet for a moment.
        let mut handles = vec![stop];
        handles.extend(&events);
        let mut timeout = INFINITE;
        loop {
            if shared.stop.load(Ordering::SeqCst) {
                break 'outer;
            }
            let r = unsafe { WaitForMultipleObjects(&handles, false, timeout) };
            if r == WAIT_TIMEOUT {
                break;
            }
            if r == WAIT_OBJECT_0 {
                break 'outer;
            }
            if r.0 > WAIT_OBJECT_0.0 && ((r.0 - WAIT_OBJECT_0.0) as usize) < handles.len() {
                timeout = QUIET_MS;
                // Re-arm: it also picks up a key created since, and resets the events.
                close(std::mem::take(&mut keys));
                keys = arm(&events);
            } else {
                // Wait failed: do not spin.
                std::thread::sleep(Duration::from_millis(500));
                break;
            }
        }
    }
    close(keys);
    for e in events {
        unsafe {
            let _ = CloseHandle(e);
        }
    }
}

fn close(keys: Vec<HKEY>) {
    for k in keys {
        unsafe {
            let _ = RegCloseKey(k);
        }
    }
}

fn arm(events: &[HANDLE]) -> Vec<HKEY> {
    let mut keys = Vec::new();
    for ((root, path, subtree), ev) in watch_targets().into_iter().zip(events) {
        let Some((key, sub)) = open_or_parent(root, &path, subtree) else {
            continue;
        };
        unsafe {
            let _ = ResetEvent(*ev);
            let filter = REG_NOTIFY_CHANGE_NAME | REG_NOTIFY_CHANGE_LAST_SET;
            if RegNotifyChangeKeyValue(key, sub, filter, Some(*ev), true).is_ok() {
                keys.push(key);
            } else {
                let _ = RegCloseKey(key);
            }
        }
    }
    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_guids() {
        let g = parse_guid("{F49A41F9-A7D1-42E9-8F02-CC3BD681ED4D}").unwrap();
        assert_eq!(g.data1, 0xF49A41F9);
        assert_eq!(g.data4[0], 0x8F);
        assert_eq!(g.data4[7], 0x4D);
        assert!(parse_guid("nope").is_none());
    }

    /// Reads the real registry (read only); whatever is registered here must not break it.
    #[test]
    fn reads_this_machine_and_loads_a_system_icon() {
        for p in read_places() {
            println!("{p:?}");
        }
        let img = load_icon(r"C:\Windows\System32\imageres.dll,-3", 48).expect("imageres icon");
        assert_eq!(img.size, [48, 48]);
        assert!(img.pixels.iter().any(|p| p.a() > 0));
        assert!(img.pixels.iter().any(|p| p.a() == 0));
    }
}
