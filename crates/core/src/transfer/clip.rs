//! The Windows clipboard, for files: Copy and Cut put the shell's data object there (with the
//! effect a paste should have), Paste reads files, a picture or text, whoever put them there,
//! and a listener says when it changes, so nothing has to look every frame.

use super::{data, dropfiles, Offer, Payload, COPY, LINK, MOVE};
use std::path::PathBuf;
use std::sync::Arc;
use windows::core::w;
use windows::Win32::Foundation::{HGLOBAL, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, CloseClipboard, EmptyClipboard, GetClipboardData,
    GetClipboardSequenceNumber, IsClipboardFormatAvailable, OpenClipboard,
};
use windows::Win32::System::Ole::{
    OleFlushClipboard, OleInitialize, OleSetClipboard, OleUninitialize, CF_DIB, CF_DIBV5, CF_HDROP,
    CF_UNICODETEXT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, RegisterClassW, HWND_MESSAGE,
    MSG, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLIPBOARDUPDATE, WNDCLASSW,
};

/// What is on the clipboard now, as far as files go.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct State {
    /// The clipboard's sequence number when this was read.
    pub seq: u32,
    pub offer: Offer,
    /// What a paste of the files should do: `MOVE` after a Cut.
    pub effect: u32,
}

impl State {
    /// Whether a paste would do anything.
    pub fn can_paste(&self) -> bool {
        !self.offer.is_empty()
    }

    /// Whether the files on it were cut (and so are drawn faded until pasted).
    pub fn cut(&self) -> bool {
        !self.offer.files.is_empty() && self.effect & MOVE != 0 && self.effect & COPY == 0
    }
}

/// The clipboard, open while this lives. It can be held by another app for a moment, so a few
/// tries are made.
struct Open;

impl Open {
    fn new() -> Option<Self> {
        for i in 0..10 {
            if unsafe { OpenClipboard(None) }.is_ok() {
                return Some(Open);
            }
            std::thread::sleep(std::time::Duration::from_millis(10 + i * 10));
        }
        None
    }

    fn bytes(&self, cf: u16) -> Option<Vec<u8>> {
        let h = unsafe { GetClipboardData(u32::from(cf)) }.ok()?;
        data::global_bytes(HGLOBAL(h.0))
    }
}

impl Drop for Open {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseClipboard();
        }
    }
}

fn available(cf: u16) -> bool {
    unsafe { IsClipboardFormatAvailable(u32::from(cf)) }.is_ok()
}

/// The effect in `Preferred DropEffect`'s bytes; a copy when there is none.
pub fn effect_of(bytes: Option<&[u8]>) -> u32 {
    bytes
        .and_then(|b| b.get(..4))
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .filter(|&e| e != 0)
        .unwrap_or(COPY)
}

/// Read what is on the clipboard: its files (cheap), and whether a picture or text is there.
pub fn state() -> State {
    let seq = unsafe { GetClipboardSequenceNumber() };
    let image = available(data::format(data::PNG)) || available(CF_DIBV5.0) || available(CF_DIB.0);
    let text = available(CF_UNICODETEXT.0);
    let mut st = State {
        seq,
        offer: Offer {
            files: Vec::new(),
            image,
            text,
        },
        effect: COPY,
    };
    if available(CF_HDROP.0) {
        if let Some(open) = Open::new() {
            st.offer.files = open.bytes(CF_HDROP.0).map(|b| dropfiles::parse(&b)).unwrap_or_default();
            st.effect = effect_of(open.bytes(data::format(data::PREFERRED_EFFECT)).as_deref());
        }
    }
    st
}

/// Read what is on the clipboard in full, for a paste: files with their effect, or else a
/// picture (as PNG), or else text. A picture can be large and its app may render it only now,
/// so call this from a worker.
pub fn payload() -> (Payload, u32, u32) {
    let seq = unsafe { GetClipboardSequenceNumber() };
    let Some(open) = Open::new() else {
        return (Payload::Nothing, COPY, seq);
    };
    let files = open.bytes(CF_HDROP.0).map(|b| dropfiles::parse(&b)).unwrap_or_default();
    if !files.is_empty() {
        let effect = effect_of(open.bytes(data::format(data::PREFERRED_EFFECT)).as_deref());
        return (Payload::Files(files), effect, seq);
    }
    let png = available(data::format(data::PNG))
        .then(|| open.bytes(data::format(data::PNG)))
        .flatten();
    let pic = data::picture(png, || open.bytes(CF_DIBV5.0).or_else(|| open.bytes(CF_DIB.0)));
    if let Some(p) = pic {
        return (Payload::Image(p), COPY, seq);
    }
    match open.bytes(CF_UNICODETEXT.0).map(|b| data::text_from_wide(&b)) {
        Some(t) if !t.is_empty() => (Payload::Text(t), COPY, seq),
        _ => (Payload::Nothing, COPY, seq),
    }
}

/// Put files on the clipboard as Explorer does: the shell's data object, with the effect a
/// paste should have (a move after Cut). Its formats are rendered at once, so they stay there
/// after Files closes. Runs on a thread of its own, for OLE.
pub fn put(paths: Vec<PathBuf>, cut: bool) -> Result<(), String> {
    std::thread::spawn(move || unsafe {
        let ole = OleInitialize(None).is_ok();
        let r = (|| {
            let obj = data::for_paths(&paths).map_err(|e| e.message())?;
            let effect = if cut { MOVE } else { COPY | LINK };
            data::set_dword(&obj, data::PREFERRED_EFFECT, effect).map_err(|e| e.message())?;
            OleSetClipboard(&obj).map_err(|e| e.message())?;
            OleFlushClipboard().map_err(|e| e.message())
        })();
        if ole {
            OleUninitialize();
        }
        r
    })
    .join()
    .unwrap_or_else(|_| Err("the clipboard thread stopped".into()))
}

/// Empty the clipboard if it still holds what was there at `seq` (after the cut files were
/// pasted, as Explorer does).
pub fn clear_if(seq: u32) {
    if unsafe { GetClipboardSequenceNumber() } != seq {
        return;
    }
    if let Some(_open) = Open::new() {
        if unsafe { GetClipboardSequenceNumber() } == seq {
            unsafe {
                let _ = EmptyClipboard();
            }
        }
    }
}

thread_local! {
    static ON_CHANGE: std::cell::RefCell<Option<Arc<dyn Fn() + Send + Sync>>> =
        const { std::cell::RefCell::new(None) };
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_CLIPBOARDUPDATE {
        ON_CHANGE.with(|f| {
            if let Some(f) = f.borrow().as_ref() {
                f();
            }
        });
        return LRESULT(0);
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}

/// Calls `on_change` on a thread of its own whenever the clipboard changes (and once at the
/// start), for as long as the process runs. Windows tells a hidden window of that thread.
pub fn listen(on_change: Arc<dyn Fn() + Send + Sync>) {
    std::thread::spawn(move || unsafe {
        on_change();
        ON_CHANGE.with(|f| *f.borrow_mut() = Some(on_change));
        let class = WNDCLASSW {
            lpfnWndProc: Some(proc),
            lpszClassName: w!("shellman-clipboard-listener"),
            ..Default::default()
        };
        RegisterClassW(&class);
        let Ok(hwnd) = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("shellman-clipboard-listener"),
            w!(""),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            None,
            None,
        ) else {
            return;
        };
        if AddClipboardFormatListener(hwnd).is_err() {
            return;
        }
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            DispatchMessageW(&msg);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_preferred_effect_reads_as_a_copy_when_missing() {
        assert_eq!(effect_of(None), COPY);
        assert_eq!(effect_of(Some(&2u32.to_le_bytes())), MOVE);
        assert_eq!(effect_of(Some(&5u32.to_le_bytes())), COPY | LINK);
        assert_eq!(effect_of(Some(&0u32.to_le_bytes())), COPY);
        assert_eq!(effect_of(Some(&[1, 2])), COPY);
    }

    #[test]
    fn cut_is_a_move_of_files_only() {
        let files = Offer {
            files: vec![PathBuf::from(r"C:\x")],
            ..Default::default()
        };
        let st = |offer: Offer, effect| State { seq: 1, offer, effect };
        assert!(st(files.clone(), MOVE).cut());
        assert!(!st(files.clone(), COPY | LINK).cut());
        assert!(!st(Offer::default(), MOVE).cut());
        assert!(st(files, COPY).can_paste());
        assert!(st(Offer { text: true, ..Default::default() }, COPY).can_paste());
        assert!(!st(Offer::default(), COPY).can_paste());
    }
}
