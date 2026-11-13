//! Dragging files out: a real OLE drag that any app accepts, with the shell's data object for
//! the items and a picture under the pointer.
//!
//! `DoDragDrop` runs its own loop until the drop, so it is run on a thread of its own: the
//! window's thread keeps drawing (its own drop target lights folders up meanwhile, see
//! `target`). That thread shares the window's input state (`AttachThreadInput`), so the drag
//! has the mouse. The button's release goes to the drag, not the window, so one is posted to
//! the window afterwards: without it the window would think the button was still down.

use super::{data, MK_LBUTTON};
use std::path::PathBuf;
use windows::core::{implement, BOOL, HRESULT};
use windows::Win32::Foundation::{
    DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS, HWND, LPARAM, POINT,
    SIZE, S_OK, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    CreateDIBSection, DeleteObject, ScreenToClient, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    DIB_RGB_COLORS,
};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::System::Ole::{
    DoDragDrop, IDropSource, IDropSource_Impl, OleInitialize, OleUninitialize, DROPEFFECT,
};
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_ESCAPE, VK_LBUTTON};
use windows::Win32::UI::Shell::{CLSID_DragDropHelper, IDragSourceHelper2, SHDRAGIMAGE};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetWindowThreadProcessId, PostMessageW, WM_LBUTTONUP,
};

/// The picture under the pointer: premultiplied BGRA, top row first, and where the pointer
/// holds it.
#[derive(Clone, Debug)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
    pub hotspot: (i32, i32),
}

/// How a drag ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ended {
    /// What `DoDragDrop` returned: the effect the target reported.
    pub effect: u32,
    /// What the target says it did (`Performed DropEffect`): a target that moved the files
    /// itself returns no effect but says it moved them.
    pub performed: u32,
    /// The drag never started (the button was already up, or the items could not be read).
    pub cancelled: bool,
}

#[implement(IDropSource)]
struct Source;

impl IDropSource_Impl for Source_Impl {
    fn QueryContinueDrag(&self, escape: BOOL, keys: MODIFIERKEYS_FLAGS) -> HRESULT {
        // The keyboard's messages go to the window's thread, not this one, so OLE does not see
        // Esc itself: it is read from the keyboard.
        let esc_down = unsafe { GetAsyncKeyState(i32::from(VK_ESCAPE.0)) } as u16 & 0x8000 != 0;
        if escape.as_bool() || esc_down {
            DRAGDROP_S_CANCEL
        } else if keys.0 & MK_LBUTTON == 0 {
            DRAGDROP_S_DROP
        } else {
            S_OK
        }
    }

    fn GiveFeedback(&self, _effect: DROPEFFECT) -> HRESULT {
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}

fn bitmap(img: &Image) -> Option<windows::Win32::Graphics::Gdi::HBITMAP> {
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: img.width as i32,
            // Top row first.
            biHeight: -(img.height as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits = std::ptr::null_mut();
    let h = unsafe { CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0) }.ok()?;
    let n = (img.width * img.height * 4) as usize;
    if bits.is_null() || img.bgra.len() < n {
        unsafe {
            let _ = DeleteObject(h.into());
        }
        return None;
    }
    unsafe { std::ptr::copy_nonoverlapping(img.bgra.as_ptr(), bits as *mut u8, n) };
    Some(h)
}

/// Give the drag `img` as its picture, with room for the drop's description ("Move to …").
fn set_image(obj: &windows::Win32::System::Com::IDataObject, img: &Image) {
    unsafe {
        let Ok(helper) =
            CoCreateInstance::<_, IDragSourceHelper2>(&CLSID_DragDropHelper, None, CLSCTX_INPROC_SERVER)
        else {
            return;
        };
        // DSH_ALLOWDROPDESCRIPTIONTEXT.
        let _ = helper.SetFlags(1);
        let Some(hbmp) = bitmap(img) else { return };
        let shdi = SHDRAGIMAGE {
            sizeDragImage: SIZE {
                cx: img.width as i32,
                cy: img.height as i32,
            },
            ptOffset: POINT {
                x: img.hotspot.0,
                y: img.hotspot.1,
            },
            hbmpDragImage: hbmp,
            // CLR_NONE: the picture's own alpha.
            crColorKey: windows::Win32::Foundation::COLORREF(0xFFFF_FFFF),
        };
        // On success the helper owns the bitmap.
        if helper.InitializeFromBitmap(&shdi, obj).is_err() {
            let _ = DeleteObject(hbmp.into());
        }
    }
}

/// Drag `paths` out of the window `hwnd`, allowing `allowed` (`COPY | MOVE | LINK`). Returns at
/// once; `done` is called from the drag's thread when it ends.
pub fn start(
    hwnd: isize,
    paths: Vec<PathBuf>,
    image: Option<Image>,
    allowed: u32,
    done: impl FnOnce(Ended) + Send + 'static,
) {
    std::thread::spawn(move || unsafe {
        let ole = OleInitialize(None).is_ok();
        let window = HWND(hwnd as *mut _);
        let ui_thread = GetWindowThreadProcessId(window, None);
        let me = GetCurrentThreadId();
        let attached = ui_thread != 0 && AttachThreadInput(me, ui_thread, true).as_bool();
        let mut ended = Ended {
            effect: 0,
            performed: 0,
            cancelled: true,
        };
        // Let go already (a quick flick): no drag, or it would drop wherever the pointer is.
        let held = GetAsyncKeyState(i32::from(VK_LBUTTON.0)) as u16 & 0x8000 != 0;
        if let (true, Ok(obj)) = (held, data::for_paths(&paths)) {
            if let Some(img) = &image {
                set_image(&obj, img);
            }
            let source: IDropSource = Source.into();
            let mut effect = DROPEFFECT(0);
            let hr = DoDragDrop(&obj, &source, DROPEFFECT(allowed), &mut effect);
            ended.cancelled = hr != DRAGDROP_S_DROP;
            ended.effect = effect.0;
            ended.performed = data::get_dword(&obj, data::PERFORMED_EFFECT).unwrap_or(effect.0);
        }
        if attached {
            let _ = AttachThreadInput(me, ui_thread, false);
        }
        // The release the drag took: the window gets it, where the pointer is now.
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let _ = ScreenToClient(window, &mut pt);
        let lp = ((pt.y as u32 & 0xffff) << 16) | (pt.x as u32 & 0xffff);
        let _ = PostMessageW(Some(window), WM_LBUTTONUP, WPARAM(0), LPARAM(lp as isize));
        done(ended);
        if ole {
            OleUninitialize();
        }
    });
}
