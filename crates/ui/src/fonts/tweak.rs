//! Per-family vertical centring. egui puts a row's text by the font's own ascent and descent, so
//! a font whose capitals sit higher or lower in its line box than Segoe UI's looks off-centre in
//! chips, pills and rows that are centred by height. Each family that was looked at has an entry;
//! every other family is left neutral.
//!
//! Offsets are a fraction of the font size, positive downwards (egui's `y_offset_factor`).

use egui::FontTweak;

struct Entry {
    /// Matches a family name that starts with this, ignoring case.
    prefix: &'static str,
    y_offset_factor: f32,
}

/// Measured by rendering a label in a pill and comparing the capital letters' ink centre with the
/// pill's centre: the offset that brings it to 0. Put longer prefixes first.
const TABLE: &[Entry] = &[
    Entry { prefix: "SF Pro", y_offset_factor: -0.02 },
    // The variable file (SegUIVar.ttf) and the static files measured the same.
    Entry { prefix: "Segoe UI", y_offset_factor: -0.052 },
    // Inter was not installed on the machine this was measured on: left neutral.
];

pub fn for_family(name: &str) -> FontTweak {
    let n = name.to_ascii_lowercase();
    let y_offset_factor = TABLE
        .iter()
        .find(|e| n.starts_with(&e.prefix.to_ascii_lowercase()))
        .map_or(0.0, |e| e.y_offset_factor);
    FontTweak { y_offset_factor, ..FontTweak::default() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_families_are_neutral() {
        assert_eq!(for_family("Comic Neue").y_offset_factor, 0.0);
        assert_eq!(for_family("Comic Neue").scale, 1.0);
    }

    #[test]
    fn known_families_match_by_prefix() {
        let sf = TABLE.iter().find(|e| e.prefix == "SF Pro").unwrap().y_offset_factor;
        assert_eq!(for_family("sf pro display").y_offset_factor, sf);
        assert_eq!(for_family("Segoe UI Variable").y_offset_factor, for_family("Segoe UI").y_offset_factor);
    }
}
