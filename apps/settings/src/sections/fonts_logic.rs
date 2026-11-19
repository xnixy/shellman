//! The Fonts page's decisions, kept apart from drawing so they are tested without a window: which
//! families the list shows and in what order, what the status group says, what the Apply button
//! offers, and how the sliders map to Fontman's values.

use shellman_sysfont::fonts::catalog::{Family, Origin};
use shellman_sysfont::machine::Status;
use shellman_sysfont::metrics;
use shellman_sysfont::plan::{self, FaceChoice, Layers, Tune};
use std::collections::BTreeMap;

/// The page's tabs, which are Fontman's pages and the editor's tabs on one row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Fonts,
    Preview,
    Weights,
    Tune,
    Coverage,
    Backups,
}

impl Tab {
    pub const ALL: [Tab; 6] = [Tab::Fonts, Tab::Preview, Tab::Weights, Tab::Tune, Tab::Coverage, Tab::Backups];

    pub fn label(self) -> &'static str {
        match self {
            Tab::Fonts => "Fonts",
            Tab::Preview => "Preview",
            Tab::Weights => "Weights",
            Tab::Tune => "Tune",
            Tab::Coverage => "Coverage",
            Tab::Backups => "Backups",
        }
    }

    /// `SHELLMAN_SYSFONT_TAB`, for opening straight onto a tab.
    pub fn from_word(word: &str) -> Option<Tab> {
        Tab::ALL.into_iter().find(|t| t.label().eq_ignore_ascii_case(word))
    }
}

/// The list's rows: indexes into `families` matching `query`, the family in use first.
pub fn order(families: &[Family], query: &str, in_use: Option<&str>) -> Vec<usize> {
    let query = query.trim().to_lowercase();
    let mut shown: Vec<usize> = families
        .iter()
        .enumerate()
        .filter(|(_, f)| query.is_empty() || f.name.to_lowercase().contains(&query))
        .map(|(i, _)| i)
        .collect();
    // A stable sort keeps the catalog's alphabetical order among the rest.
    shown.sort_by_key(|&i| {
        let f = &families[i];
        let using = in_use.is_some_and(|name| f.name.eq_ignore_ascii_case(name)) && f.origin == Origin::Installed;
        !using
    });
    shown
}

/// Which rows of `n` (each `stride` tall, the first at `top`) a clip rect reaches.
pub fn visible_range(clip_top: f32, clip_bottom: f32, top: f32, stride: f32, n: usize) -> std::ops::Range<usize> {
    let first = (((clip_top - top) / stride).floor().max(0.0) as usize).min(n);
    let last = ((((clip_bottom - top) / stride).ceil().max(0.0) as usize) + 1).min(n);
    first..last.max(first)
}

/// The key a family's sample face is loaded under.
pub fn tile_key(family: &Family) -> String {
    format!("tile:{}|{:?}", family.name, family.origin)
}

/// What the preview was built from: a new build is needed when this changes.
pub fn preview_key(family: &Family, tune: &Tune, overrides: &BTreeMap<String, FaceChoice>) -> String {
    format!("{}|{:?}|{:?}|{:?}", family.name, family.origin, tune, overrides)
}

/// The status group's first line.
pub fn status_title(status: &Status) -> String {
    match &status.current {
        Some(current) if current.summary.is_empty() => format!("Windows uses {}.", current.family),
        Some(current) => format!("Windows uses {} for {}.", current.family, current.summary),
        None => "Windows uses its own Segoe UI.".to_string(),
    }
}

/// What the Apply button offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Offer {
    /// No family is chosen.
    Choose,
    /// The choice is what is in place, and Windows still has it.
    InPlace,
    /// Put the chosen family in place: "Apply Inter".
    Apply(String),
    /// Windows put Segoe UI back: put the same font back.
    Again(String),
}

impl Offer {
    pub fn label(&self) -> String {
        match self {
            Offer::Choose => "Choose a font".into(),
            Offer::InPlace => "In use".into(),
            Offer::Apply(name) => format!("Apply {name}"),
            Offer::Again(_) => "Apply again".into(),
        }
    }

    pub fn enabled(&self) -> bool {
        matches!(self, Offer::Apply(_) | Offer::Again(_))
    }
}

/// What Apply offers for `chosen`, with `differs` telling whether the choices changed since it was
/// last applied.
pub fn offer(chosen: Option<&str>, status: &Status, differs: bool) -> Offer {
    let Some(name) = chosen else { return Offer::Choose };
    let current = status.current.as_ref().map(|c| c.family.as_str());
    if status.drift && current == Some(name) && !differs {
        return Offer::Again(name.to_string());
    }
    if differs {
        Offer::Apply(name.to_string())
    } else {
        Offer::InPlace
    }
}

/// The Weights tab's rows: the twelve Segoe UI weights and slopes a face can be picked for.
pub const WEIGHT_ROWS: [(&str, u16, bool); 12] = [
    ("Light", 300, false),
    ("Semilight", 350, false),
    ("Regular", 400, false),
    ("Semibold", 600, false),
    ("Bold", 700, false),
    ("Black", 900, false),
    ("Light Italic", 300, true),
    ("Semilight Italic", 350, true),
    ("Italic", 400, true),
    ("Semibold Italic", 600, true),
    ("Bold Italic", 700, true),
    ("Black Italic", 900, true),
];

/// One part of Windows the Coverage tab switches.
pub struct Part {
    pub label: &'static str,
    pub what: &'static str,
    pub needs: &'static str,
    pub get: fn(&Layers) -> bool,
    pub set: fn(&mut Layers, bool),
}

pub const PARTS: [Part; 4] = [
    Part {
        label: "Window text",
        what: "Title bars, menus, message boxes, tooltips, status bars and desktop icon labels.",
        needs: "Nothing: changes at once, and comes back if a theme change resets it.",
        get: |l| l.window_text,
        set: |l, on| l.window_text = on,
    },
    Part {
        label: "Apps and File Explorer",
        what: "Segoe UI: File Explorer, Control Panel, most desktop apps, and web pages that use the system font.",
        needs: "Administrator rights, then a restart.",
        get: |l| l.segoe,
        set: |l, on| l.segoe = on,
    },
    Part {
        label: "Windows 11 shell",
        what: "Segoe UI Variable: Start, the taskbar, Settings and Windows 11's newer windows.",
        needs: "Administrator rights, then a restart.",
        get: |l| l.variable,
        set: |l, on| l.variable = on,
    },
    Part {
        label: "Old dialogs",
        what: "MS Shell Dlg: property sheets and older dialogs.",
        needs: "Administrator rights, then a restart.",
        get: |l| l.legacy,
        set: |l, on| l.legacy = on,
    },
];

/// The glyph size slider: 0..1 to Fontman's percentage.
pub fn size_from_slider(t: f32) -> u16 {
    let span = (plan::SIZE_MAX - plan::SIZE_MIN) as f32;
    (plan::SIZE_MIN as f32 + t.clamp(0.0, 1.0) * span).round() as u16
}

pub fn slider_from_size(size: u16) -> f32 {
    let span = (plan::SIZE_MAX - plan::SIZE_MIN) as f32;
    (size.clamp(plan::SIZE_MIN, plan::SIZE_MAX) - plan::SIZE_MIN) as f32 / span
}

/// A window text size slider: 0..1 to points, in half points.
pub fn points_from_slider(t: f32) -> f32 {
    let span = metrics::SIZE_MAX - metrics::SIZE_MIN;
    ((metrics::SIZE_MIN + t.clamp(0.0, 1.0) * span) * 2.0).round() / 2.0
}

pub fn slider_from_points(points: f32) -> f32 {
    let span = metrics::SIZE_MAX - metrics::SIZE_MIN;
    (points.clamp(metrics::SIZE_MIN, metrics::SIZE_MAX) - metrics::SIZE_MIN) / span
}

#[cfg(test)]
mod tests {
    use super::*;
    use shellman_sysfont::machine::Applied;
    use shellman_sysfont::plan::RegistryPlan;

    fn family(name: &str, origin: Origin) -> Family {
        Family { name: name.into(), faces: Vec::new(), origin }
    }

    fn status(family: Option<&str>, drift: bool) -> Status {
        Status {
            current: family.map(|f| Applied {
                generation: "g".into(),
                family: f.into(),
                summary: "apps and Windows 11 shell".into(),
                applied_at: 0,
                registry: RegistryPlan::default(),
                files: Vec::new(),
            }),
            drift,
            ..Status::default()
        }
    }

    #[test]
    fn the_font_in_use_comes_first_then_the_alphabet() {
        let fams = [family("Arial", Origin::Installed), family("Inter", Origin::Added), family("Bahnschrift", Origin::Installed), family("Inter", Origin::Installed)];
        assert_eq!(order(&fams, "", Some("Inter")), vec![3, 0, 1, 2]);
        assert_eq!(order(&fams, "", None), vec![0, 1, 2, 3]);
    }

    #[test]
    fn the_search_matches_any_part_of_the_name() {
        let fams = [family("Arial", Origin::Installed), family("SF Pro Text", Origin::Installed), family("SF Pro Display", Origin::Installed)];
        assert_eq!(order(&fams, " pro ", None), vec![1, 2]);
        assert_eq!(order(&fams, "zzz", None), Vec::<usize>::new());
    }

    #[test]
    fn only_rows_in_view_are_drawn() {
        assert_eq!(visible_range(400.0, 900.0, 100.0, 60.0, 100), 5..15);
        assert_eq!(visible_range(0.0, 100.0, 0.0, 60.0, 0), 0..0);
    }

    #[test]
    fn the_status_says_what_windows_uses() {
        assert_eq!(status_title(&status(None, false)), "Windows uses its own Segoe UI.");
        assert_eq!(status_title(&status(Some("SF Pro Display"), false)), "Windows uses SF Pro Display for apps and Windows 11 shell.");
    }

    #[test]
    fn apply_offers_what_makes_sense() {
        assert_eq!(offer(None, &status(None, false), true), Offer::Choose);
        assert_eq!(offer(Some("Inter"), &status(None, false), true), Offer::Apply("Inter".into()));
        assert_eq!(offer(Some("Inter"), &status(Some("Inter"), false), false), Offer::InPlace);
        assert_eq!(offer(Some("Inter"), &status(Some("Inter"), true), false), Offer::Again("Inter".into()));
        // A changed choice is a new apply, even after Windows put Segoe UI back.
        assert_eq!(offer(Some("Inter"), &status(Some("Inter"), true), true), Offer::Apply("Inter".into()));
        assert!(!Offer::InPlace.enabled() && Offer::Again("x".into()).enabled());
    }

    #[test]
    fn sliders_map_to_fontmans_ranges() {
        assert_eq!(size_from_slider(0.0), 80);
        assert_eq!(size_from_slider(1.0), 125);
        assert_eq!(size_from_slider(slider_from_size(105)), 105);
        assert_eq!(points_from_slider(0.0), 7.0);
        assert_eq!(points_from_slider(1.0), 14.0);
        assert_eq!(points_from_slider(slider_from_points(9.5)), 9.5);
    }

    #[test]
    fn tabs_are_found_by_name() {
        for t in Tab::ALL {
            assert_eq!(Tab::from_word(&t.label().to_lowercase()), Some(t));
        }
        assert_eq!(Tab::from_word("nope"), None);
    }

    #[test]
    fn coverage_switches_set_their_own_part() {
        for (i, part) in PARTS.iter().enumerate() {
            let mut layers = Layers { window_text: false, segoe: false, variable: false, legacy: false };
            (part.set)(&mut layers, true);
            assert!((part.get)(&layers));
            let on = [layers.window_text, layers.segoe, layers.variable, layers.legacy];
            assert_eq!(on.iter().filter(|b| **b).count(), 1, "{}", part.label);
            assert!(on[i]);
        }
    }
}
