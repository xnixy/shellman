//! Putting the chosen font into egui. What to load is worked out by a pure function (`resolve`),
//! then loaded only when it differs from what the window already wears: a font atlas rebuild is
//! expensive, and `Appearance::apply` runs on every change of the look.
//!
//! A variable font (like SegUIVar.ttf) draws as its default instance: egui cannot pick an axis
//! value, so weights come only from separate files.

use super::catalog::{Face, Family};
use super::choice::{FontChoice, SF_DEFAULT};
use super::{source, tweak};
use egui::{FontData, FontDefinitions, FontFamily, FontTweak};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Clone, PartialEq, Debug)]
pub struct Source {
    path: PathBuf,
    index: u32,
}

impl From<&Face> for Source {
    fn from(f: &Face) -> Self {
        Self { path: f.path.clone(), index: f.index }
    }
}

/// Everything the loaded fonts depend on: equal plans give equal fonts.
#[derive(Clone, PartialEq, Debug)]
pub struct Plan {
    /// The family the body font belongs to (its tweak applies), or "Segoe UI".
    family: String,
    regular: Option<Source>,
    /// None: the regular face stands in for it.
    semibold: Option<Source>,
    /// A light face for large numerals (the widgets' date); None: the regular face.
    light: Option<Source>,
    /// SF Mono, when the SF default is in use and it is installed.
    mono: Option<Source>,
    /// Segoe UI, behind the chosen font for glyphs it lacks.
    fallback: Option<Source>,
}

fn segoe_plan(dir: &Path, exists: &impl Fn(&Path) -> bool) -> Plan {
    let pick = |names: &[&str]| {
        names
            .iter()
            .map(|n| dir.join(n))
            .find(|p| exists(p))
            .map(|path| Source { path, index: 0 })
    };
    let regular = pick(&["SegUIVar.ttf", "segoeui.ttf"]);
    let variable = regular.as_ref().is_some_and(|s| s.path.ends_with("SegUIVar.ttf"));
    Plan {
        family: if variable { "Segoe UI Variable" } else { "Segoe UI" }.into(),
        semibold: pick(&["seguisb.ttf"]),
        light: pick(&["segoeuil.ttf", "segoeuisl.ttf"]),
        mono: None,
        // The chosen font is Segoe already.
        fallback: None,
        regular,
    }
}

fn family_plan(f: &Family, dir: &Path, exists: &impl Fn(&Path) -> bool) -> Option<Plan> {
    let regular = Source::from(f.regular()?);
    let mut plan = segoe_plan(dir, exists);
    plan.fallback = plan.regular.take();
    plan.regular = Some(regular);
    plan.semibold = f.semibold().map(Source::from);
    if let Some(l) = f.light() {
        plan.light = Some(Source::from(l));
    }
    plan.family = f.name.clone();
    Some(plan)
}

/// What to load for `choice`. A named family that is not installed (or whose files are gone)
/// falls back to the default; the default is SF Pro Text when installed, else Segoe UI Variable.
pub fn resolve(
    choice: FontChoice,
    installed: &[Family],
    dir: &Path,
    exists: &impl Fn(&Path) -> bool,
) -> Plan {
    let find = |name: &str| {
        installed
            .iter()
            .find(|f| f.name.eq_ignore_ascii_case(name))
            .filter(|f| f.regular().is_some_and(|r| exists(&r.path)))
    };
    let named = match choice {
        FontChoice::Named(n) => find(n.as_str()),
        FontChoice::Default => None,
    };
    if let Some(plan) = named.and_then(|f| family_plan(f, dir, exists)) {
        return plan;
    }
    if let Some(mut plan) = find(SF_DEFAULT).and_then(|f| family_plan(f, dir, exists)) {
        plan.mono = installed
            .iter()
            .find(|f| f.name.eq_ignore_ascii_case("SF Mono"))
            .and_then(|f| f.regular())
            .filter(|r| exists(&r.path))
            .map(Source::from);
        return plan;
    }
    segoe_plan(dir, exists)
}

/// "SF Pro" or "Segoe UI Variable": what the Default entry of the picker stands for.
pub fn default_label() -> &'static str {
    let exists = |p: &Path| p.exists();
    let plan = resolve(FontChoice::Default, source::installed(), &source::fonts_dir(), &exists);
    if plan.family == SF_DEFAULT { "SF Pro" } else { "Segoe UI Variable" }
}

fn read(src: &Source, tweak: FontTweak) -> Option<Arc<FontData>> {
    let mut data = FontData::from_owned(std::fs::read(&src.path).ok()?);
    data.index = src.index;
    data.tweak = tweak;
    Some(Arc::new(data))
}

/// Read the plan's files into font definitions. None when the body font cannot be read.
fn build(plan: &Plan, icons: &[&str]) -> Option<FontDefinitions> {
    let mut fonts = FontDefinitions::default();
    let t = tweak::for_family(&plan.family);
    let regular = read(plan.regular.as_ref()?, t)?;
    fonts.font_data.insert("text".into(), regular);
    let mut text = vec!["text".to_owned()];
    if let Some(d) = plan.fallback.as_ref().and_then(|s| read(s, tweak::for_family("Segoe UI Variable"))) {
        fonts.font_data.insert("fallback".into(), d);
        text.push("fallback".into());
    }
    let proportional = fonts.families.entry(FontFamily::Proportional).or_default();
    proportional.splice(0..0, text.iter().cloned());
    let mut semibold = Vec::new();
    if let Some(d) = plan.semibold.as_ref().and_then(|s| read(s, t)) {
        fonts.font_data.insert("semibold".into(), d);
        semibold.push("semibold".to_owned());
    }
    semibold.extend(fonts.families[&FontFamily::Proportional].iter().cloned());
    fonts.families.insert(FontFamily::Name("semibold".into()), semibold);
    let mut light = Vec::new();
    if let Some(d) = plan.light.as_ref().and_then(|s| read(s, t)) {
        fonts.font_data.insert("light".into(), d);
        light.push("light".to_owned());
    }
    light.extend(fonts.families[&FontFamily::Proportional].iter().cloned());
    fonts.families.insert(FontFamily::Name("light".into()), light);
    if let Some(d) = plan.mono.as_ref().and_then(|s| read(s, FontTweak::default())) {
        fonts.font_data.insert("mono".into(), d);
        fonts.families.entry(FontFamily::Monospace).or_default().insert(0, "mono".into());
    }
    for path in icons {
        if let Ok(bytes) = std::fs::read(path) {
            fonts.font_data.insert("icons".into(), Arc::new(FontData::from_owned(bytes)));
            fonts.families.insert(FontFamily::Name("icons".into()), vec!["icons".into()]);
            break;
        }
    }
    Some(fonts)
}

const ICON_FILES: [&str; 2] = [r"C:\Windows\Fonts\SegoeIcons.ttf", r"C:\Windows\Fonts\segmdl2.ttf"];

/// Make the window wear `choice`, unless it already does. Never panics: when the chosen files
/// cannot be read, the default is loaded, and when even that fails egui keeps its own fonts.
pub fn apply(ctx: &egui::Context, choice: FontChoice) {
    let exists = |p: &Path| p.exists();
    let planning = shellman_core::timing::span("fonts: installed list and plan");
    let plan = resolve(choice, source::installed(), &source::fonts_dir(), &exists);
    drop(planning);
    let id = egui::Id::new("shellman font plan");
    if ctx.data(|d| d.get_temp::<Plan>(id)).as_ref() == Some(&plan) {
        return;
    }
    let _reading = shellman_core::timing::span("fonts: read files");
    let fonts = build(&plan, &ICON_FILES)
        .or_else(|| build(&segoe_plan(&source::fonts_dir(), &exists), &ICON_FILES));
    if let Some(fonts) = fonts {
        crate::theme::mark_text_font_installed();
        ctx.set_fonts(fonts);
    }
    // Remembered even when it failed, so a broken file is not retried every frame.
    ctx.data_mut(|d| d.insert_temp(id, plan));
}

/// The first install of an app: the saved choice (or the default), read from the settings folder
/// without writing anything.
pub fn install_saved(ctx: &egui::Context) {
    apply(ctx, crate::appearance::saved_font());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fonts::catalog::{parse, Family};

    const DIR: &str = r"C:\Windows\Fonts";

    fn fams(list: &[(&str, &str)]) -> Vec<Family> {
        let e: Vec<_> = list.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        parse(&e, Path::new(DIR))
    }

    fn sf() -> Vec<Family> {
        fams(&[
            ("SF Pro Text Regular (OpenType)", r"C:\u\SFT-R.otf"),
            ("SF Pro Text Semibold (OpenType)", r"C:\u\SFT-S.otf"),
            ("SF Mono Regular (TrueType)", r"C:\u\SFM-R.otf"),
            ("Inter (TrueType)", r"C:\u\inter.ttf"),
        ])
    }

    fn all_exist(_: &Path) -> bool {
        true
    }

    #[test]
    fn default_is_sf_pro_text_with_sf_mono_and_segoe_behind() {
        let p = resolve(FontChoice::Default, &sf(), Path::new(DIR), &all_exist);
        assert_eq!(p.family, "SF Pro Text");
        assert!(p.regular.unwrap().path.ends_with("SFT-R.otf"));
        assert!(p.semibold.unwrap().path.ends_with("SFT-S.otf"));
        assert!(p.mono.unwrap().path.ends_with("SFM-R.otf"));
        assert!(p.fallback.unwrap().path.ends_with("SegUIVar.ttf"));
    }

    #[test]
    fn default_without_sf_is_segoe_variable() {
        let p = resolve(FontChoice::Default, &[], Path::new(DIR), &all_exist);
        assert_eq!(p.family, "Segoe UI Variable");
        assert!(p.regular.unwrap().path.ends_with("SegUIVar.ttf"));
        assert!(p.semibold.unwrap().path.ends_with("seguisb.ttf"));
        assert!(p.mono.is_none() && p.fallback.is_none());
    }

    #[test]
    fn segoe_without_the_variable_file_uses_the_plain_one() {
        let only = |p: &Path| p.ends_with("segoeui.ttf");
        let p = resolve(FontChoice::Default, &[], Path::new(DIR), &only);
        assert!(p.regular.unwrap().path.ends_with("segoeui.ttf"));
        assert!(p.semibold.is_none());
    }

    #[test]
    fn a_named_family_replaces_the_body_font_but_not_the_mono() {
        let p = resolve(FontChoice::named("inter"), &sf(), Path::new(DIR), &all_exist);
        assert_eq!(p.family, "Inter");
        assert!(p.regular.unwrap().path.ends_with("inter.ttf"));
        assert!(p.semibold.unwrap().path.ends_with("inter.ttf"));
        assert!(p.mono.is_none());
    }

    #[test]
    fn a_missing_family_or_file_falls_back_to_the_default() {
        let d = resolve(FontChoice::Default, &sf(), Path::new(DIR), &all_exist);
        assert_eq!(resolve(FontChoice::named("Nope"), &sf(), Path::new(DIR), &all_exist), d);
        let no_inter = |p: &Path| !p.ends_with("inter.ttf");
        let p = resolve(FontChoice::named("Inter"), &sf(), Path::new(DIR), &no_inter);
        assert_eq!(p.family, "SF Pro Text");
    }

    #[test]
    fn choosing_the_default_family_by_name_is_the_same_plan() {
        let a = resolve(FontChoice::Default, &sf(), Path::new(DIR), &all_exist);
        let b = resolve(FontChoice::named("SF Pro Text"), &sf(), Path::new(DIR), &all_exist);
        assert_eq!(a.regular, b.regular);
    }

    #[test]
    fn nothing_installed_gives_an_empty_plan() {
        let none = |_: &Path| false;
        let p = resolve(FontChoice::Default, &[], Path::new(DIR), &none);
        assert!(p.regular.is_none());
        assert!(build(&p, &[]).is_none());
    }
}
