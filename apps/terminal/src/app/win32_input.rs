//! win32-input-mode: while the shown tab's console asked for it (`CSI ? 9001 h`, ConPTY does at
//! its start) and the grid has the keyboard, keys reach the console as whole key events
//! ([`keys::Win32Key`]: key code, scan code, character, down or up, modifier state), so console
//! programs see exactly what was pressed, as in a console window: PSReadLine's Shift+arrow
//! selection and its own Ctrl+C copy, Ctrl+Alt combinations, key releases.
//!
//! egui only reports keys it knows and no releases or scan codes, so the window's messages are
//! read first, in a subclass of the window procedure: `WM_KEYDOWN`/`UP` (and the `SYS` ones) and
//! the `WM_CHAR`s Windows makes of them (`TranslateMessage` has already queued those when the key
//! is handled, so a look at the queue tells whether characters follow, as winit does). The key
//! events are queued for the shown tab and the messages are kept from egui, except:
//!
//! - the terminal's own keys (`input::keeps`: its shortcuts, paste, copy of its own selection,
//!   paging through the history, Alt+F4), which go on to egui as before;
//! - modifier and lock keys, which go to both (egui tracks modifiers for the mouse and shortcuts);
//! - input-method keys (`VK_PROCESSKEY`), left to the IME, whose committed text is sent as text.

use crate::keys::{self, Composer, Mods, Next, RawKey};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, GetKeyboardLayout, GetKeyboardState, ToUnicodeEx};
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    PeekMessageW, MSG, PM_NOREMOVE, WM_CHAR, WM_DEADCHAR, WM_KEYDOWN, WM_KEYFIRST, WM_KEYLAST, WM_KEYUP, WM_SYSCHAR, WM_SYSDEADCHAR,
    WM_SYSKEYDOWN, WM_SYSKEYUP,
};

/// What the window procedure and the app share.
pub struct Gate {
    /// The shown tab's console asked for win32-input-mode and the grid has the keyboard.
    active: AtomicBool,
    /// The shown tab has a selection (Ctrl+C, Enter and Escape are then the terminal's).
    selection: AtomicBool,
    /// The shown tab has more than one pane (Alt+arrows are then the terminal's).
    panes: AtomicBool,
    /// Encoded key events not yet sent, in order.
    queue: Mutex<Vec<u8>>,
    composer: Mutex<Composer>,
    /// Keys whose key-down was kept from egui: so are their characters.
    swallowing: AtomicBool,
    ctx: egui::Context,
}

impl Gate {
    /// Called every frame: whether keys go to the console whole, and whether there is a selection.
    pub fn set(&self, active: bool, selection: bool, panes: bool) {
        self.active.store(active, Ordering::Release);
        self.panes.store(panes, Ordering::Release);
        self.selection.store(selection, Ordering::Release);
    }

    pub fn active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }

    /// Key events queued since the last call.
    pub fn take(&self) -> Vec<u8> {
        std::mem::take(&mut *self.queue.lock().unwrap())
    }

    fn push(&self, key: keys::Win32Key) {
        self.queue.lock().unwrap().extend_from_slice(key.encode().as_bytes());
        self.ctx.request_repaint();
    }
}

const SUBCLASS_ID: usize = 0x5348_4D54; // "SHMT"

/// Read the window's key messages before winit does.
pub fn install(hwnd: isize, ctx: egui::Context) -> Arc<Gate> {
    let gate = Arc::new(Gate {
        active: AtomicBool::new(false),
        selection: AtomicBool::new(false),
        panes: AtomicBool::new(false),
        queue: Mutex::new(Vec::new()),
        composer: Mutex::new(Composer::default()),
        swallowing: AtomicBool::new(false),
        ctx,
    });
    // The window lives as long as the process; the subclass holds its own reference.
    let data = Arc::into_raw(gate.clone()) as usize;
    unsafe {
        let _ = SetWindowSubclass(HWND(hwnd as *mut _), Some(proc), SUBCLASS_ID, data);
    }
    gate
}

fn key_down_vk(v: u16) -> bool {
    (unsafe { GetKeyState(v as i32) }) < 0
}

fn toggled(v: u16) -> bool {
    (unsafe { GetKeyState(v as i32) }) & 1 != 0
}

fn raw(wparam: WPARAM, lparam: LPARAM) -> RawKey {
    let l = lparam.0 as u32;
    let enhanced = l & (1 << 24) != 0;
    RawKey {
        vk: wparam.0 as u16,
        sc: ((l >> 16) & 0xFF) as u16,
        state: keys::control_state(key_down_vk, toggled, enhanced),
        repeat: (l & 0xFFFF).max(1) as u16,
    }
}

fn mods() -> Mods {
    Mods { shift: key_down_vk(keys::vk::SHIFT), ctrl: key_down_vk(keys::vk::CONTROL), alt: key_down_vk(keys::vk::MENU) }
}

/// What follows a key-down in the queue: its characters, a dead key's accent, or nothing.
fn next_message(hwnd: HWND) -> Next {
    let mut msg = MSG::default();
    let found = unsafe { PeekMessageW(&mut msg, Some(hwnd), WM_KEYFIRST, WM_KEYLAST, PM_NOREMOVE) }.as_bool();
    if !found {
        return Next::Nothing;
    }
    match msg.message {
        WM_CHAR | WM_SYSCHAR => Next::Char,
        WM_DEADCHAR | WM_SYSDEADCHAR => Next::DeadChar,
        _ => Next::Nothing,
    }
}

/// The character a released key stands for (the console's key-up events carry it), without
/// touching the keyboard's dead-key state.
fn char_of(key: RawKey) -> u16 {
    let mut state = [0u8; 256];
    let mut out = [0u16; 4];
    unsafe {
        if GetKeyboardState(&mut state).is_err() {
            return 0;
        }
        // Flag 4: do not change the keyboard state (Windows 10 1607 and later).
        let n = ToUnicodeEx(key.vk as u32, key.sc as u32, &state, &mut out, 4, Some(GetKeyboardLayout(0)));
        if n == 1 {
            out[0]
        } else {
            0
        }
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _id: usize, data: usize) -> LRESULT {
    let gate = &*(data as *const Gate);
    if gate.active() {
        match msg {
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                let key = raw(wparam, lparam);
                if key.vk == keys::vk::PROCESSKEY || key.vk == keys::vk::LWIN || key.vk == keys::vk::RWIN {
                    return DefSubclassProc(hwnd, msg, wparam, lparam);
                }
                if keys::is_modifier(key.vk) {
                    if let Some(k) = gate.composer.lock().unwrap().key_down(key, Next::Nothing) {
                        gate.push(k);
                    }
                    return DefSubclassProc(hwnd, msg, wparam, lparam);
                }
                if super::input::keeps(key.vk, mods(), gate.selection.load(Ordering::Acquire), gate.panes.load(Ordering::Acquire)) {
                    gate.swallowing.store(false, Ordering::Release);
                    return DefSubclassProc(hwnd, msg, wparam, lparam);
                }
                gate.swallowing.store(true, Ordering::Release);
                let next = next_message(hwnd);
                if let Some(k) = gate.composer.lock().unwrap().key_down(key, next) {
                    gate.push(k);
                }
                return LRESULT(0);
            }
            WM_CHAR | WM_SYSCHAR if gate.swallowing.load(Ordering::Acquire) => {
                let k = gate.composer.lock().unwrap().char(wparam.0 as u16);
                gate.push(k);
                return LRESULT(0);
            }
            WM_DEADCHAR | WM_SYSDEADCHAR if gate.swallowing.load(Ordering::Acquire) => {
                gate.composer.lock().unwrap().dead_char();
                return LRESULT(0);
            }
            WM_KEYUP | WM_SYSKEYUP => {
                let key = raw(wparam, lparam);
                if key.vk == keys::vk::PROCESSKEY || key.vk == keys::vk::LWIN || key.vk == keys::vk::RWIN {
                    return DefSubclassProc(hwnd, msg, wparam, lparam);
                }
                let modifier = keys::is_modifier(key.vk);
                // A key the terminal kept had no key-down sent: no key-up either.
                if !modifier && super::input::keeps(key.vk, mods(), gate.selection.load(Ordering::Acquire), gate.panes.load(Ordering::Acquire)) {
                    return DefSubclassProc(hwnd, msg, wparam, lparam);
                }
                let ch = if modifier { 0 } else { char_of(key) };
                let up = gate.composer.lock().unwrap().key_up(key, ch);
                gate.push(up);
                if modifier {
                    return DefSubclassProc(hwnd, msg, wparam, lparam);
                }
                return LRESULT(0);
            }
            _ => {}
        }
    } else if matches!(msg, WM_KEYDOWN | WM_SYSKEYDOWN) {
        gate.swallowing.store(false, Ordering::Release);
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}
