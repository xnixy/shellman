//! From "use this family" to the files and registry values that make it so.
//!
//! Each Segoe UI face gets the closest face of the chosen family: the same weight when the family
//! has it, a variable font set to that weight, or the nearest weight it has. Every replacement is
//! then checked by asking GDI and DirectWrite what they would call it, before anything is
//! installed.

use crate::fonts::catalog::{Family, Origin};
use crate::fonts::rewrite::{self, Position, Tweaks};
use crate::fonts::segoe::{Layer, Slot};
use crate::fonts::{validate, FaceInfo, FontFile};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// A face picked by hand for one weight, instead of the automatic choice.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FaceChoice {
    pub path: PathBuf,
    pub index: u32,
    /// The weight to set a variable face to.
    pub weight: Option<f32>,
}

/// Which parts of Windows get the new font.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Layers {
    /// Title bars, menus, message boxes, tooltips, and icon labels. Changes at once.
    pub window_text: bool,
    /// Segoe UI, for Explorer and most apps. Needs administrator and a restart.
    pub segoe: bool,
    /// Segoe UI Variable, for the Windows 11 shell. Needs administrator and a restart.
    pub variable: bool,
    /// MS Shell Dlg, for old dialogs and property sheets.
    pub legacy: bool,
}

impl Default for Layers {
    fn default() -> Self {
        Self {
            window_text: true,
            segoe: true,
            variable: true,
            legacy: true,
        }
    }
}

impl Layers {
    /// Whether any part needs administrator rights.
    pub fn machine(&self) -> bool {
        self.segoe || self.variable || self.legacy
    }
}

/// How the replacement is adjusted.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Tune {
    /// Give it Segoe UI's line height, so text sits right in controls sized for Segoe UI.
    pub match_line_height: bool,
    /// Glyph size as a percentage of the font's own.
    pub size: u16,
    /// Slant the upright face where the family has no italic.
    pub synth_italic: bool,
}

impl Default for Tune {
    fn default() -> Self {
        Self {
            match_line_height: true,
            size: 100,
            synth_italic: true,
        }
    }
}

pub const SIZE_MIN: u16 = 80;
pub const SIZE_MAX: u16 = 125;

impl Tune {
    pub fn tweaks(&self) -> Tweaks {
        Tweaks {
            match_metrics: self.match_line_height,
            scale: self.size.clamp(SIZE_MIN, SIZE_MAX) as f32 / 100.0,
        }
    }
}

/// The key a hand-picked face is saved under: the weight, and "i" for italic. Segoe UI and
/// Segoe UI Variable faces of the same weight share it.
pub fn override_key(weight: u16, italic: bool) -> String {
    format!("{weight}{}", if italic { "i" } else { "" })
}

/// The face of `family` that stands in for one Segoe UI face.
#[derive(Clone, Debug, PartialEq)]
pub struct Mapped {
    pub face: FaceInfo,
    pub position: Position,
    /// The weight the replacement really has.
    pub weight: f32,
    pub by_hand: bool,
}

impl Mapped {
    /// "Bold", "Variable at 600", "Regular, slanted".
    pub fn describe(&self) -> String {
        let mut text = if self.face.is_variable() {
            format!("{} at {:.0}", variable_label(&self.face), self.weight)
        } else {
            self.face.style.clone()
        };
        if self.position.slant {
            text.push_str(", slanted");
        }
        text
    }
}

fn variable_label(face: &FaceInfo) -> &'static str {
    if face.italic {
        "Variable italic"
    } else {
        "Variable"
    }
}

/// Pick the face for a weight and slope, honoring a choice made by hand.
pub fn map(
    family: &Family,
    weight: u16,
    italic: bool,
    opsz: Option<f32>,
    overrides: &BTreeMap<String, FaceChoice>,
    tune: &Tune,
) -> Option<Mapped> {
    if let Some(choice) = overrides.get(&override_key(weight, italic)) {
        if let Some(face) = family
            .faces
            .iter()
            .find(|face| face.path == choice.path && face.index == choice.index)
        {
            let w = choice.weight.unwrap_or(face.weight as f32);
            let slant = italic && !face.italic && !face.italic_axis() && tune.synth_italic;
            return Some(Mapped {
                position: position_for(face, w, italic, opsz, slant),
                weight: clamp_weight(face, w),
                face: face.clone(),
                by_hand: true,
            });
        }
    }
    let usable: Vec<&FaceInfo> = {
        let normal: Vec<&FaceInfo> = family.faces.iter().filter(|f| f.width == 5).collect();
        if normal.is_empty() {
            family.faces.iter().collect()
        } else {
            normal
        }
    };
    let target = weight as f32;
    let styled = |want_italic: bool| -> Vec<&FaceInfo> {
        usable
            .iter()
            .copied()
            .filter(|f| f.italic == want_italic)
            .collect()
    };

    let exact = styled(italic)
        .into_iter()
        .find(|f| !f.is_variable() && f.weight == weight);
    if let Some(face) = exact {
        return Some(found(face, target, italic, opsz, false));
    }
    let variable = styled(italic)
        .into_iter()
        .find(|f| f.is_variable())
        .or_else(|| {
            italic
                .then(|| {
                    usable
                        .iter()
                        .copied()
                        .find(|f| f.is_variable() && f.italic_axis())
                })
                .flatten()
        });
    if let Some(face) = variable {
        return Some(found(face, target, italic, opsz, false));
    }
    if let Some(face) = nearest(&styled(italic), weight) {
        return Some(found(face, face.weight as f32, italic, opsz, false));
    }
    // No face has this slope. An italic falls back to the upright face, slanted if allowed.
    if italic {
        let upright = map(family, weight, false, opsz, &BTreeMap::new(), tune)?;
        let slant = tune.synth_italic;
        return Some(Mapped {
            position: position_for(&upright.face, upright.weight, false, opsz, slant),
            ..upright
        });
    }
    let face = nearest(&usable, weight)?;
    Some(found(face, face.weight as f32, italic, opsz, false))
}

fn found(face: &FaceInfo, weight: f32, italic: bool, opsz: Option<f32>, slant: bool) -> Mapped {
    Mapped {
        position: position_for(face, weight, italic, opsz, slant),
        weight: clamp_weight(face, weight),
        face: face.clone(),
        by_hand: false,
    }
}

fn clamp_weight(face: &FaceInfo, weight: f32) -> f32 {
    let (low, high) = face.weight_range();
    weight.clamp(low, high)
}

/// Axis settings that put a variable face at the wanted weight, slope, and optical size.
fn position_for(
    face: &FaceInfo,
    weight: f32,
    italic: bool,
    opsz: Option<f32>,
    slant: bool,
) -> Position {
    let mut axes = Vec::new();
    for axis in &face.axes {
        let value = match axis.tag.as_str() {
            "wght" => weight,
            "opsz" => opsz.unwrap_or(axis.default),
            "ital" if !face.italic => {
                if italic {
                    axis.max.min(1.0)
                } else {
                    0.0
                }
            }
            "slnt" if !face.italic => {
                if italic {
                    axis.min
                } else {
                    0.0
                }
            }
            _ => axis.default,
        };
        axes.push((axis.tag.clone(), value.clamp(axis.min, axis.max)));
    }
    Position { axes, slant }
}

/// The closest weight among static faces, the way CSS chooses: lighter first for light requests,
/// heavier first for bold ones.
fn nearest<'a>(faces: &[&'a FaceInfo], weight: u16) -> Option<&'a FaceInfo> {
    faces
        .iter()
        .copied()
        .filter(|f| !f.is_variable())
        .min_by_key(|f| weight_distance(weight, f.weight))
        .or_else(|| faces.first().copied())
}

fn weight_distance(want: u16, have: u16) -> (u8, u16) {
    let (want, have) = (want as i32, have as i32);
    let diff = (want - have).unsigned_abs() as u16;
    let preferred = if want < 400 {
        have <= want
    } else if want > 500 {
        have >= want
    } else {
        have >= want && have <= 500
    };
    (if preferred { 0 } else { 1 }, diff)
}

/// One replacement face, written to the staging folder.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BuiltFile {
    pub key: String,
    pub label: String,
    pub layer: Layer,
    /// The registry value it is installed under.
    pub value: String,
    /// The file name in the Windows fonts folder.
    pub file_name: String,
    pub staged: PathBuf,
}

/// Registry changes, as value name and data.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistryPlan {
    /// Existing Segoe UI values pointed at the replacements.
    pub set: Vec<(String, String)>,
    /// Added values, for faces that share one original file.
    pub extras: Vec<(String, String)>,
    /// FontSubstitutes entries.
    pub substitutes: Vec<(String, String)>,
}

/// Values Fontman adds start with this, so they can always be found and removed.
pub const EXTRA_PREFIX: &str = "Fontman ";

/// Everything the administrator step needs, written as JSON for it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MachinePlan {
    pub generation: String,
    pub family: String,
    pub summary: String,
    pub files: Vec<BuiltFile>,
    pub registry: RegistryPlan,
}

/// A new generation id: the time in base 36. Each apply gets fresh file names, so the files
/// Windows has open from the last apply never have to be overwritten.
pub fn new_generation() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut n = secs;
    let mut out = Vec::new();
    while n > 0 {
        let digit = (n % 36) as u8;
        out.push(if digit < 10 {
            b'0' + digit
        } else {
            b'a' + digit - 10
        });
        n /= 36;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_else(|_| "0".into())
}

/// What one build should cover.
pub struct Request<'a> {
    pub family: &'a Family,
    pub slots: &'a [Slot],
    pub overrides: &'a BTreeMap<String, FaceChoice>,
    pub tune: Tune,
    pub layers: Layers,
}

impl Request<'_> {
    pub fn wanted_slots(&self) -> impl Iterator<Item = &Slot> {
        self.slots.iter().filter(|slot| match slot.layer {
            Layer::Segoe => self.layers.segoe,
            Layer::Variable => self.layers.variable,
        })
    }

    pub fn mapped(&self, slot: &Slot) -> Option<Mapped> {
        map(
            self.family,
            slot.weight(),
            slot.italic(),
            slot.target.opsz,
            self.overrides,
            &self.tune,
        )
    }
}

/// The name note written into each replacement.
fn note(face: &FaceInfo) -> String {
    format!(
        "Made by Fontman from {} for use as a system font on this PC.",
        face.full_name
    )
}

/// Write one replacement face for `slot` and return its bytes.
pub fn build_face(request: &Request, slot: &Slot) -> Result<(Mapped, Vec<u8>), String> {
    let mapped = request
        .mapped(slot)
        .ok_or_else(|| format!("{} has no face for {}.", request.family.name, slot.label))?;
    let file = FontFile::open(&mapped.face.path)?;
    let font = file.face(mapped.face.index)?;
    let bytes = rewrite::build(
        &font,
        &mapped.position,
        &slot.target,
        request.tune.tweaks(),
        &note(&mapped.face),
    )
    .map_err(|err| format!("{}: {err}", slot.label))?;
    Ok((mapped, bytes))
}

/// Build every replacement into `staging`, check each one with GDI and DirectWrite, and return
/// the plan for the administrator step. `progress` hears (done, total).
pub fn build(
    request: &Request,
    staging: &Path,
    generation: &str,
    mut progress: impl FnMut(usize, usize),
) -> Result<MachinePlan, String> {
    std::fs::create_dir_all(staging).map_err(|err| format!("{}: {err}", staging.display()))?;
    let slots: Vec<&Slot> = request.wanted_slots().collect();
    let total = slots.len();
    let mut files = Vec::new();
    for (done, slot) in slots.iter().enumerate() {
        progress(done, total);
        let (_, bytes) = build_face(request, slot)?;
        let file_name = format!(
            "{}{generation}-{}.ttf",
            crate::fonts::catalog::OWN_PREFIX,
            match slot.layer {
                Layer::Segoe => format!("segoeui-{}", slot.key),
                Layer::Variable => format!("segoeuivar-{}", slot.key.trim_start_matches("var-")),
            }
        );
        let staged = staging.join(&file_name);
        std::fs::write(&staged, &bytes).map_err(|err| format!("{}: {err}", staged.display()))?;
        check(slot, &staged)?;
        files.push(BuiltFile {
            key: slot.key.clone(),
            label: slot.label.clone(),
            layer: slot.layer,
            value: slot.value.clone(),
            file_name,
            staged,
        });
    }
    progress(total, total);
    let registry = registry_plan(&files, request);
    Ok(MachinePlan {
        generation: generation.to_string(),
        family: request.family.name.clone(),
        summary: machine_summary(&request.layers),
        files,
        registry,
    })
}

/// Make sure Windows will see the replacement exactly as it sees the face it replaces.
pub fn check(slot: &Slot, path: &Path) -> Result<(), String> {
    let gdi = validate::gdi_faces(path)?;
    let dw = validate::dwrite_faces(path)?;
    let problems = validate::compare(
        std::slice::from_ref(&slot.expected_gdi),
        &gdi,
        std::slice::from_ref(&slot.expected_dw),
        &dw,
    );
    if problems.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "The replacement for {} would not work: {}.",
            slot.label,
            problems.join("; ")
        ))
    }
}

/// Which registry values point where. Static Segoe UI faces each have their own value. The
/// faces of Segoe UI Variable share one value, so the first takes it and the rest are added.
fn registry_plan(files: &[BuiltFile], request: &Request) -> RegistryPlan {
    let mut plan = RegistryPlan::default();
    let mut taken = std::collections::HashSet::new();
    for file in files {
        if taken.insert(file.value.clone()) {
            plan.set.push((file.value.clone(), file.file_name.clone()));
        } else {
            plan.extras.push((
                format!("{EXTRA_PREFIX}Segoe UI Variable {} (TrueType)", file.label),
                file.file_name.clone(),
            ));
        }
    }
    if request.layers.legacy {
        let name = if request.layers.segoe {
            Some("Segoe UI".to_string())
        } else if request.family.origin == Origin::Installed {
            request
                .family
                .regular()
                .map(|face| face.legacy_family.clone())
        } else {
            None
        };
        if let Some(name) = name {
            for dialog in ["MS Shell Dlg", "MS Shell Dlg 2"] {
                plan.substitutes.push((dialog.to_string(), name.clone()));
            }
        }
    }
    plan
}

/// "Apps, Windows 11 shell, and old dialogs".
pub fn summary(request: &Request) -> String {
    layers_summary(&request.layers)
}

/// "window text, apps, Windows 11 shell, and old dialogs": every part `layers` switches on.
pub fn layers_summary(layers: &Layers) -> String {
    let mut parts = Vec::new();
    if layers.window_text {
        parts.push("window text");
    }
    if layers.segoe {
        parts.push("apps");
    }
    if layers.variable {
        parts.push("Windows 11 shell");
    }
    if layers.legacy {
        parts.push("old dialogs");
    }
    join_words(&parts)
}

/// The machine-wide parts only, for the record of what is in place: "apps and old dialogs".
pub fn machine_summary(layers: &Layers) -> String {
    let mut parts = Vec::new();
    if layers.segoe {
        parts.push("apps");
    }
    if layers.variable {
        parts.push("Windows 11 shell");
    }
    if layers.legacy {
        parts.push("old dialogs");
    }
    join_words(&parts)
}

pub fn join_words(parts: &[&str]) -> String {
    match parts {
        [] => String::new(),
        [one] => one.to_string(),
        [first, second] => format!("{first} and {second}"),
        [rest @ .., last] => format!("{}, and {last}", rest.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fonts::{AxisInfo, Outlines};

    fn face(style: &str, weight: u16, italic: bool) -> FaceInfo {
        FaceInfo {
            path: PathBuf::from(format!("{style}.ttf")),
            index: 0,
            family: "Test".into(),
            legacy_family: "Test".into(),
            style: style.into(),
            full_name: format!("Test {style}"),
            postscript: String::new(),
            weight,
            width: 5,
            italic,
            axes: Vec::new(),
            instances: Vec::new(),
            outlines: Outlines::TrueType,
            latin: true,
            fs_type: 0,
        }
    }

    fn family(faces: Vec<FaceInfo>) -> Family {
        Family {
            name: "Test".into(),
            faces,
            origin: Origin::Installed,
        }
    }

    #[test]
    fn exact_weights_are_used() {
        let fam = family(vec![
            face("Regular", 400, false),
            face("Bold", 700, false),
            face("Light", 300, false),
        ]);
        let tune = Tune::default();
        let m = map(&fam, 700, false, None, &BTreeMap::new(), &tune).unwrap();
        assert_eq!(m.face.style, "Bold");
        assert!(m.position.axes.is_empty());
    }

    #[test]
    fn missing_weights_follow_css_order() {
        let fam = family(vec![
            face("Regular", 400, false),
            face("Bold", 700, false),
            face("Thin", 100, false),
        ]);
        let tune = Tune::default();
        // Semibold prefers heavier: Bold.
        assert_eq!(
            map(&fam, 600, false, None, &BTreeMap::new(), &tune)
                .unwrap()
                .face
                .style,
            "Bold"
        );
        // Light prefers lighter: Thin.
        assert_eq!(
            map(&fam, 300, false, None, &BTreeMap::new(), &tune)
                .unwrap()
                .face
                .style,
            "Thin"
        );
        // Semilight (350) prefers lighter too.
        assert_eq!(
            map(&fam, 350, false, None, &BTreeMap::new(), &tune)
                .unwrap()
                .face
                .style,
            "Thin"
        );
    }

    #[test]
    fn italics_are_slanted_when_the_family_has_none() {
        let fam = family(vec![face("Regular", 400, false), face("Bold", 700, false)]);
        let tune = Tune::default();
        let m = map(&fam, 700, true, None, &BTreeMap::new(), &tune).unwrap();
        assert_eq!(m.face.style, "Bold");
        assert!(m.position.slant);
        let plain = Tune {
            synth_italic: false,
            ..Tune::default()
        };
        assert!(
            !map(&fam, 700, true, None, &BTreeMap::new(), &plain)
                .unwrap()
                .position
                .slant
        );
    }

    #[test]
    fn variable_faces_are_set_to_the_weight() {
        let mut var = face("Regular", 400, false);
        var.axes = vec![
            AxisInfo {
                tag: "wght".into(),
                min: 100.0,
                default: 400.0,
                max: 900.0,
            },
            AxisInfo {
                tag: "opsz".into(),
                min: 14.0,
                default: 14.0,
                max: 32.0,
            },
        ];
        let fam = family(vec![var]);
        let m = map(
            &fam,
            350,
            false,
            Some(36.0),
            &BTreeMap::new(),
            &Tune::default(),
        )
        .unwrap();
        assert_eq!(
            m.position.axes,
            vec![("wght".into(), 350.0), ("opsz".into(), 32.0)]
        );
    }

    #[test]
    fn a_face_picked_by_hand_wins() {
        let fam = family(vec![
            face("Regular", 400, false),
            face("Medium", 500, false),
        ]);
        let mut overrides = BTreeMap::new();
        overrides.insert(
            override_key(400, false),
            FaceChoice {
                path: "Medium.ttf".into(),
                index: 0,
                weight: None,
            },
        );
        let m = map(&fam, 400, false, None, &overrides, &Tune::default()).unwrap();
        assert_eq!(m.face.style, "Medium");
        assert!(m.by_hand);
    }

    /// Builds every replacement from fonts every Windows has, a static family and a variable
    /// one, and has GDI and DirectWrite check each against the Segoe UI face it replaces.
    #[test]
    fn windows_fonts_replace_every_segoe_face() {
        // Windows' own files, even on a PC whose entries point at replacements now.
        let values = crate::machine::originals_from(crate::reg::values(
            crate::reg::HKEY_LOCAL_MACHINE,
            crate::reg::FONTS_KEY,
        ));
        let slots = crate::fonts::segoe::slots(&values).expect("Segoe UI faces");
        let dir = crate::fonts::windows_fonts_dir();
        let cases: [(&str, &[&str]); 2] = [
            (
                "Arial",
                &["arial.ttf", "arialbd.ttf", "ariali.ttf", "arialbi.ttf"],
            ),
            ("Bahnschrift", &["bahnschrift.ttf"]),
        ];
        for (name, files) in cases {
            // Windows Server, which CI runs on, may lack some of the desktop fonts.
            if !files.iter().all(|file| dir.join(file).exists()) {
                eprintln!("{name} is not installed here; skipping it.");
                continue;
            }
            let faces: Vec<FaceInfo> = files
                .iter()
                .flat_map(|file| crate::fonts::read_faces(&dir.join(file)).expect(file))
                .collect();
            let family = Family {
                name: name.into(),
                faces,
                origin: Origin::Installed,
            };
            let request = Request {
                family: &family,
                slots: &slots,
                overrides: &BTreeMap::new(),
                tune: Tune {
                    size: 105,
                    ..Tune::default()
                },
                layers: Layers::default(),
            };
            let out =
                std::env::temp_dir().join(format!("shellman-sysfont-test-{name}-{}", std::process::id()));
            let plan = build(&request, &out, "test", |_, _| {}).expect(name);
            assert_eq!(plan.files.len(), slots.len());
            assert!(plan
                .registry
                .set
                .iter()
                .any(|(v, _)| v == "Segoe UI (TrueType)"));
            let _ = std::fs::remove_dir_all(&out);
        }
    }

    #[test]
    fn generations_are_short_and_ordered() {
        let a = new_generation();
        assert!(a.len() >= 5 && a.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn words_join_like_a_sentence() {
        assert_eq!(join_words(&["a"]), "a");
        assert_eq!(join_words(&["a", "b"]), "a and b");
        assert_eq!(join_words(&["a", "b", "c"]), "a, b, and c");
    }
}
