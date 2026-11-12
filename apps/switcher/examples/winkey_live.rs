//! Live check of the Win key logic (`src/winkey.rs`) without the switcher: its own low-level hook
//! runs the same `on_key`, and keys are sent with SendInput. The summon only counts, so no
//! launcher opens. For each key sequence it lists the windows that appeared, then closes them
//! with Esc (sent while this tool's own window has the focus otherwise).
//!
//! `winkey_live hook tap R V S A N Tab ShiftS` (or `baseline ...` for no hook, to compare;
//! `hook-real ...` summons the real launcher).
//! Every key it presses is released again on exit, panics included.

#[path = "../src/winkey.rs"]
#[allow(dead_code)]
mod winkey;

use std::collections::HashSet;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};
use windows::core::{w, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP,
    VIRTUAL_KEY, VK_ESCAPE, VK_LSHIFT, VK_LWIN, VK_RWIN, VK_TAB,
};
use windows::Win32::UI::WindowsAndMessaging::*;

static SUMMONS: AtomicU32 = AtomicU32::new(0);
const TEST: usize = 0x7E57; // any marker but shellman's: the hook treats these keys as real

/// No launcher next to this example: a tap is ours whenever Explorer is not the shell.
fn take() -> bool {
    !shellman_core::app_host::explorer_is_shell()
}

/// `hook-real`: summon the real launcher, as the switcher does.
static REAL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn real() {
    note();
    winkey::summon();
}

fn note() {
    SUMMONS.fetch_add(1, Ordering::SeqCst);
}

unsafe extern "system" fn on_key(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == 0 {
        let info = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        let summon: fn() = if REAL.load(Ordering::SeqCst) { real } else { note };
        if info.dwExtraInfo != winkey::OURS && winkey::on_key(info, take, summon) {
            return LRESULT(1);
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

fn send(keys: &[(VIRTUAL_KEY, bool)]) {
    let inputs: Vec<INPUT> = keys
        .iter()
        .map(|&(vk, down)| {
            let mut flags = if down { Default::default() } else { KEYEVENTF_KEYUP };
            if vk == VK_LWIN || vk == VK_RWIN {
                flags |= KEYEVENTF_EXTENDEDKEY;
            }
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, dwFlags: flags, dwExtraInfo: TEST, ..Default::default() } },
            }
        })
        .collect();
    unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
}

/// Releases every key the test may press, however it ends.
struct Release;
impl Drop for Release {
    fn drop(&mut self) {
        send(&[(VK_LSHIFT, false), (VK_LWIN, false), (VK_RWIN, false)]);
    }
}

fn press(vk: VIRTUAL_KEY) -> [(VIRTUAL_KEY, bool); 2] {
    [(vk, true), (vk, false)]
}

fn sequence(name: &str) -> Vec<(VIRTUAL_KEY, bool)> {
    let win_with = |vk: VIRTUAL_KEY| vec![(VK_LWIN, true), (vk, true), (vk, false), (VK_LWIN, false)];
    match name {
        "tap" => press(VK_LWIN).to_vec(),
        "Tab" => win_with(VK_TAB),
        "ShiftS" => vec![(VK_LWIN, true), (VK_LSHIFT, true), (VIRTUAL_KEY(b'S' as u16), true), (VIRTUAL_KEY(b'S' as u16), false), (VK_LSHIFT, false), (VK_LWIN, false)],
        k if k.len() == 1 && k != "L" && k != "E" && k != "D" => win_with(VIRTUAL_KEY(k.as_bytes()[0] as u16)),
        _ => panic!("not a safe sequence: {name}"),
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct Win {
    hwnd: isize,
    text: String,
}

fn process_name(pid: u32) -> String {
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return "?".into() };
        let mut buf = [0u16; 520];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(h);
        if !ok {
            return "?".into();
        }
        let full = String::from_utf16_lossy(&buf[..len as usize]);
        full.rsplit('\\').next().unwrap_or("?").to_string()
    }
}

/// Visible, uncloaked top-level windows.
fn shown() -> HashSet<Win> {
    unsafe extern "system" fn each(hwnd: HWND, out: LPARAM) -> windows::core::BOOL {
        let out = &mut *(out.0 as *mut HashSet<Win>);
        if IsWindowVisible(hwnd).as_bool() {
            let mut cloaked = 0u32;
            let _ = DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, (&raw mut cloaked).cast(), 4);
            if cloaked == 0 {
                let mut class = [0u16; 128];
                let n = GetClassNameW(hwnd, &mut class) as usize;
                let mut title = [0u16; 128];
                let t = GetWindowTextW(hwnd, &mut title) as usize;
                let mut pid = 0u32;
                GetWindowThreadProcessId(hwnd, Some(&mut pid));
                out.insert(Win {
                    hwnd: hwnd.0 as isize,
                    text: format!(
                        "{} [{}] {:?}",
                        process_name(pid),
                        String::from_utf16_lossy(&class[..n]),
                        String::from_utf16_lossy(&title[..t])
                    ),
                });
            }
        }
        true.into()
    }
    let mut out = HashSet::new();
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&raw mut out as isize));
    }
    out
}

/// Any window: its process, class and title.
fn describe(hwnd: HWND) -> String {
    unsafe {
        let mut class = [0u16; 128];
        let n = GetClassNameW(hwnd, &mut class) as usize;
        let mut title = [0u16; 128];
        let t = GetWindowTextW(hwnd, &mut title) as usize;
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        format!("{} [{}] {:?}", process_name(pid), String::from_utf16_lossy(&class[..n]), String::from_utf16_lossy(&title[..t]))
    }
}
fn pump(ms: u64) {
    let until = Instant::now() + Duration::from_millis(ms);
    let mut msg = MSG::default();
    while Instant::now() < until {
        unsafe {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    DefWindowProcW(hwnd, msg, w, l)
}

fn own_window() -> HWND {
    unsafe {
        let instance = GetModuleHandleW(None).unwrap_or_default();
        let class = WNDCLASSW { lpfnWndProc: Some(proc), hInstance: instance.into(), lpszClassName: w!("WinkeyLiveTest"), ..Default::default() };
        RegisterClassW(&class);
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST,
            w!("WinkeyLiveTest"),
            w!("winkey live test"),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            40,
            40,
            300,
            120,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .unwrap();
        front(hwnd);
        hwnd
    }
}

fn front(hwnd: HWND) {
    unsafe {
        if !SetForegroundWindow(hwnd).as_bool() {
            // One unused key press makes this process the last input's sender.
            send(&press(VIRTUAL_KEY(0xE8)));
            let _ = SetForegroundWindow(hwnd);
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let hooked = args.first().is_some_and(|m| m.starts_with("hook"));
    REAL.store(args.first().is_some_and(|m| m == "hook-real"), Ordering::SeqCst);
    let _release = Release;
    if hooked {
        std::thread::spawn(|| unsafe {
            let module = GetModuleHandleW(None).ok();
            let _hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(on_key), module.map(Into::into), 0).unwrap();
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {}
        });
        std::thread::sleep(Duration::from_millis(300));
    }
    let me = own_window();
    pump(500);
    for name in args.iter().skip(1) {
        front(me);
        pump(400);
        let before = shown();
        let summons = SUMMONS.load(Ordering::SeqCst);
        let t = Instant::now();
        send(&sequence(name));
        pump(1500);
        let new: Vec<String> = shown().difference(&before).map(|w| w.text.clone()).collect();
        println!(
            "{name:>7}: summoned {} | foreground {} | new windows: {:?} ({} ms)",
            SUMMONS.load(Ordering::SeqCst) - summons,
            describe(unsafe { GetForegroundWindow() }),
            new,
            t.elapsed().as_millis()
        );
        let fg = describe(unsafe { GetForegroundWindow() });
        let shell = ["StartMenuExperienceHost", "SearchHost", "ShellExperienceHost", "ShellAppRuntime", "ScreenClippingHost", "explorer", "TextInputHost", "ShellHost"];
        if !new.is_empty() || shell.iter().any(|p| fg.starts_with(p)) {
            send(&press(VK_ESCAPE));
            pump(1000);
            let left: Vec<String> = shown().difference(&before).map(|w| w.text.clone()).collect();
            println!("         after Esc: foreground {} | still new: {left:?}", describe(unsafe { GetForegroundWindow() }));
        }
    }
    unsafe {
        let _ = DestroyWindow(me);
    }
}
