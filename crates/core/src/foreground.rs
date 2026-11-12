//! Becoming allowed to hand the foreground to another process.

use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{AllowSetForegroundWindow, ASFW_ANY};

/// Marks input this crate synthesised ("SHMN"), so hooks can tell it from a real key.
pub const OURS: usize = 0x5348_4D4E;

/// A virtual key no keyboard has and no app uses.
const VK_MASK: VIRTUAL_KEY = VIRTUAL_KEY(0xE8);

/// One press of an unused key. Apps open their menu bar when Alt is pressed and released with no
/// key in between; this keeps them out of it. Sending input also makes this process the last
/// input's sender, which lets it bring a window to front or pass that right on.
pub fn send_mask() {
    let key = |flags| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT { wVk: VK_MASK, dwFlags: flags, dwExtraInfo: OURS, ..Default::default() },
        },
    };
    let inputs = [key(Default::default()), key(KEYEVENTF_KEYUP)];
    unsafe {
        SendInput(&inputs, size_of::<INPUT>() as i32);
    }
}

/// Let process `pid` take the foreground. Windows refuses the grant when this process is not
/// itself allowed to set the foreground, so on a refusal one unused key press is sent first and
/// the grant tried again. Returns (granted, needed the key press).
pub fn allow_foreground(pid: u32) -> (bool, bool) {
    // The process, and then any process: a menu is often shown by a helper process of the app
    // that owns the icon's window. The grant ends with the next input event.
    let grant = || {
        let owner = unsafe { AllowSetForegroundWindow(pid) }.is_ok();
        let any = unsafe { AllowSetForegroundWindow(ASFW_ANY) }.is_ok();
        owner || any
    };
    if grant() {
        return (true, false);
    }
    send_mask();
    (grant(), true)
}
