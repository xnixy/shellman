//! Shellman's own app icons, drawn from code in whatever accent color is asked for.
//! `tools/icongen` uses this for the committed `.ico` files; the apps use it at run time so
//! their icons follow the user's accent.

mod art;
mod shapes;

pub use art::{render, Kind};

/// The accent the committed `.ico` files are drawn in (the default rose).
pub const DEFAULT_ACCENT: [u8; 3] = [196, 54, 78];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_and_alpha_are_right() {
        let px = render(Kind::Files, 32, DEFAULT_ACCENT);
        assert_eq!(px.len(), 32 * 32 * 4);
        // The plate's rounded corner is clear, its middle solid.
        assert_eq!(px[3], 0);
        assert_eq!(px[(16 * 32 + 16) * 4 + 3], 255);
    }

    #[test]
    fn a_different_accent_gives_different_pixels() {
        for kind in [Kind::Shellman, Kind::Files, Kind::Settings] {
            assert_ne!(render(kind, 48, DEFAULT_ACCENT), render(kind, 48, [0, 103, 192]));
        }
    }

    #[test]
    fn a_light_accent_gets_a_dark_glyph() {
        let px = render(Kind::Shellman, 64, [255, 220, 80]);
        // The middle of the top-left dot.
        let at = ((22 * 64 + 22) * 4) as usize;
        assert!(px[at] < 100, "glyph should be dark, got {}", px[at]);
    }

    fn at(px: &[u8], size: u32, x: u32, y: u32) -> [u8; 3] {
        let i = ((y * size + x) * 4) as usize;
        [px[i], px[i + 1], px[i + 2]]
    }

    fn lum(c: [u8; 3]) -> f32 {
        (0.2126 * c[0] as f32 + 0.7152 * c[1] as f32 + 0.0722 * c[2] as f32) / 255.0
    }

    #[test]
    fn a_black_accent_still_has_a_plate_with_form() {
        let size = 96;
        let px = render(Kind::Files, size, [0, 0, 0]);
        // Left of the glyph, near the top and near the bottom.
        let top = at(&px, size, 8, 22);
        let bottom = at(&px, size, 8, 74);
        assert!(lum(top) > lum(bottom) + 0.05, "{top:?} vs {bottom:?}");
        assert!(lum(top) > 0.1, "plate is not lifted off black: {top:?}");
        // The rim along the top differs from the plate just below it.
        let rim = at(&px, size, 48, 4);
        let body = at(&px, size, 48, 12);
        assert!(lum(rim) > lum(body) + 0.02, "{rim:?} vs {body:?}");
    }

    #[test]
    fn plate_luminance_stays_in_the_clamp() {
        for accent in [[0u8, 0, 0], [255, 255, 255], [0, 0, 128], [26, 26, 26]] {
            let l = art::luminance(art::plate_base(accent.map(|v| v as f32 / 255.0)));
            assert!((art::MIN_LUM - 0.01..=art::MAX_LUM + 0.01).contains(&l), "{accent:?} -> {l}");
        }
        // A normal accent is left as it is.
        let rose = DEFAULT_ACCENT.map(|v| v as f32 / 255.0);
        assert_eq!(art::plate_base(rose), rose);
    }

    #[test]
    fn a_white_accent_gets_a_dark_glyph() {
        let px = render(Kind::Shellman, 64, [255, 255, 255]);
        let at = ((22 * 64 + 22) * 4) as usize;
        assert!(px[at] < 100, "glyph should be dark, got {}", px[at]);
    }

    /// Run with `cargo test -p shellman-icons --release -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn timing() {
        for size in [32, 96, 256] {
            let start = std::time::Instant::now();
            render(Kind::Files, size, DEFAULT_ACCENT);
            println!("{size} px: {:?}", start.elapsed());
        }
    }
}