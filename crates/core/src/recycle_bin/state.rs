//! The bin as a whole: how much is in it, and emptying it.

use super::com::Apartment;
use crate::fsops::format_size;
use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::{
    SHEmptyRecycleBinW, SHQueryRecycleBinW, SHERB_NOCONFIRMATION, SHERB_NOPROGRESSUI,
    SHERB_NOSOUND, SHQUERYRBINFO,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct BinState {
    pub items: u64,
    pub bytes: u64,
}

impl BinState {
    pub fn is_empty(&self) -> bool {
        self.items == 0
    }
}

/// Item count and total size over every drive's bin, without listing them.
pub fn state() -> Option<BinState> {
    let _apartment = Apartment::enter();
    let mut info = SHQUERYRBINFO { cbSize: size_of::<SHQUERYRBINFO>() as u32, ..Default::default() };
    // A null root means all drives.
    unsafe { SHQueryRecycleBinW(PCWSTR::null(), &mut info) }.ok()?;
    Some(BinState { items: info.i64NumItems.max(0) as u64, bytes: info.i64Size.max(0) as u64 })
}

/// "3 items, 12 MB", for confirmations.
pub fn summary(s: BinState) -> String {
    let items = if s.items == 1 { "1 item".to_owned() } else { format!("{} items", s.items) };
    format!("{items}, {}", format_size(s.bytes))
}

/// Delete everything in the bin for good. No shell confirmation, progress window or sound: the
/// caller has asked the user already and shows its own progress.
pub fn empty(hwnd: isize) -> Result<(), String> {
    let _apartment = Apartment::enter();
    let flags = SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND;
    unsafe { SHEmptyRecycleBinW(Some(HWND(hwnd as *mut _)), PCWSTR::null(), flags) }
        .map_err(|e| e.message())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_text() {
        assert_eq!(summary(BinState { items: 1, bytes: 512 }), "1 item, 512 B");
        assert_eq!(summary(BinState { items: 3, bytes: 3 * 1024 * 1024 }), "3 items, 3.0 MB");
        assert!(BinState::default().is_empty());
    }
}
