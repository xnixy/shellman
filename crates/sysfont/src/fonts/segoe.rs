//! The faces being replaced: every Segoe UI face Windows registers, and each named instance of
//! Segoe UI Variable, read from Windows' own files so the replacements copy them exactly.

use super::names::{self, NameSet};
use super::rewrite::{Target, VMetrics};
use super::validate::{self, DwFace, GdiFace};
use super::{resolve_font_path, FontFile};
use read_fonts::TableProvider;
use serde::{Deserialize, Serialize};
use skrifa::MetadataProvider;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Layer {
    /// Segoe UI: Explorer, Control Panel, most desktop apps, and system-ui on the web.
    Segoe,
    /// Segoe UI Variable: Start, the taskbar, Settings, and other Windows 11 parts.
    Variable,
}

/// One face to replace, and where Windows registers it.
#[derive(Clone, Debug)]
pub struct Slot {
    pub layer: Layer,
    /// The registry value under HKLM\...\Fonts, like "Segoe UI Semibold (TrueType)".
    pub value: String,
    /// Its original data, like "seguisb.ttf".
    pub original_file: String,
    /// A stable short name, used for file names and saved choices: "semibold-italic".
    pub key: String,
    /// What the window shows: "Semibold Italic", "Display Light".
    pub label: String,
    pub target: Target,
    /// The optical-size group of a Segoe UI Variable instance: Small, Text, or Display.
    pub size_group: Option<String>,
    pub expected_gdi: GdiFace,
    pub expected_dw: DwFace,
}

impl Slot {
    pub fn weight(&self) -> u16 {
        self.target.weight
    }

    pub fn italic(&self) -> bool {
        self.target.italic
    }
}

/// Segoe UI faces Fontman never touches: they hold symbols, emoji, and historic scripts rather
/// than the letters of the UI font.
const KEPT: &[&str] = &["emoji", "symbol", "historic"];

/// Whether a registry value is one Fontman may replace, judged by its name alone.
pub fn is_candidate_value(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("segoe ui") && !KEPT.iter().any(|kept| lower.contains(kept))
}

/// The faces to replace, read from the original registry values `(name, data)`.
pub fn slots(values: &[(String, String)]) -> Result<Vec<Slot>, String> {
    let mut slots = Vec::new();
    for (value, data) in values {
        if !is_candidate_value(value) {
            continue;
        }
        let path = resolve_font_path(data);
        let file = FontFile::open(&path)?;
        let font = file.face(0)?;
        let family = names::get(&font, 16)
            .or_else(|| names::get(&font, 1))
            .unwrap_or_default();
        let has_axes = !font.axes().is_empty();
        if family.eq_ignore_ascii_case("Segoe UI") && !has_axes {
            slots.push(static_slot(&font, value, data)?);
        } else if family.eq_ignore_ascii_case("Segoe UI Variable") && has_axes {
            slots.extend(variable_slots(&font, value, data)?);
        }
    }
    if !slots.iter().any(|slot| slot.layer == Layer::Segoe) {
        return Err("Windows has no Segoe UI registered, so there is nothing to replace.".into());
    }
    slots.sort_by_key(|slot| {
        (
            slot.layer == Layer::Variable,
            slot.size_group.clone(),
            slot.italic(),
            slot.weight(),
        )
    });
    Ok(slots)
}

fn base_target(font: &read_fonts::FontRef) -> Result<Target, String> {
    let os2 = font.os2().map_err(|err| err.to_string())?;
    let mut panose = [0u8; 10];
    panose.copy_from_slice(os2.panose_10());
    let fs_selection = os2.fs_selection().bits();
    let mac_style = font.head().map(|head| head.mac_style().bits()).unwrap_or(0);
    Ok(Target {
        names: NameSet::from_font(font).ok_or("The face has no name.")?,
        weight: os2.us_weight_class(),
        width: os2.us_width_class(),
        italic: fs_selection & 1 != 0,
        fs_selection,
        mac_style,
        panose,
        metrics: VMetrics::from_font(font).ok_or("The face has no line metrics.")?,
        opsz: None,
    })
}

fn static_slot(font: &read_fonts::FontRef, value: &str, data: &str) -> Result<Slot, String> {
    let target = base_target(font)?;
    let path = resolve_font_path(data);
    let style = target
        .names
        .typo_subfamily
        .clone()
        .unwrap_or_else(|| target.names.subfamily.clone());
    let gdi = validate::gdi_faces(&path)?;
    let dw = validate::dwrite_faces(&path)?;
    Ok(Slot {
        layer: Layer::Segoe,
        value: value.to_string(),
        original_file: data.to_string(),
        key: slug(&style),
        label: style,
        target,
        size_group: None,
        expected_gdi: gdi.into_iter().next().ok_or("GDI found no face.")?,
        expected_dw: dw.into_iter().next().ok_or("DirectWrite found no face.")?,
    })
}

/// One slot per named instance. GDI and DirectWrite list the instances in the font's own order,
/// so the i-th face each reports belongs to the i-th instance.
fn variable_slots(
    font: &read_fonts::FontRef,
    value: &str,
    data: &str,
) -> Result<Vec<Slot>, String> {
    let base = base_target(font)?;
    let path = resolve_font_path(data);
    let gdi = validate::gdi_faces(&path)?;
    let dw = validate::dwrite_faces(&path)?;
    let axes: Vec<String> = font.axes().iter().map(|a| a.tag().to_string()).collect();
    let instances: Vec<_> = font.named_instances().iter().collect();
    if instances.len() != gdi.len() || instances.len() != dw.len() {
        return Err(format!(
            "{data} has {} instances, but Windows lists {} faces.",
            instances.len(),
            dw.len()
        ));
    }
    let mut slots = Vec::new();
    for (i, instance) in instances.iter().enumerate() {
        let coords: Vec<(String, f32)> = axes.iter().cloned().zip(instance.user_coords()).collect();
        let coord = |tag: &str| coords.iter().find(|(t, _)| t == tag).map(|(_, v)| *v);
        let weight = coord("wght").unwrap_or(400.0).round() as u16;
        let instance_name = names::get(font, instance.subfamily_name_id().to_u16())
            .unwrap_or_else(|| "Regular".into());
        let face = &dw[i];
        let bold = weight >= 700;
        let mut target = base.clone();
        target.weight = weight;
        target.italic = false;
        target.mac_style = if bold { 1 } else { 0 };
        target.fs_selection = if bold {
            0x0020
        } else if weight == 400 {
            0x0040
        } else {
            0x0000
        };
        target.opsz = coord("opsz");
        let typo_family = face.typographic.clone();
        target.names = NameSet {
            family: face.win32.clone(),
            subfamily: if bold { "Bold" } else { "Regular" }.into(),
            full: face.full.clone(),
            postscript: names::postscript_safe(&format!("{}-{}", typo_family, instance_name)),
            typo_family: Some(typo_family.clone()),
            typo_subfamily: Some(instance_name.clone()),
            wws_family: Some(face.family.clone()),
            wws_subfamily: Some(face.face.clone()),
        };
        let group = face
            .family
            .strip_prefix(&typo_family)
            .map(|rest| rest.trim().to_string())
            .filter(|rest| !rest.is_empty())
            .unwrap_or_else(|| "Text".into());
        let label = format!("{group} {}", face.face);
        slots.push(Slot {
            layer: Layer::Variable,
            value: value.to_string(),
            original_file: data.to_string(),
            key: format!("var-{}", slug(&label)),
            label,
            target,
            size_group: Some(group),
            expected_gdi: gdi[i].clone(),
            expected_dw: face.clone(),
        });
    }
    Ok(slots)
}

pub fn slug(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

/// The registry values a clean Windows 11 has for the faces Fontman replaces. Used to put Windows
/// back when no backup survives.
pub const WINDOWS_DEFAULTS: &[(&str, &str)] = &[
    ("Segoe UI (TrueType)", "segoeui.ttf"),
    ("Segoe UI Black (TrueType)", "seguibl.ttf"),
    ("Segoe UI Black Italic (TrueType)", "seguibli.ttf"),
    ("Segoe UI Bold (TrueType)", "segoeuib.ttf"),
    ("Segoe UI Bold Italic (TrueType)", "segoeuiz.ttf"),
    ("Segoe UI Italic (TrueType)", "segoeuii.ttf"),
    ("Segoe UI Light (TrueType)", "segoeuil.ttf"),
    ("Segoe UI Light Italic (TrueType)", "seguili.ttf"),
    ("Segoe UI Semibold (TrueType)", "seguisb.ttf"),
    ("Segoe UI Semibold Italic (TrueType)", "seguisbi.ttf"),
    ("Segoe UI Semilight (TrueType)", "segoeuisl.ttf"),
    ("Segoe UI Semilight Italic (TrueType)", "seguisli.ttf"),
    ("Segoe UI Variable (TrueType)", "SegUIVar.ttf"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_faces_are_never_candidates() {
        assert!(is_candidate_value("Segoe UI Semibold (TrueType)"));
        assert!(is_candidate_value("Segoe UI Variable (TrueType)"));
        assert!(!is_candidate_value("Segoe UI Emoji (TrueType)"));
        assert!(!is_candidate_value("Segoe UI Symbol (TrueType)"));
        assert!(!is_candidate_value("Segoe UI Historic (TrueType)"));
        assert!(!is_candidate_value("Segoe MDL2 Assets (TrueType)"));
        assert!(!is_candidate_value("Segoe Fluent Icons (TrueType)"));
    }

    #[test]
    fn slugs_are_lowercase_words() {
        assert_eq!(slug("Semibold Italic"), "semibold-italic");
        assert_eq!(slug("Display  Light"), "display-light");
    }
}
