//! The glass under the bar's windows (see `shellman_ui::glass`), kept by window handle.
//!
//! A glass panel belongs to the UI thread (its visuals and its backdrop window do), while the
//! bars' state is shared with the viewports' frames and must be `Send`; so the panels live here,
//! in a thread-local map, and the state keeps only window handles. Every frame runs on the UI
//! thread, so the map is always at hand.
//!
//! A window that is destroyed takes its backdrop with it (the panel hears `WM_NCDESTROY`), so a
//! panel left in the map is only an empty shell; `prune` drops those.

use shellman_ui::glass::{GlassPanel, Style};
use std::cell::RefCell;
use std::collections::HashMap;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::WindowsAndMessaging::IsWindow;

thread_local! {
    static PANELS: RefCell<HashMap<isize, GlassPanel>> = RefCell::new(HashMap::new());
}

/// Put glass under `hwnd`. True if Windows composes it (blur, rim and shadow); if not, the
/// window must paint its own surface (`glass::content_frame`).
pub fn attach(hwnd: isize, style: Style) -> bool {
    let panel = GlassPanel::attach(hwnd, style);
    let composed = panel.composed();
    PANELS.with(|p| p.borrow_mut().insert(hwnd, panel));
    composed
}

/// Whether `hwnd` has composed glass under it.
pub fn composed(hwnd: isize) -> bool {
    PANELS.with(|p| p.borrow().get(&hwnd).is_some_and(GlassPanel::composed))
}

/// Give `hwnd`'s glass this look; free when it is unchanged.
pub fn set_style(hwnd: isize, style: Style) {
    PANELS.with(|p| {
        if let Some(panel) = p.borrow().get(&hwnd) {
            panel.set_style(style);
        }
    });
}

/// Put `hwnd`'s glass on `shape` and also show `also` (see `GlassPanel::set_shape`).
pub fn set_shape(hwnd: isize, shape: Option<RECT>, also: Option<RECT>) {
    PANELS.with(|p| {
        if let Some(panel) = p.borrow().get(&hwnd) {
            panel.set_shape(shape, also);
        }
    });
}

/// Take the glass away from `hwnd` (its window is about to go).
pub fn detach(hwnd: isize) {
    // Dropped outside the borrow: dropping a panel sends its window messages.
    let panel = PANELS.with(|p| p.borrow_mut().remove(&hwnd));
    drop(panel);
}

/// Forget the panels of windows that no longer exist.
pub fn prune() {
    let gone: Vec<isize> = PANELS.with(|p| {
        p.borrow()
            .keys()
            .copied()
            .filter(|&h| !unsafe { IsWindow(Some(HWND(h as *mut _))) }.as_bool())
            .collect()
    });
    for hwnd in gone {
        detach(hwnd);
    }
}
