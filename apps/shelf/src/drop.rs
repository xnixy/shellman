//! Dropping onto the shelf: files go into the folder the settings name (the Desktop unless
//! changed), through the shell's file operation; a dropped picture or text becomes a new file
//! there. While a drag that the shelf would take hovers over it, the drop zone says "Keep".
//!
//! The window's region is its islands, so OLE only asks about points over the shelf (and an
//! open fan); the fan's pills take nothing. What a drop does is decided here, purely.

use shellman_core::transfer::target::{Answer, Handler};
use shellman_core::transfer::{self, Offer, Payload, NONE};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// What a drop of `offer` into `dest` does with `keys` held, `allowed` by the source. Files
/// follow Explorer's rules (a move on the same volume, a copy across volumes; Ctrl copies, Shift
/// moves, Alt makes shortcuts); a picture or text from a browser is written as a new file.
/// Anything else (a browser's virtual files without a real path) is not taken.
pub fn decide(offer: &Offer, dest: &Path, keys: u32, allowed: u32) -> u32 {
    if !offer.files.is_empty() {
        transfer::choose(&offer.files, dest, keys, allowed)
    } else if offer.image || offer.text {
        transfer::choose_content(allowed)
    } else {
        NONE
    }
}

/// What the frame and the drop target share.
#[derive(Default)]
pub struct Shared {
    /// The shelf island on the screen (pixels), where drops are taken.
    pub zone: Option<(i32, i32, i32, i32)>,
    /// Where drops go, and its name for the drag image.
    pub dest: Option<(PathBuf, String)>,
    /// A drag the shelf would take is over it.
    pub hovering: bool,
    /// Drops taken, waiting for the frame to carry them out.
    pub dropped: Vec<(Payload, u32)>,
    offer: Offer,
}

impl Shared {
    fn answer(&self, offer: &Offer, pt: (i32, i32), keys: u32, allowed: u32) -> Answer {
        let inside = self.zone.is_some_and(|z| !shellman_core::outside::outside(pt.0, pt.1, z));
        match (&self.dest, inside) {
            (Some((dir, name)), true) => Answer { effect: decide(offer, dir, keys, allowed), label: Some(name.clone()) },
            _ => Answer::default(),
        }
    }
}

pub struct Target {
    pub shared: Arc<Mutex<Shared>>,
    pub ctx: egui::Context,
}

impl Target {
    fn update(&self, offer: Option<&Offer>, pt: (i32, i32), keys: u32, allowed: u32) -> Answer {
        let mut s = self.shared.lock().unwrap();
        if let Some(o) = offer {
            s.offer = o.clone();
        }
        let a = s.answer(&s.offer.clone(), pt, keys, allowed);
        let hovering = a.effect != NONE;
        if s.hovering != hovering {
            s.hovering = hovering;
            self.ctx.request_repaint_of(egui::ViewportId::ROOT);
        }
        a
    }
}

impl Handler for Target {
    fn enter(&self, offer: &Offer, pt: (i32, i32), keys: u32, allowed: u32) -> Answer {
        self.update(Some(offer), pt, keys, allowed)
    }

    fn over(&self, pt: (i32, i32), keys: u32, allowed: u32) -> Answer {
        self.update(None, pt, keys, allowed)
    }

    fn leave(&self) {
        let mut s = self.shared.lock().unwrap();
        s.hovering = false;
        s.offer = Offer::default();
        self.ctx.request_repaint_of(egui::ViewportId::ROOT);
    }

    fn dropped(&self, payload: Payload, pt: (i32, i32), keys: u32, allowed: u32) -> u32 {
        let mut s = self.shared.lock().unwrap();
        let offer = match &payload {
            Payload::Files(f) => Offer { files: f.clone(), ..Default::default() },
            Payload::Image(_) => Offer { image: true, ..Default::default() },
            Payload::Text(_) => Offer { text: true, ..Default::default() },
            Payload::Nothing => Offer::default(),
        };
        let effect = s.answer(&offer, pt, keys, allowed).effect;
        s.hovering = false;
        s.offer = Offer::default();
        if effect != NONE {
            s.dropped.push((payload, effect));
        }
        self.ctx.request_repaint_of(egui::ViewportId::ROOT);
        effect
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shellman_core::transfer::{COPY, LINK, MK_CONTROL, MOVE};

    const ALL: u32 = COPY | MOVE | LINK;

    fn files(p: &[&str]) -> Offer {
        Offer { files: p.iter().map(PathBuf::from).collect(), ..Default::default() }
    }

    #[test]
    fn files_move_on_the_same_volume_and_copy_across() {
        let desk = Path::new(r"C:\Users\a\Desktop");
        assert_eq!(decide(&files(&[r"C:\Users\a\Downloads\x.zip"]), desk, 0, ALL), MOVE);
        assert_eq!(decide(&files(&[r"E:\stick\x.zip"]), desk, 0, ALL), COPY);
        assert_eq!(decide(&files(&[r"C:\Users\a\Downloads\x.zip"]), desk, MK_CONTROL, ALL), COPY);
        // Already there, or the Desktop itself.
        assert_eq!(decide(&files(&[r"C:\Users\a\Desktop\x.zip"]), desk, 0, ALL), NONE);
        assert_eq!(decide(&files(&[r"C:\Users\a\Desktop"]), desk, 0, ALL), NONE);
    }

    #[test]
    fn pictures_and_text_are_written_and_the_rest_refused() {
        let desk = Path::new(r"C:\Users\a\Desktop");
        assert_eq!(decide(&Offer { image: true, ..Default::default() }, desk, 0, ALL), COPY);
        assert_eq!(decide(&Offer { text: true, ..Default::default() }, desk, 0, MOVE), NONE);
        assert_eq!(decide(&Offer::default(), desk, 0, ALL), NONE);
    }

    #[test]
    fn only_points_on_the_shelf_take_the_drop() {
        let s = Shared {
            zone: Some((0, 100, 150, 700)),
            dest: Some((PathBuf::from(r"C:\Users\a\Desktop"), "Desktop".into())),
            ..Default::default()
        };
        let o = files(&[r"C:\Users\a\Downloads\x.zip"]);
        let a = s.answer(&o, (50, 300), 0, ALL);
        assert_eq!((a.effect, a.label.as_deref()), (MOVE, Some("Desktop")));
        assert_eq!(s.answer(&o, (500, 300), 0, ALL).effect, NONE);
        let nowhere = Shared { zone: Some((0, 0, 10, 10)), ..Default::default() };
        assert_eq!(nowhere.answer(&o, (5, 5), 0, ALL).effect, NONE);
    }
}
