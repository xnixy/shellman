//! Floating glass: a rounded surface of any radius with real blur, a soft shadow and a light rim,
//! under a window whose content egui draws on a transparent background.
//!
//! MEASUREMENTS (to be filled in).
//!
//! Using it: attach a panel to the window once it exists, draw the window's content with
//! [`content_frame`] (transparent while the panel is composed), and call
//! [`GlassPanel::set_style`] when the theme changes. The panel follows the window's moves,
//! resizes, showing, hiding and DPI on its own.

mod backdrop;
mod style;
mod tree;

pub use style::{content_frame, Geometry, Shadow, Style};

use std::cell::Cell;
use std::rc::Rc;
use std::sync::OnceLock;
use tree::Tree;
use windows::core::w;
use windows::Foundation::TypedEventHandler;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMSBT_NONE, DWMWA_BORDER_COLOR, DWMWA_SYSTEMBACKDROP_TYPE,
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND, DWMWINDOWATTRIBUTE,
};
use windows::Win32::Graphics::Gdi::{CreateRoundRectRgn, SetWindowRgn};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyWindow, GetWindowRect, IsIconic, IsWindowVisible, PostMessageW, RegisterWindowMessageW,
    SetWindowPos, SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
    SWP_SHOWWINDOW, WM_DPICHANGED, WM_NCDESTROY, WM_WINDOWPOSCHANGED,
};
use windows::UI::ViewManagement::UISettings;

/// The subclass id on the served window.
const SUBCLASS: usize = 0x6c61_7373;

/// A glass surface under one window. Dropping it removes the surface.
pub struct GlassPanel {
    inner: Rc<Inner>,
}

struct Inner {
    /// The window the panel serves, whose content sits on top.
    target: HWND,
    /// The backdrop window under it.
    window: HWND,
    tree: Tree,
    style: Cell<Style>,
    /// What the tree was last laid out for: the shape's size and the DPI. None forces a layout.
    laid_out: Cell<Option<((i32, i32), u32)>>,
    /// Whether Windows' transparency effects are on.
    effects: Cell<bool>,
    /// Tells the panel when transparency effects are turned on or off.
    settings: Option<(UISettings, i64)>,
}

impl GlassPanel {
    /// Put glass under `hwnd` (a top-level window of this thread). None when Windows cannot
    /// compose it; the window then paints its own surface, see [`content_frame`].
    ///
    /// The window loses Windows' own corners, border and backdrop, and gets a rounded window
    /// region so that clicks outside the shape fall through.
    pub fn attach(hwnd: isize, style: Style) -> Option<Self> {
        let target = HWND(hwnd as *mut _);
        let compositor = tree::compositor()?;
        let window = backdrop::create()?;
        let tree = match Tree::new(&compositor, window) {
            Ok(tree) => tree,
            Err(_) => {
                unsafe {
                    let _ = DestroyWindow(window);
                }
                return None;
            }
        };
        bare(target);
        let inner = Rc::new(Inner {
            target,
            window,
            tree,
            style: Cell::new(style),
            laid_out: Cell::new(None),
            effects: Cell::new(effects_enabled()),
            settings: watch_effects(hwnd),
        });
        unsafe {
            let _ = SetWindowSubclass(
                target,
                Some(follow_proc),
                SUBCLASS,
                Rc::as_ptr(&inner) as usize,
            );
        }
        inner.follow();
        Some(Self { inner })
    }

    /// Change the look (the theme, accent or glass amount changed). Free when nothing changed.
    pub fn set_style(&self, style: Style) {
        if self.inner.style.replace(style) != style {
            self.inner.laid_out.set(None);
            self.inner.follow();
        }
    }

    /// The look the panel has now.
    pub fn style(&self) -> Style {
        self.inner.style.get()
    }
}

impl Drop for GlassPanel {
    fn drop(&mut self) {
        let inner = &self.inner;
        unsafe {
            let _ = RemoveWindowSubclass(inner.target, Some(follow_proc), SUBCLASS);
            let _ = DestroyWindow(inner.window);
            let _ = SetWindowRgn(inner.target, None, true);
        }
        if let Some((settings, token)) = &inner.settings {
            let _ = settings.RemoveAdvancedEffectsEnabledChanged(*token);
        }
    }
}

impl Inner {
    /// Put the backdrop where the window is: same place, same visibility, right under it.
    fn follow(&self) {
        unsafe {
            if !IsWindowVisible(self.target).as_bool() || IsIconic(self.target).as_bool() {
                let _ = SetWindowPos(
                    self.window,
                    None,
                    0,
                    0,
                    0,
                    0,
                    SWP_HIDEWINDOW | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
                return;
            }
            let mut rc = RECT::default();
            if GetWindowRect(self.target, &mut rc).is_err() {
                return;
            }
            let shape = (rc.right - rc.left, rc.bottom - rc.top);
            let dpi = GetDpiForWindow(self.target).max(96);
            let style = self.style.get();
            let geo = Geometry::new(shape, dpi as f32 / 96.0, &style);
            if self.laid_out.replace(Some((shape, dpi))) != Some((shape, dpi)) {
                let _ = self.tree.apply(&geo, &style, self.effects.get());
                // This sends the window another move message, which finds the layout done.
                let r = geo.region_radius() * 2;
                let region = CreateRoundRectRgn(0, 0, shape.0 + 1, shape.1 + 1, r, r);
                SetWindowRgn(self.target, Some(region), true);
            }
            let (w, h) = geo.window();
            let _ = SetWindowPos(
                self.window,
                Some(self.target),
                rc.left - geo.left,
                rc.top - geo.top,
                w,
                h,
                SWP_SHOWWINDOW | SWP_NOACTIVATE,
            );
        }
    }
}

/// Take away what Windows draws around and behind the window: its corners, its border and any
/// system backdrop. The panel draws all of that.
fn bare(hwnd: HWND) {
    let set = |attribute: DWMWINDOWATTRIBUTE, value: u32| unsafe {
        let _ = DwmSetWindowAttribute(hwnd, attribute, (&raw const value).cast(), 4);
    };
    set(DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND.0 as u32);
    // DWMWA_COLOR_NONE.
    set(DWMWA_BORDER_COLOR, 0xFFFF_FFFE);
    set(DWMWA_SYSTEMBACKDROP_TYPE, DWMSBT_NONE.0 as u32);
}

/// Whether Windows' transparency effects are on (Settings, Personalization, Colors). Assumed on
/// when Windows cannot say.
pub fn effects_enabled() -> bool {
    UISettings::new()
        .and_then(|s| s.AdvancedEffectsEnabled())
        .unwrap_or(true)
}

/// The message the served window gets when transparency effects are turned on or off.
fn effects_message() -> u32 {
    static MESSAGE: OnceLock<u32> = OnceLock::new();
    *MESSAGE.get_or_init(|| unsafe { RegisterWindowMessageW(w!("shellman-glass-effects")) })
}

/// Have Windows say when transparency effects change. Its callback comes on another thread, so
/// it only posts a message to the window.
fn watch_effects(hwnd: isize) -> Option<(UISettings, i64)> {
    let settings = UISettings::new().ok()?;
    let message = effects_message();
    let token = settings
        .AdvancedEffectsEnabledChanged(&TypedEventHandler::new(move |_, _| {
            unsafe {
                let _ = PostMessageW(Some(HWND(hwnd as *mut _)), message, WPARAM(0), LPARAM(0));
            }
            Ok(())
        }))
        .ok()?;
    Some((settings, token))
}

unsafe extern "system" fn follow_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    let inner = &*(data as *const Inner);
    match msg {
        WM_WINDOWPOSCHANGED => {
            inner.follow();
            DefSubclassProc(hwnd, msg, wparam, lparam)
        }
        WM_DPICHANGED => {
            let result = DefSubclassProc(hwnd, msg, wparam, lparam);
            inner.follow();
            result
        }
        WM_NCDESTROY => {
            let _ = RemoveWindowSubclass(hwnd, Some(follow_proc), SUBCLASS);
            let _ = DestroyWindow(inner.window);
            DefSubclassProc(hwnd, msg, wparam, lparam)
        }
        _ if msg == effects_message() => {
            inner.effects.set(effects_enabled());
            inner.laid_out.set(None);
            inner.follow();
            LRESULT(0)
        }
        _ => DefSubclassProc(hwnd, msg, wparam, lparam),
    }
}
