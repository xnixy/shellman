//! A made-up menu handler for tests: a plain entry, an owner-drawn one, and a submenu it fills
//! only when told the submenu is opening. No extension on the test machine needs to draw its own
//! entries for the owner-drawn path to be checked.

use super::slow::{SlowBook, TOO_SLOW};
use super::walk::Walk;
use super::{paint::Palette, Cascade};
use windows::core::{implement, w, Result, HRESULT, PSTR};
use windows::Win32::Foundation::{COLORREF, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{CreateSolidBrush, DeleteObject, FillRect, TextOutW, HGDIOBJ};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Controls::{DRAWITEMSTRUCT, MEASUREITEMSTRUCT};
use windows::Win32::UI::Shell::{
    IContextMenu, IContextMenu2_Impl, IContextMenu3, IContextMenu3_Impl, IContextMenu_Impl,
    CMINVOKECOMMANDINFO,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, HMENU, MF_OWNERDRAW, MF_POPUP, MF_STRING,
};

#[implement(IContextMenu3)]
struct Fake;

impl IContextMenu_Impl for Fake_Impl {
    fn QueryContextMenu(&self, hmenu: HMENU, _index: u32, first: u32, _last: u32, _flags: u32) -> HRESULT {
        unsafe {
            let _ = AppendMenuW(hmenu, MF_STRING, first as usize, w!("Plain &item"));
            let _ = AppendMenuW(hmenu, MF_OWNERDRAW, (first + 1) as usize, windows::core::PCWSTR(0x42 as *const u16));
            let sub = CreatePopupMenu().unwrap();
            let _ = AppendMenuW(hmenu, MF_POPUP | MF_STRING, sub.0 as usize, w!("&Lazy"));
        }
        // Three ids used.
        HRESULT(3)
    }

    fn InvokeCommand(&self, _info: *const CMINVOKECOMMANDINFO) -> Result<()> {
        Ok(())
    }

    fn GetCommandString(&self, id: usize, kind: u32, _r: *const u32, name: PSTR, max: u32) -> Result<()> {
        let text: &str = match (id, kind) {
            (0, 4) => "plain",
            (1, 5) => "Drawn entry",
            (2, 4) => "inside",
            _ => return Err(windows::Win32::Foundation::E_NOTIMPL.into()),
        };
        let wide: Vec<u16> = text.encode_utf16().chain([0]).collect();
        let n = wide.len().min(max as usize);
        unsafe { std::ptr::copy_nonoverlapping(wide.as_ptr(), name.0.cast::<u16>(), n) };
        Ok(())
    }
}

impl IContextMenu2_Impl for Fake_Impl {
    fn HandleMenuMsg(&self, msg: u32, wparam: WPARAM, lparam: LPARAM) -> Result<()> {
        self.HandleMenuMsg2(msg, wparam, lparam, std::ptr::null_mut())
    }
}

impl IContextMenu3_Impl for Fake_Impl {
    fn HandleMenuMsg2(&self, msg: u32, wparam: WPARAM, lparam: LPARAM, _out: *mut LRESULT) -> Result<()> {
        unsafe {
            match msg {
                // WM_INITMENUPOPUP: fill the submenu now.
                0x0117 => {
                    let _ = AppendMenuW(HMENU(wparam.0 as *mut _), MF_STRING, 3, w!("Inside"));
                }
                // WM_MEASUREITEM
                0x002C => {
                    let m = &mut *(lparam.0 as *mut MEASUREITEMSTRUCT);
                    m.itemWidth = 60;
                    m.itemHeight = 20;
                }
                // WM_DRAWITEM: a red square, and text in whatever colour the DC was given.
                0x002B => {
                    let d = &*(lparam.0 as *const DRAWITEMSTRUCT);
                    assert_eq!(d.itemData, 0x42, "item data comes back");
                    let brush = CreateSolidBrush(COLORREF(0x0000FF));
                    FillRect(d.hDC, &RECT { left: 2, top: 2, right: 18, bottom: 18 }, brush);
                    let _ = DeleteObject(HGDIOBJ(brush.0));
                    let hi: Vec<u16> = "Hi".encode_utf16().collect();
                    let _ = TextOutW(d.hDC, 24, 2, &hi);
                }
                _ => return Err(windows::Win32::Foundation::E_NOTIMPL.into()),
            }
        }
        Ok(())
    }
}

fn fake() -> IContextMenu {
    let cm3: IContextMenu3 = Fake.into();
    windows::core::Interface::cast(&cm3).unwrap()
}

fn palette() -> Palette {
    Palette { text: 0xFFFFFF, background: 0x202020 }
}

#[test]
fn walks_owner_drawn_and_lazy_entries() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    let plan = SlowBook::default();
    let mut walk = Walk::query(fake(), false, palette()).expect("menu");
    let mut items = walk.top(&plan);
    assert_eq!(items.len(), 3);
    assert_eq!(items[0].label, "Plain item");
    assert_eq!(items[0].mnemonic, Some('i'));
    assert_eq!(items[0].verb.as_deref(), Some("plain"));
    // The owner-drawn entry: no text, named by its help text, and rendered off screen.
    let drawn = &items[1];
    assert!(drawn.owner_drawn && drawn.label.is_empty());
    assert_eq!(drawn.title(), "Drawn entry");
    let pic = drawn.drawn.as_ref().expect("rendered");
    assert_eq!(pic.size, [60, 20]);
    let px = |x: usize, y: usize| &pic.rgba[(y * 60 + x) * 4..(y * 60 + x) * 4 + 4];
    assert_eq!(px(10, 10), &[255, 0, 0, 255], "the red square, opaque");
    assert_eq!(px(1, 1)[3], 0, "outside it, see-through");
    assert!(pic.rgba.chunks_exact(4).skip(24).any(|p| p[3] > 0 && p[0] > 200 && p[1] > 200), "white text");
    // The submenu is filled only once it is told it opens.
    assert_eq!(items[2].cascade, Cascade::Pending);
    assert!(items[2].submenu.is_empty());
    let (key, _) = walk.fill_next(&mut items, &plan).expect("one to fill");
    assert_eq!(key, "label:Lazy");
    assert_eq!(items[2].cascade, Cascade::Filled);
    assert_eq!(items[2].submenu[0].label, "Inside");
    assert_eq!(items[2].submenu[0].verb.as_deref(), Some("inside"));
    assert!(walk.fill_next(&mut items, &plan).is_none());
}

#[test]
fn a_handler_that_hung_before_is_left_for_when_it_is_opened() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    let mut plan = SlowBook::default();
    plan.record("label:Lazy", TOO_SLOW * 2);
    let mut walk = Walk::query(fake(), false, palette()).expect("menu");
    let mut items = walk.top(&plan);
    assert_eq!(items[2].cascade, Cascade::Deferred);
    assert_eq!(walk.pending(), 0, "nothing waits for it");
    let path = items[2].path.clone();
    walk.fill_deferred(&path, &mut items, &plan).expect("filled on demand");
    assert_eq!(items[2].submenu[0].label, "Inside");
}
