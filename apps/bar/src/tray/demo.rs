//! Stand-in tray icons for the `--no-shell` test bar, which hosts no tray: a few plain coloured
//! shapes, so the top bar's tray styling can be looked at. They have no owner, so clicks do nothing.

use super::TrayIcon;
use std::sync::Arc;

/// Big enough to stay sharp on a 300% monitor (16 pt is 48 px there).
const SIZE: usize = 48;

pub fn icons() -> Vec<TrayIcon> {
    [([86, 156, 214], "Cloud"), ([232, 168, 56], "Sync"), ([106, 190, 120], "Chat"), ([214, 96, 120], "Mail"), ([160, 130, 220], "Player")]
        .into_iter()
        .enumerate()
        .map(|(i, (rgb, tip))| TrayIcon { uid: i as u32 + 1, tip: tip.into(), image: Some(Arc::new(disc(rgb, i))), ..Default::default() })
        .collect()
}

/// A filled disc, a rounded square for odd icons, with soft edges.
fn disc(rgb: [u8; 3], shape: usize) -> egui::ColorImage {
    let mut pixels = Vec::with_capacity(SIZE * SIZE);
    let mid = SIZE as f32 / 2.0;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let (dx, dy) = ((x as f32 + 0.5 - mid).abs(), (y as f32 + 0.5 - mid).abs());
            let dist = if shape.is_multiple_of(2) { (dx * dx + dy * dy).sqrt() } else { dx.max(dy) * 1.12 };
            let a = (mid - 2.0 - dist).clamp(0.0, 1.0);
            pixels.push(egui::Color32::from_rgba_unmultiplied(rgb[0], rgb[1], rgb[2], (a * 255.0) as u8));
        }
    }
    egui::ColorImage { size: [SIZE, SIZE], pixels }
}
