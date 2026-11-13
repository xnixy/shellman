//! Dropping onto a window: an `IDropTarget` that asks a `Handler` what a drop at each point
//! would do, tells the drag image (through the shell's helper, which draws it over the window
//! and writes "Move to Pictures" under it), and hands the drop's contents over.
//!
//! A move the handler does itself is reported the way Explorer reports it (an "optimized
//! move"): the target says it moved the files (`Performed DropEffect`) and returns no effect,
//! so the source never deletes what it dragged.

use super::{data, Offer, Payload, MOVE, NONE};
use std::cell::RefCell;
use windows::core::{implement, Ref, Result};
use windows::Win32::Foundation::{HWND, POINT, POINTL};
use windows::Win32::System::Com::{CoCreateInstance, IDataObject, CLSCTX_INPROC_SERVER};
use windows::Win32::System::Ole::{
    IDropTarget, IDropTarget_Impl, RegisterDragDrop, RevokeDragDrop, DROPEFFECT,
};
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::Win32::UI::Shell::{CLSID_DragDropHelper, IDropTargetHelper};

/// What a drop at a point would do: its effect, and the name of what it would land in, for the
/// drag image's line.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Answer {
    pub effect: u32,
    pub label: Option<String>,
}

/// The window's side of a drop. Points are on the screen, in pixels.
pub trait Handler {
    fn enter(&self, offer: &Offer, pt: (i32, i32), keys: u32, allowed: u32) -> Answer;
    fn over(&self, pt: (i32, i32), keys: u32, allowed: u32) -> Answer;
    fn leave(&self);
    /// The drop itself: do it (or start doing it) and say what was done.
    fn dropped(&self, payload: Payload, pt: (i32, i32), keys: u32, allowed: u32) -> u32;
}

#[implement(IDropTarget)]
struct Target {
    handler: Box<dyn Handler>,
    hwnd: HWND,
    helper: Option<IDropTargetHelper>,
    data: RefCell<Option<IDataObject>>,
    /// What the drag image was last told, so it is told only when that changes.
    said: RefCell<Option<Answer>>,
}

impl Target_Impl {
    fn answer(&self, a: &Answer) {
        if self.said.borrow().as_ref() == Some(a) {
            return;
        }
        if let Some(d) = self.data.borrow().as_ref() {
            let insert = a.label.as_deref().unwrap_or("");
            data::describe(d, (a.effect != NONE && a.label.is_some()).then_some((a.effect, insert)));
        }
        *self.said.borrow_mut() = Some(a.clone());
    }
}

fn point(pt: &POINTL) -> (i32, i32) {
    (pt.x, pt.y)
}

impl IDropTarget_Impl for Target_Impl {
    fn DragEnter(
        &self,
        obj: Ref<'_, IDataObject>,
        keys: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> Result<()> {
        let obj = obj.ok()?.clone();
        let allowed = unsafe { (*effect).0 };
        let offer = data::offer(&obj);
        *self.data.borrow_mut() = Some(obj.clone());
        *self.said.borrow_mut() = None;
        let a = self.handler.enter(&offer, point(pt), keys.0, allowed);
        self.answer(&a);
        unsafe {
            *effect = DROPEFFECT(a.effect);
            if let Some(h) = &self.helper {
                let _ = h.DragEnter(self.hwnd, &obj, &POINT { x: pt.x, y: pt.y }, DROPEFFECT(a.effect));
            }
        }
        Ok(())
    }

    fn DragOver(&self, keys: MODIFIERKEYS_FLAGS, pt: &POINTL, effect: *mut DROPEFFECT) -> Result<()> {
        let allowed = unsafe { (*effect).0 };
        let a = self.handler.over(point(pt), keys.0, allowed);
        self.answer(&a);
        unsafe {
            *effect = DROPEFFECT(a.effect);
            if let Some(h) = &self.helper {
                let _ = h.DragOver(&POINT { x: pt.x, y: pt.y }, DROPEFFECT(a.effect));
            }
        }
        Ok(())
    }

    fn DragLeave(&self) -> Result<()> {
        self.handler.leave();
        if let Some(d) = self.data.borrow().as_ref() {
            data::describe(d, None);
        }
        *self.data.borrow_mut() = None;
        *self.said.borrow_mut() = None;
        if let Some(h) = &self.helper {
            unsafe {
                let _ = h.DragLeave();
            }
        }
        Ok(())
    }

    fn Drop(
        &self,
        obj: Ref<'_, IDataObject>,
        keys: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> Result<()> {
        let obj = obj.ok()?.clone();
        let allowed = unsafe { (*effect).0 };
        let payload = data::payload(&obj);
        let done = self.handler.dropped(payload, point(pt), keys.0, allowed);
        data::describe(&obj, None);
        // A move done here: say so, and return no effect so the source deletes nothing.
        let returned = if done == MOVE {
            let _ = data::set_dword(&obj, data::PERFORMED_EFFECT, MOVE);
            let _ = data::set_dword(&obj, data::LOGICAL_EFFECT, MOVE);
            NONE
        } else {
            if done != NONE {
                let _ = data::set_dword(&obj, data::PERFORMED_EFFECT, done);
            }
            done
        };
        unsafe {
            *effect = DROPEFFECT(returned);
            if let Some(h) = &self.helper {
                let _ = h.Drop(&obj, &POINT { x: pt.x, y: pt.y }, DROPEFFECT(done));
            }
        }
        *self.data.borrow_mut() = None;
        *self.said.borrow_mut() = None;
        Ok(())
    }
}

/// Make `handler` the window's drop target, in place of any it had. Call on the window's
/// thread, with OLE initialized there.
pub fn register(hwnd: isize, handler: Box<dyn Handler>) -> Result<()> {
    let hwnd = HWND(hwnd as *mut _);
    let helper = unsafe { CoCreateInstance(&CLSID_DragDropHelper, None, CLSCTX_INPROC_SERVER) }.ok();
    let target: IDropTarget = Target {
        handler,
        hwnd,
        helper,
        data: RefCell::new(None),
        said: RefCell::new(None),
    }
    .into();
    unsafe {
        let _ = RevokeDragDrop(hwnd);
        RegisterDragDrop(hwnd, &target)
    }
}
