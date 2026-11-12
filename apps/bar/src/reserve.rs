//! One monitor's reserved space: its top bar's strip and, with a dock, the dock's band. Next to
//! Explorer they are app bars; as the shell, the monitor's work area. Dropping it gives the
//! space back.

use crate::appbar::{AppBar, Edge};
use crate::monitors::layout::{self, Slot, Sizes};
use crate::workarea::WorkArea;
use windows::Win32::Foundation::RECT;

/// How space is reserved, decided once at start.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// Explorer runs and keeps the app bars.
    Explorer,
    /// shellman is the shell: set work areas directly.
    Shell,
    /// A test run beside the real bar (`--no-shell`): reserve nothing.
    Off,
}

pub struct Reservation {
    _top: Option<AppBar>,
    dock: Option<AppBar>,
    _work: Option<WorkArea>,
    /// Where the top bar goes and where the dock's band is, as granted.
    pub top: RECT,
    pub band: Option<RECT>,
}

impl Reservation {
    /// Reserve `slot`'s space. `top_hwnd` and `dock_hwnd` are the windows Explorer will send its
    /// notices to.
    pub fn new(mode: Mode, slot: &Slot, top_hwnd: isize, dock_hwnd: Option<isize>, sizes: &Sizes) -> Self {
        let mut r = Self { _top: None, dock: None, _work: None, top: slot.top, band: slot.dock_band };
        let height = |rect: RECT| rect.bottom - rect.top;
        match mode {
            Mode::Off => {}
            Mode::Shell => {
                let bottom = slot.dock_band.map_or(0, height);
                r._work = Some(WorkArea::reserve(slot.monitor, height(slot.top), bottom));
            }
            Mode::Explorer => {
                r._top = AppBar::new(top_hwnd, Edge::Top, slot.top);
                if let Some(bar) = &r._top {
                    r.top = bar.granted;
                }
                if let (Some(band), Some(hwnd)) = (slot.dock_band, dock_hwnd) {
                    r.dock = AppBar::new(hwnd, Edge::Bottom, band);
                    if let Some(bar) = &r.dock {
                        // Explorer may have moved it up, above its own taskbar; the band keeps
                        // its height and ends where Explorer's space begins.
                        r.band = Some(layout::dock_band(slot.monitor, bar.granted.bottom, slot.scale, sizes));
                    }
                }
                if r._top.is_none() {
                    // Nobody answered: Explorer is gone after all.
                    let bottom = slot.dock_band.map_or(0, height);
                    r._work = Some(WorkArea::reserve(slot.monitor, height(slot.top), bottom));
                }
            }
        }
        r
    }

    /// Explorer said bar positions changed: ask again. True if anything moved.
    pub fn requery(&mut self, slot: &Slot, sizes: &Sizes) -> bool {
        let mut moved = false;
        if let Some(bar) = &mut self._top {
            if bar.requery() {
                self.top = bar.granted;
                moved = true;
            }
        }
        if let Some(bar) = &mut self.dock {
            if bar.requery() {
                self.band = Some(layout::dock_band(slot.monitor, bar.granted.bottom, slot.scale, sizes));
                moved = true;
            }
        }
        moved
    }
}
