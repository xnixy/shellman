//! What the Display page shows and asks for. Plain data: the worker fills it in, the page draws it.

use super::geom::{self, Rect};
use super::modes::Group;
use super::scale::Scale;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    Landscape,
    Portrait,
    LandscapeFlipped,
    PortraitFlipped,
}

impl Orientation {
    pub const ALL: [Orientation; 4] =
        [Orientation::Landscape, Orientation::Portrait, Orientation::LandscapeFlipped, Orientation::PortraitFlipped];

    /// The DMDO_* value GDI uses.
    pub fn dmdo(self) -> u32 {
        self as u32
    }

    pub fn from_dmdo(v: u32) -> Self {
        Self::ALL.get(v as usize).copied().unwrap_or(Orientation::Landscape)
    }

    pub fn is_portrait(self) -> bool {
        matches!(self, Orientation::Portrait | Orientation::PortraitFlipped)
    }

    pub fn label(self) -> &'static str {
        match self {
            Orientation::Landscape => "Landscape",
            Orientation::Portrait => "Portrait",
            Orientation::LandscapeFlipped => "Landscape (flipped)",
            Orientation::PortraitFlipped => "Portrait (flipped)",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hdr {
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Monitor {
    /// The GDI device name, `\\.\DISPLAY1`.
    pub id: String,
    pub number: u32,
    pub name: String,
    /// Position and size on the desktop, in pixels, as the monitor is now.
    pub rect: Rect,
    pub orientation: Orientation,
    pub hz: u32,
    pub primary: bool,
    /// Resolutions with their rates, in the monitor's current orientation.
    pub groups: Vec<Group>,
    pub scale: Option<Scale>,
    /// Present only where the monitor and driver support advanced colour (HDR).
    pub hdr: Option<Hdr>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Topology {
    Extend,
    Duplicate,
    /// One screen only, or a mix this page does not name.
    Other,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Page {
    pub monitors: Vec<Monitor>,
    pub topology: Option<Topology>,
    /// Set while a change awaits "Keep these display settings?".
    pub pending: Option<Instant>,
    /// What the last action said, when it was a refusal or a failure.
    pub notice: Option<String>,
}

/// One monitor as the user staged it.
#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    pub id: String,
    pub x: i32,
    pub y: i32,
    /// The resolution as the monitor shows it in the staged orientation.
    pub w: u32,
    pub h: u32,
    pub hz: u32,
    pub orientation: Orientation,
}

impl Change {
    pub fn of(m: &Monitor) -> Change {
        Change { id: m.id.clone(), x: m.rect.x, y: m.rect.y, w: m.rect.w as u32, h: m.rect.h as u32, hz: m.hz, orientation: m.orientation }
    }

    pub fn differs_from(&self, m: &Monitor) -> bool {
        *self != Change::of(m)
    }

    /// The rectangle this change would give the monitor.
    pub fn rect(&self) -> Rect {
        Rect::new(self.x, self.y, self.w as i32, self.h as i32)
    }
}

/// The staged state turned into a full set of changes: sizes follow the orientation (turning a
/// monitor a quarter swaps its sides) and everything is shifted so the chosen primary is at 0,0.
pub fn plan(monitors: &[Monitor], staged: &[Change], primary: &str) -> Vec<Change> {
    let mut out: Vec<Change> = monitors
        .iter()
        .map(|m| {
            let mut c = staged.iter().find(|c| c.id == m.id).cloned().unwrap_or_else(|| Change::of(m));
            if c.orientation.is_portrait() != m.orientation.is_portrait() {
                std::mem::swap(&mut c.w, &mut c.h);
            }
            c
        })
        .collect();
    let mut rects: Vec<Rect> = out.iter().map(Change::rect).collect();
    if let Some(p) = out.iter().position(|c| c.id == primary) {
        geom::normalise(&mut rects, p);
        for (c, r) in out.iter_mut().zip(&rects) {
            c.x = r.x;
            c.y = r.y;
        }
    }
    out
}

/// Whether a plan is an arrangement Windows accepts.
pub fn plan_valid(changes: &[Change]) -> bool {
    let rects: Vec<Rect> = changes.iter().map(Change::rect).collect();
    geom::valid(&rects)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mon(id: &str, x: i32, y: i32, primary: bool) -> Monitor {
        Monitor {
            id: id.into(),
            number: 1,
            name: id.into(),
            rect: Rect::new(x, y, 1920, 1080),
            orientation: Orientation::Landscape,
            hz: 60,
            primary,
            groups: vec![],
            scale: None,
            hdr: None,
        }
    }

    #[test]
    fn an_untouched_page_plans_no_change() {
        let m = vec![mon("a", 0, 0, true), mon("b", 1920, 0, false)];
        let p = plan(&m, &[], "a");
        assert!(p.iter().zip(&m).all(|(c, m)| !c.differs_from(m)));
        assert!(plan_valid(&p));
    }

    #[test]
    fn a_new_primary_moves_everything_so_it_sits_at_the_origin() {
        let m = vec![mon("a", 0, 0, true), mon("b", 1920, 0, false)];
        let p = plan(&m, &[], "b");
        assert_eq!((p[1].x, p[1].y), (0, 0));
        assert_eq!((p[0].x, p[0].y), (-1920, 0));
    }

    #[test]
    fn turning_a_monitor_swaps_its_sides() {
        let m = vec![mon("a", 0, 0, true)];
        let mut c = Change::of(&m[0]);
        c.orientation = Orientation::Portrait;
        let p = plan(&m, &[c], "a");
        assert_eq!((p[0].w, p[0].h), (1080, 1920));
    }

    #[test]
    fn overlapping_staged_positions_are_not_valid() {
        let m = vec![mon("a", 0, 0, true), mon("b", 1920, 0, false)];
        let mut c = Change::of(&m[1]);
        c.x = 1000;
        assert!(!plan_valid(&plan(&m, &[c], "a")));
    }

    #[test]
    fn orientations_round_trip_through_gdi_values() {
        for o in Orientation::ALL {
            assert_eq!(Orientation::from_dmdo(o.dmdo()), o);
        }
    }
}
