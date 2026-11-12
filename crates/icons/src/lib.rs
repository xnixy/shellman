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