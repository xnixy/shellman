//! The bar windows on each monitor: a top bar everywhere and, per the settings, a dock. Keeps
//! them matched to the monitors as those come, go and change, reserves each monitor's space, and
//! places the windows in that monitor's pixels.
//!
//! Each bar window is an egui viewport, opened and closed by the host's frames (`app.rs`). A new
//! window starts hidden; once it exists it is found by its title, given its looks, placed, and
//! only then shown, so it never flashes in the wrong place or at the wrong scale.

use crate::monitors::layout::{self, Sizes, Slot};
use crate::reserve::{Mode, Reservation};
use crate::shape::surface;
use crate::{dock, fullscreen, place, shape, topbar};
use shellman_ui::glass::{self, Shadow, Style};
use windows::Win32::Foundation::RECT;

/// Space between the dock and the edge of the screen (or Explorer's taskbar) below it, and
/// between the dock and maximized windows above it, in points: the same as the top bar's
/// islands keep from the top edge.
const GAP: f32 = 12.0;

/// The top bar's glass: the same material as the dock's, on its islands (pills, each with its own
/// radius), with the dock's short, tight shadow so little of it falls on the windows below.
pub fn top_style() -> Style {
    let style = Style::from_theme(topbar::ISLAND_H / 2.0);
    Style {
        shadow: Shadow { offset_y: 3.0, blur: 14.0, ..style.shadow },
        ..style
    }
}

/// The bars' sizes for a dock `dock_height` points tall (it follows the icon size).
pub fn sizes(dock_height: f32) -> Sizes {
    Sizes {
        top: topbar::HEIGHT,
        dock: dock_height,
        above: dock::ABOVE,
        gap: GAP,
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Top,
    Dock,
}

/// One bar window.
pub struct View {
    pub id: egui::ViewportId,
    pub kind: Kind,
    pub title: String,
    pub hwnd: Option<isize>,
}

impl View {
    fn new(kind: Kind, device: &str) -> Self {
        let name = match kind {
            Kind::Top => "top bar",
            Kind::Dock => "dock",
        };
        Self {
            id: egui::ViewportId::from_hash_of((name, device)),
            kind,
            title: format!("shellman {name} {device}"),
            hwnd: None,
        }
    }
}

/// One monitor's bars.
struct Set {
    slot: Slot,
    top: View,
    dock: Option<View>,
    reservation: Option<Reservation>,
}

impl Set {
    fn new(slot: Slot) -> Self {
        Self {
            top: View::new(Kind::Top, &slot.device),
            dock: slot.dock_band.is_some().then(|| View::new(Kind::Dock, &slot.device)),
            slot,
            reservation: None,
        }
    }

    /// Both windows exist.
    fn ready(&self) -> bool {
        self.top.hwnd.is_some() && self.dock.as_ref().is_none_or(|d| d.hwnd.is_some())
    }

    /// The dock window spans its band (free when it is there already); its glass, inside it,
    /// follows the icons on its own.
    fn place_dock(&self, mode: Mode, sizes: &Sizes) {
        let (Some(dock), Some(r)) = (&self.dock, &self.reservation) else { return };
        if let (Some(hwnd), Some(band)) = (dock.hwnd, r.band) {
            place::put(hwnd, shifted(mode, layout::dock_rect(band, self.slot.scale, sizes), self.slot.scale, -1));
        }
    }
}

/// A test bar (`--no-shell`) can be moved in from the edges by `SHELLMAN_BAR_TEST_SHIFT`
/// points (the top bar down, the dock up, by `sign`), so it can be looked at clear of the real
/// bar, which sits on the same edges. Never for a bar that reserves space.
fn shifted(mode: Mode, rect: RECT, scale: f32, sign: i32) -> RECT {
    let shift = (mode == Mode::Off)
        .then(|| std::env::var("SHELLMAN_BAR_TEST_SHIFT").ok()?.parse::<f32>().ok())
        .flatten()
        .map_or(0, |pt| layout::px(pt, scale) * sign);
    RECT { top: rect.top + shift, bottom: rect.bottom + shift, ..rect }
}

pub struct Bars {
    mode: Mode,
    sets: Vec<Set>,
    /// The sizes the plan was made with.
    sizes: Sizes,
}

/// A bar window's top-left corner in physical screen pixels.
pub fn client_origin(hwnd: isize) -> Option<(i32, i32)> {
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;
    let mut r = RECT::default();
    unsafe { GetWindowRect(HWND(hwnd as *mut _), &mut r) }.ok()?;
    Some((r.left, r.top))
}

impl Bars {
    pub fn new(mode: Mode) -> Self {
        Self { mode, sets: Vec::new(), sizes: sizes(0.0) }
    }

    pub fn sizes(&self) -> Sizes {
        self.sizes
    }

    /// Match the bars to a new plan, made with `sizes`. Monitors are matched by device name, so a monitor that was
    /// moved or rescaled keeps its windows and is only placed again; one that went away has its
    /// windows closed (they are no longer declared) and its space given back.
    /// With `reassert`, monitors whose space is unchanged get their work areas put back if
    /// someone else changed them (the host's work-area timer decided it is time).
    pub fn update(&mut self, slots: Vec<Slot>, sizes: Sizes, reassert: bool) {
        self.sizes = sizes;
        let mut put_back = false;
        let mut old = std::mem::take(&mut self.sets);
        for slot in slots {
            let set = match old.iter().position(|s| s.slot.device == slot.device) {
                Some(i) => {
                    let mut set = old.remove(i);
                    if set.slot != slot {
                        // Give the old space back before the new is taken.
                        set.reservation = None;
                        if slot.dock_band.is_none() {
                            set.dock = None;
                        } else if set.dock.is_none() {
                            set.dock = Some(View::new(Kind::Dock, &slot.device));
                        }
                        set.slot = slot;
                    } else if let (true, Some(r)) = (reassert, &set.reservation) {
                        // Same monitor, same space, and someone else changed its work area.
                        put_back |= r.reassert();
                    }
                    set
                }
                None => Set::new(slot),
            };
            self.sets.push(set);
        }
        // Whatever is left in `old` is dropped here, releasing its space.
        if put_back {
            crate::workarea::trace("work areas put back");
            if let Ok(mut k) = crate::workarea::KEEPER.lock() {
                k.reasserted(std::time::Instant::now());
            }
        }
    }

    /// Every bar window, with the monitor it is on and whether that is the primary.
    pub fn views(&self) -> impl Iterator<Item = (&View, &str, bool)> {
        self.sets.iter().flat_map(|s| {
            std::iter::once(&s.top)
                .chain(s.dock.as_ref())
                .map(move |v| (v, s.slot.device.as_str(), s.slot.primary))
        })
    }

    pub fn ids(&self) -> Vec<egui::ViewportId> {
        self.views().map(|(v, _, _)| v.id).collect()
    }

    /// The top bar window on `device`, once it exists.
    pub fn top_hwnd(&self, device: &str) -> Option<isize> {
        self.sets.iter().find(|s| s.slot.device == device)?.top.hwnd
    }

    /// The dock window on `device`, once it exists.
    pub fn dock_hwnd(&self, device: &str) -> Option<isize> {
        self.sets.iter().find(|s| s.slot.device == device)?.dock.as_ref()?.hwnd
    }

    /// Find windows that were just created and give them their looks: glass under each, the
    /// dock's in `dock_style`. True when every bar window exists.
    pub fn adopt(&mut self, dock_style: glass::Style) -> bool {
        surface::prune();
        let mut all = true;
        for set in &mut self.sets {
            for view in std::iter::once(&mut set.top).chain(set.dock.as_mut()) {
                if view.hwnd.is_some() {
                    continue;
                }
                let Some(hwnd) = shape::own_window(&view.title) else {
                    all = false;
                    continue;
                };
                view.hwnd = Some(hwnd);
                shape::never_activate(hwnd);
                shape::see_through(hwnd);
                let style = match view.kind {
                    Kind::Top => top_style(),
                    Kind::Dock => dock_style,
                };
                surface::attach(hwnd, style);
            }
        }
        all
    }

    /// Reserve space and place windows where that is still to do, then show what is ready
    /// (unless a fullscreen app covers its monitor).
    pub fn settle(&mut self) {
        for set in self.sets.iter_mut().filter(|s| s.ready()) {
            if set.reservation.is_none() {
                let dock = set.dock.as_ref().and_then(|d| d.hwnd);
                set.reservation = Some(Reservation::new(self.mode, &set.slot, set.top.hwnd.unwrap_or(0), dock, &self.sizes));
            }
            if let (Some(hwnd), Some(r)) = (set.top.hwnd, &set.reservation) {
                place::put(hwnd, shifted(self.mode, r.top, set.slot.scale, 1));
            }
            set.place_dock(self.mode, &self.sizes);
        }
        let groups = self
            .sets
            .iter()
            .filter(|s| s.ready() && s.reservation.is_some())
            .map(|s| std::iter::once(&s.top).chain(s.dock.as_ref()).filter_map(|v| v.hwnd).collect())
            .collect();
        fullscreen::set_groups(groups);
    }

    /// Explorer said app bar positions changed; ask again and move what moved.
    pub fn requery(&mut self) {
        for set in &mut self.sets {
            if let Some(r) = &mut set.reservation {
                // What moved is placed again by the next `settle`.
                r.requery(&set.slot, &self.sizes);
            }
        }
    }

    /// Give every monitor's space back (the bar is exiting).
    pub fn release(&mut self) {
        for set in &mut self.sets {
            set.reservation = None;
        }
    }
}
