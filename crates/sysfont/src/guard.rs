//! What the administrator step accepts. The plan is written by the unelevated side, in a folder
//! the user can write, so the elevated side trusts nothing in it it can check: the generation,
//! every file name and where it was staged, and every registry value name and data. Pure, so the
//! rules are tested without a plan ever being applied.

use crate::fonts::catalog::OWN_PREFIX;
use crate::machine::DIALOG_FONTS;
use crate::plan::{MachinePlan, EXTRA_PREFIX};
use std::collections::HashSet;
use std::path::Path;

/// The most faces one plan can have: 12 Segoe UI faces and the Segoe UI Variable instances, with
/// room to spare.
const MAX_FILES: usize = 64;

/// A generation id as `plan::new_generation` makes them: base-36 time, lowercase, short.
pub fn is_generation(s: &str) -> bool {
    !s.is_empty() && s.len() <= 13 && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

/// Whether `name` is a replacement file of `generation`, as `plan::build` names them:
/// `fontman-<generation>-<segoeui|segoeuivar>-<slug>.ttf`, a bare file name.
pub fn is_file_name(name: &str, generation: &str) -> bool {
    let Some(rest) = name.strip_prefix(OWN_PREFIX) else { return false };
    let Some(rest) = rest.strip_prefix(generation).and_then(|r| r.strip_prefix('-')) else { return false };
    let Some(stem) = rest.strip_suffix(".ttf") else { return false };
    let slug = stem.strip_prefix("segoeuivar-").or_else(|| stem.strip_prefix("segoeui-"));
    slug.is_some_and(|s| {
        !s.is_empty()
            && !s.starts_with('-')
            && !s.ends_with('-')
            && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    })
}

/// Whether `name` is a value this program adds for a Segoe UI Variable instance:
/// `Fontman Segoe UI Variable <label> (TrueType)`.
pub fn is_extra_value(name: &str) -> bool {
    let Some(label) = name
        .strip_prefix(EXTRA_PREFIX)
        .and_then(|r| r.strip_prefix("Segoe UI Variable "))
        .and_then(|r| r.strip_suffix(" (TrueType)"))
    else {
        return false;
    };
    !label.is_empty()
        && label.len() <= 40
        && label.chars().all(|c| c.is_ascii_alphanumeric() || c == ' ')
}

/// A font family name for MS Shell Dlg to stand for: plain text, never a path.
pub fn is_family_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().count() <= 64
        && name.trim() == name
        && !name.chars().any(|c| c.is_control() || matches!(c, '\\' | '/' | '"' | ';' | ':' | ',' | '@'))
}

/// Check `plan` before the administrator step does anything with it. `staged` is the one folder
/// its files may come from; `known` the Segoe UI value names this PC has (from the backup or the
/// registry), the only existing values it may point elsewhere.
pub fn check_plan(plan: &MachinePlan, generation: &str, staged: &Path, known: &[String]) -> Result<(), String> {
    let bad = |why: &str| Err(format!("The plan was refused: {why}."));
    if !is_generation(generation) || plan.generation != generation {
        return bad("it is not the plan that was just built");
    }
    if plan.files.is_empty() || plan.files.len() > MAX_FILES {
        return bad("it has no faces, or too many");
    }
    let mut names = HashSet::new();
    for file in &plan.files {
        if !is_file_name(&file.file_name, generation) {
            return bad("a file is not one of the replacement faces");
        }
        if !names.insert(file.file_name.to_ascii_lowercase()) {
            return bad("a file is listed twice");
        }
        // Exactly the file in the expected folder: no other folder, and no way out of it.
        if file.staged != staged.join(&file.file_name) {
            return bad("a file is not in the folder it was built in");
        }
        if !known.iter().any(|k| k == &file.value) {
            return bad("a face is for a value this PC does not have");
        }
    }
    let is_file = |data: &str| plan.files.iter().any(|f| f.file_name == data);
    let mut values = HashSet::new();
    for (name, data) in &plan.registry.set {
        if !known.iter().any(|k| k == name) || !values.insert(name.as_str()) {
            return bad("it changes a value that is not a Segoe UI face");
        }
        if !is_file(data) {
            return bad("a value would point at a file the plan does not install");
        }
    }
    for (name, data) in &plan.registry.extras {
        if !is_extra_value(name) || !values.insert(name.as_str()) {
            return bad("it adds a value that is not a Segoe UI Variable face");
        }
        if !is_file(data) {
            return bad("an added value would point at a file the plan does not install");
        }
    }
    let used = |file: &str| plan.registry.set.iter().chain(&plan.registry.extras).any(|(_, d)| d == file);
    if !plan.files.iter().all(|f| used(&f.file_name)) {
        return bad("a file would be installed that nothing uses");
    }
    let mut dialogs = HashSet::new();
    for (name, data) in &plan.registry.substitutes {
        if !DIALOG_FONTS.contains(&name.as_str()) || !dialogs.insert(name.as_str()) {
            return bad("it changes a font substitute other than MS Shell Dlg");
        }
        if !is_family_name(data) {
            return bad("MS Shell Dlg would stand for something that is not a font name");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fonts::segoe::Layer;
    use crate::plan::{BuiltFile, RegistryPlan};
    use std::path::PathBuf;

    const GEN: &str = "tob9y0";

    fn staged() -> PathBuf {
        PathBuf::from("C:\\Users\\u\\AppData\\Local\\Fontman\\staging").join(GEN)
    }

    fn known() -> Vec<String> {
        ["Segoe UI (TrueType)", "Segoe UI Bold (TrueType)", "Segoe UI Variable (TrueType)"].map(String::from).to_vec()
    }

    fn file(slug: &str, value: &str) -> BuiltFile {
        let file_name = format!("fontman-{GEN}-{slug}.ttf");
        BuiltFile {
            key: slug.into(),
            label: slug.into(),
            layer: if slug.starts_with("segoeuivar") { Layer::Variable } else { Layer::Segoe },
            value: value.into(),
            staged: staged().join(&file_name),
            file_name,
        }
    }

    fn good() -> MachinePlan {
        let files = vec![
            file("segoeui-regular", "Segoe UI (TrueType)"),
            file("segoeui-bold", "Segoe UI Bold (TrueType)"),
            file("segoeuivar-text-regular", "Segoe UI Variable (TrueType)"),
            file("segoeuivar-display-bold", "Segoe UI Variable (TrueType)"),
        ];
        MachinePlan {
            generation: GEN.into(),
            family: "Inter".into(),
            summary: "apps".into(),
            registry: RegistryPlan {
                set: vec![
                    ("Segoe UI (TrueType)".into(), files[0].file_name.clone()),
                    ("Segoe UI Bold (TrueType)".into(), files[1].file_name.clone()),
                    ("Segoe UI Variable (TrueType)".into(), files[2].file_name.clone()),
                ],
                extras: vec![("Fontman Segoe UI Variable Display Bold (TrueType)".into(), files[3].file_name.clone())],
                substitutes: vec![("MS Shell Dlg".into(), "Segoe UI".into()), ("MS Shell Dlg 2".into(), "Segoe UI".into())],
            },
            files,
        }
    }

    fn check(plan: &MachinePlan) -> Result<(), String> {
        check_plan(plan, GEN, &staged(), &known())
    }

    #[test]
    fn a_plan_as_built_is_accepted() {
        assert_eq!(check(&good()), Ok(()));
    }

    #[test]
    fn generations_are_plain_and_short() {
        assert!(is_generation("tob9y0"));
        for bad in ["", "TOB9Y0", "..", "a/b", "a\\b", "a b", "12345678901234"] {
            assert!(!is_generation(bad), "{bad:?}");
        }
    }

    #[test]
    fn file_names_follow_the_build() {
        assert!(is_file_name("fontman-tob9y0-segoeui-semibold-italic.ttf", GEN));
        assert!(is_file_name("fontman-tob9y0-segoeuivar-small-light.ttf", GEN));
        for bad in [
            "fontman-other-segoeui-regular.ttf",
            "fontman-tob9y0-segoeui-.ttf",
            "fontman-tob9y0-segoeui-regular.otf",
            "fontman-tob9y0-arial-regular.ttf",
            "fontman-tob9y0-segoeui-..\\..\\x.ttf",
            "fontman-tob9y0-segoeui-a/b.ttf",
            "segoeui.ttf",
            "C:\\Windows\\Fonts\\fontman-tob9y0-segoeui-regular.ttf",
        ] {
            assert!(!is_file_name(bad, GEN), "{bad:?}");
        }
    }

    #[test]
    fn another_generation_is_refused() {
        assert!(check_plan(&good(), "zzz999", &staged(), &known()).is_err());
        let mut plan = good();
        plan.generation = "zzz999".into();
        assert!(check(&plan).is_err());
    }

    #[test]
    fn files_from_anywhere_else_are_refused() {
        let mut plan = good();
        plan.files[0].staged = PathBuf::from("C:\\Temp").join(&plan.files[0].file_name);
        assert!(check(&plan).is_err());
        let mut plan = good();
        plan.files[0].staged = staged().join("..").join(&plan.files[0].file_name);
        assert!(check(&plan).is_err());
        let mut plan = good();
        plan.files.push(plan.files[0].clone());
        assert!(check(&plan).is_err(), "a file twice");
    }

    #[test]
    fn only_known_segoe_values_may_change() {
        let mut plan = good();
        plan.registry.set.push(("Arial (TrueType)".into(), plan.files[0].file_name.clone()));
        assert!(check(&plan).is_err());
        let mut plan = good();
        plan.files[0].value = "Segoe UI Emoji (TrueType)".into();
        assert!(check(&plan).is_err());
        let mut plan = good();
        plan.registry.extras[0].0 = "Fontman Arial (TrueType)".into();
        assert!(check(&plan).is_err());
        let mut plan = good();
        plan.registry.extras[0].0 = "Fontman Segoe UI Variable ..\\Run (TrueType)".into();
        assert!(check(&plan).is_err());
    }

    #[test]
    fn values_point_only_at_the_plans_files() {
        let mut plan = good();
        plan.registry.set[0].1 = "C:\\evil.ttf".into();
        assert!(check(&plan).is_err());
        let mut plan = good();
        plan.registry.set[0].1 = "segoeui.ttf".into();
        assert!(check(&plan).is_err());
        let mut plan = good();
        plan.registry.extras.clear();
        assert!(check(&plan).is_err(), "a file nothing uses");
    }

    #[test]
    fn only_the_dialog_fonts_are_substituted_and_only_with_names() {
        let mut plan = good();
        plan.registry.substitutes.push(("Tahoma".into(), "Inter".into()));
        assert!(check(&plan).is_err());
        let mut plan = good();
        plan.registry.substitutes[0].1 = "C:\\Fonts\\x.ttf".into();
        assert!(check(&plan).is_err());
        let mut plan = good();
        plan.registry.substitutes[0].1 = String::new();
        assert!(check(&plan).is_err());
        assert!(is_family_name("SF Pro Display"));
    }

    #[test]
    fn an_empty_plan_is_refused() {
        let mut plan = good();
        plan.files.clear();
        plan.registry = RegistryPlan::default();
        assert!(check(&plan).is_err());
    }
}
