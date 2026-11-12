//! Floating glass: a rounded surface of any radius with real blur, a soft shadow and a light rim,
//! under a window whose content egui draws on a transparent background.
//!
//! What was measured (Windows 11 build 26300, `examples/glass_probe.rs`, screen captures at 150%
//! and 300%):
//!
//! - DWM's system backdrop (`DWMWA_SYSTEMBACKDROP_TYPE`, what `window::frost_window` uses) paints
//!   the whole window rectangle; a rounded window region does not clip it. On a window that is
//!   never active it is a flat grey, not glass.
//! - The undocumented accent policy (`SetWindowCompositionAttribute`, acrylic blur-behind) does
//!   blur, but also over the whole rectangle: the rounded region is ignored there too.
//! - Windows.UI.Composition works: the host backdrop brush (the blurred desktop behind the window)
//!   clipped by a rounded-rectangle geometry gives real blur with smooth, anti-aliased corners at
//!   any radius, and a drop shadow masked by the same shape. Its visuals cannot go under an egui
//!   window's own content, and its shadow needs room outside the shape, so they live in a second
//!   window just below.
//!
//! So a panel is two windows: the served window keeps its size, loses Windows' corners, border
//! and backdrop, and gets a rounded window region (so clicks outside the shape fall through); a
//! backdrop window of its own class sits right under it, shape plus shadow, layered and
//! transparent to the mouse, and shows only the composition tree. A subclass on the served window
//! moves, sizes, shows and hides the backdrop in the same message that moved the window; screen
//! captures taken while a window moved every 10 ms found the two never apart. During a live resize
//! about one capture in forty showed them one step apart.
//!
//! When transparency effects are off, the tree fills the shape with a solid colour instead of the
//! blur (rounded corners and shadow stay). When composition is not available at all (or
//! `SHELLMAN_NO_COMPOSITION` is set, to see this path), the window paints its own rounded surface
//! with [`content_frame`], without blur or shadow.
//!
//! Using it: attach a panel once the window exists, paint the content inside
//! [`content_frame`]`(panel.composed(), ..)`, and pass the theme's style to
//! [`GlassPanel::set_style`] when it may have changed. Everything else follows on its own,
//! except cloaking, which sends no message: cloak the window with [`cloak`], which takes its
//! glass along (from any thread).
//!
//! The surface fills the window unless [`GlassPanel::set_shape`] puts it on a part of it. A
//! window whose surface changes size often (the dock, as apps come and go) can stay one size and
//! change the shape instead: a resized window shows its old content for a frame, and a resized
//! backdrop window shows its old visuals, while a shape changed inside both windows reaches the
//! screen with the window's next frame.

mod backdrop;
mod cloak;
mod style;
mod tree;

pub use cloak::{cloak, is_cloaked};
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
use windows::Win32::Graphics::Gdi::{
    CombineRgn, CreateRectRgn, CreateRoundRectRgn, DeleteObject, SetWindowRgn, RGN_OR,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyWindow, GetWindowRect, IsIconic, IsWindowVisible, PostMessageW, RegisterWindowMessageW,
    SetWindowPos, SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
    SWP_SHOWWINDOW, WM_DPICHANGED, WM_DWMCOMPOSITIONCHANGED, WM_NCDESTROY, WM_WINDOWPOSCHANGED,
};
use windows::UI::ViewManagement::UISettings;

/// The subclass id on the served window.
const SUBCLASS: usize = 0x6c61_7373;

/// A rounded glass surface under one window. Dropping it removes the surface.
pub struct GlassPanel {
    inner: Rc<Inner>,
}

struct Inner {
    /// The window the panel serves, whose content sits on top.
    target: HWND,
    /// The backdrop window and its visuals; None when Windows cannot compose them.
    composed: Option<Composed>,
    style: Cell<Style>,
    /// Where the surface is, in the window's pixels from its top-left; None for all of it.
    shape: Cell<Option<RECT>>,
    /// A part of the window outside the surface that still shows and takes clicks.
    also: Cell<Option<RECT>>,
    /// What the shape was last laid out for: the window's size, the shape, the extra part and
    /// the DPI. None forces a layout.
    laid_out: Cell<Option<Layout>>,
    /// Whether Windows' transparency effects are on.
    effects: Cell<bool>,
    /// False once the served window is destroyed (and the backdrop with it).
    alive: Cell<bool>,
}

#[derive(Clone, Copy, PartialEq)]
struct Layout {
    window: (i32, i32),
    shape: RECT,
    also: Option<RECT>,
    dpi: u32,
}

struct Composed {
    window: HWND,
    tree: Tree,
    /// Tells the panel when transparency effects are turned on or off.
    settings: Option<(UISettings, i64)>,
}

impl GlassPanel {
    /// Put a rounded surface under `hwnd`, a top-level window of this thread. The window loses
    /// Windows' own corners, border and backdrop, and gets a rounded window region so clicks
    /// outside the shape fall through. See [`GlassPanel::composed`] for whether there is glass.
    pub fn attach(hwnd: isize, style: Style) -> Self {
        let target = HWND(hwnd as *mut _);
        bare(target);
        let inner = Rc::new(Inner {
            target,
            composed: compose(hwnd),
            style: Cell::new(style),
            shape: Cell::new(None),
            also: Cell::new(None),
            laid_out: Cell::new(None),
            effects: Cell::new(effects_enabled()),
            alive: Cell::new(true),
        });
        if let Some(composed) = &inner.composed {
            cloak::remember(target, composed.window);
        }
        unsafe {
            let _ = SetWindowSubclass(
                target,
                Some(follow_proc),
                SUBCLASS,
                Rc::as_ptr(&inner) as usize,
            );
        }
        inner.follow();
        Self { inner }
    }

    /// Whether the surface is drawn under the window (blur, tint, rim and shadow). If not, the
    /// window paints it itself: see [`content_frame`].
    pub fn composed(&self) -> bool {
        self.inner.composed.is_some()
    }

    /// Change the look (the theme, accent or glass amount changed). Free when nothing changed.
    pub fn set_style(&self, style: Style) {
        if self.inner.style.replace(style) != style {
            self.inner.laid_out.set(None);
            self.inner.follow();
        }
    }

    /// Put the surface on `shape` (in the window's pixels, from its top-left) instead of the
    /// whole window; None gives it the whole window again. `also` is another part of the window
    /// that shows and takes clicks without glass under it (a label above the surface); the rest
    /// of the window is neither drawn nor clicked. Free when nothing changed.
    pub fn set_shape(&self, shape: Option<RECT>, also: Option<RECT>) {
        let changed = self.inner.shape.replace(shape) != shape;
        if (self.inner.also.replace(also) != also) | changed {
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
        if let Some(composed) = &inner.composed {
            if let Some((settings, token)) = &composed.settings {
                let _ = settings.RemoveAdvancedEffectsEnabledChanged(*token);
            }
        }
        if !inner.alive.get() {
            return;
        }
        cloak::forget(inner.target);
        unsafe {
            let _ = RemoveWindowSubclass(inner.target, Some(follow_proc), SUBCLASS);
            if let Some(composed) = &inner.composed {
                let _ = DestroyWindow(composed.window);
            }
            let _ = SetWindowRgn(inner.target, None, true);
        }
    }
}

/// The backdrop window and its visuals, if Windows can compose them.
fn compose(hwnd: isize) -> Option<Composed> {
    if std::env::var_os("SHELLMAN_NO_COMPOSITION").is_some() {
        return None;
    }
    let compositor = tree::compositor()?;
    let window = backdrop::create()?;
    match Tree::new(&compositor, window) {
        Ok(tree) => Some(Composed {
            window,
            tree,
            settings: watch_effects(hwnd),
        }),
        Err(_) => {
            unsafe {
                let _ = DestroyWindow(window);
            }
            None
        }
    }
}

impl Inner {
    /// Match the window: its rounded region, and the backdrop in the same place, at the same
    /// visibility, right under it.
    fn follow(&self) {
        unsafe {
            let visible =
                IsWindowVisible(self.target).as_bool() && !IsIconic(self.target).as_bool();
            if !visible {
                if let Some(composed) = &self.composed {
                    let flags = SWP_HIDEWINDOW | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER;
                    let _ = SetWindowPos(composed.window, None, 0, 0, 0, 0, flags | SWP_NOACTIVATE);
                }
                return;
            }
            let mut rc = RECT::default();
            if GetWindowRect(self.target, &mut rc).is_err() {
                return;
            }
            let window = (rc.right - rc.left, rc.bottom - rc.top);
            let area = self.shape.get().unwrap_or(RECT {
                left: 0,
                top: 0,
                right: window.0,
                bottom: window.1,
            });
            let shape = (area.right - area.left, area.bottom - area.top);
            let dpi = GetDpiForWindow(self.target).max(96);
            let style = self.style.get();
            let mut geo = Geometry::new(shape, dpi as f32 / 96.0, &style);
            // The backdrop window covers the whole served window (and the shadow's room around
            // it), and the shape moves inside it. So a shape that changes every frame (the dock
            // growing as an app opens) only changes the visuals, which land on screen together
            // with the window's own next frame; moving the backdrop window as well would show it
            // a frame ahead of them.
            geo.left += area.left;
            geo.top += area.top;
            geo.right += window.0 - area.right;
            geo.bottom += window.1 - area.bottom;
            let layout = Layout { window, shape: area, also: self.also.get(), dpi };
            let before = self.laid_out.replace(Some(layout));
            if before != Some(layout) {
                let moved = before.is_none_or(|b| (b.window, b.shape, b.dpi) != (window, area, dpi));
                if let Some(composed) = self.composed.as_ref().filter(|_| moved) {
                    let _ = composed.tree.apply(&geo, &style, self.effects.get());
                }
                // This sends the window another move message, which finds the layout done.
                let r = geo.region_radius() * 2;
                let region = CreateRoundRectRgn(area.left, area.top, area.right + 1, area.bottom + 1, r, r);
                if let Some(also) = layout.also {
                    let extra = CreateRectRgn(also.left, also.top, also.right, also.bottom);
                    CombineRgn(Some(region), Some(region), Some(extra), RGN_OR);
                    let _ = DeleteObject(extra.into());
                }
                SetWindowRgn(self.target, Some(region), true);
            }
            if let Some(composed) = &self.composed {
                let (w, h) = geo.window();
                let _ = SetWindowPos(
                    composed.window,
                    Some(self.target),
                    rc.left - (geo.left - area.left),
                    rc.top - (geo.top - area.top),
                    w,
                    h,
                    SWP_SHOWWINDOW | SWP_NOACTIVATE,
                );
            }
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
            inner.alive.set(false);
            cloak::forget(hwnd);
            let _ = RemoveWindowSubclass(hwnd, Some(follow_proc), SUBCLASS);
            if let Some(composed) = &inner.composed {
                let _ = DestroyWindow(composed.window);
            }
            DefSubclassProc(hwnd, msg, wparam, lparam)
        }
        // DWM restarted (Windows tells every top-level window): lay the surface out and colour
        // it again, in case the new DWM missed what was set before.
        WM_DWMCOMPOSITIONCHANGED => {
            inner.laid_out.set(None);
            inner.follow();
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
