//! Turning the registry's font list into families with weights. Pure: the registry reader gives
//! it (value name, file) pairs, so it is tested with made-up entries.
//!
//! A registry value is named like `SF Pro Text Semibold (OpenType)` and points at a file
//! (relative names live in the Windows fonts folder). A collection (`.ttc`) is named
//! `Cambria & Cambria Math (TrueType)`: the n-th name is the n-th font in the file.

use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Weight {
    /// Light, thin and the like: never picked unless nothing else is there.
    Light,
    Regular,
    Medium,
    Semibold,
    Bold,
    /// Black, heavy, extra bold: kept out of the way.
    Heavy,
}

/// One font file (or one font of a collection) of a family.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Face {
    pub weight: Weight,
    pub path: PathBuf,
    /// Which font of a `.ttc` collection; 0 for a plain file.
    pub index: u32,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Family {
    pub name: String,
    pub faces: Vec<Face>,
}

impl Family {
    fn face(&self, w: Weight) -> Option<&Face> {
        self.faces.iter().find(|f| f.weight == w)
    }

    /// The body face: Regular, else Medium, else Light.
    pub fn regular(&self) -> Option<&Face> {
        [Weight::Regular, Weight::Medium, Weight::Light]
            .into_iter()
            .find_map(|w| self.face(w))
    }

    /// A light face, for large numerals, if the family has one.
    pub fn light(&self) -> Option<&Face> {
        self.face(Weight::Light)
    }

    /// The face for names and headings: Semibold, else Bold, else the regular one.
    pub fn semibold(&self) -> Option<&Face> {
        [Weight::Semibold, Weight::Bold]
            .into_iter()
            .find_map(|w| self.face(w))
            .or_else(|| self.regular())
    }
}

/// What a name says about its font, once the style words are taken off the end.
struct Parsed {
    family: String,
    weight: Option<Weight>,
    italic: bool,
}

fn style_word(w: &str) -> Option<(Option<Weight>, bool)> {
    let weight = |w| Some((Some(w), false));
    match w.to_ascii_lowercase().as_str() {
        "regular" | "normal" => weight(Weight::Regular),
        "italic" | "oblique" => Some((None, true)),
        "medium" => weight(Weight::Medium),
        "semibold" | "demibold" | "demi" => weight(Weight::Semibold),
        "bold" => weight(Weight::Bold),
        "light" | "semilight" | "thin" | "ultralight" | "extralight" => weight(Weight::Light),
        "heavy" | "black" | "extrabold" | "ultrabold" | "extrablack" | "ultrablack" => {
            weight(Weight::Heavy)
        }
        _ => None,
    }
}

fn parse_name(name: &str) -> Parsed {
    let mut words: Vec<&str> = name.split_whitespace().collect();
    let mut weight = None;
    let mut italic = false;
    while words.len() > 1 {
        let Some((w, i)) = style_word(words[words.len() - 1]) else { break };
        // "Bold Italic": the weight is the one nearest the family name.
        weight = weight.or(w);
        italic |= i;
        words.pop();
    }
    Parsed { family: words.join(" "), weight, italic }
}

/// The last dash-separated word of a file's name, as a weight (`...-semibold.ttf`).
fn weight_of_file(file: &str) -> Option<Weight> {
    let stem = Path::new(file).file_stem()?.to_str()?;
    style_word(stem.rsplit(['-', '_']).next()?)?.0
}

/// Families that are not for text: icons, symbols, emoji.
fn is_text_family(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    ![
        "symbol", "icon", "emoji", "dingbat", "webdings", "wingdings", "mdl2", "marlett", "holo",
        "historic",
    ]
    .iter()
    .any(|bad| n.contains(bad))
}

/// `(value name, file)` pairs of the registry's font keys into families, sorted by name.
/// `fonts_dir` is where relative files live. Italics, non-text families and families without a
/// regular face are left out. When two entries claim the same family and weight, the first wins.
pub fn parse(entries: &[(String, String)], fonts_dir: &Path) -> Vec<Family> {
    let mut families: Vec<Family> = Vec::new();
    for (value_name, file) in entries {
        let path = {
            let p = Path::new(file);
            if p.is_absolute() { p.to_path_buf() } else { fonts_dir.join(p) }
        };
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        if !matches!(ext.as_str(), "ttf" | "otf" | "ttc" | "otc") {
            continue;
        }
        // "Name (TrueType)": the parenthesis is the font type.
        let name = value_name.rsplit_once(" (").map_or(value_name.as_str(), |(n, _)| n);
        let name = name.strip_prefix("Fontman ").unwrap_or(name);
        for (index, part) in name.split(" & ").enumerate() {
            let p = parse_name(part);
            if p.italic || !is_text_family(&p.family) {
                continue;
            }
            // A name with no style word may still be a styled file (`Segoe UI Variable` points at
            // its Light instance); the file's name then tells the weight.
            let weight = p
                .weight
                .or_else(|| weight_of_file(file))
                .unwrap_or(Weight::Regular);
            let face = Face { weight, path: path.clone(), index: index as u32 };
            let key = p.family.to_ascii_lowercase();
            match families.iter_mut().find(|f| f.name.to_ascii_lowercase() == key) {
                Some(f) if f.face(weight).is_some() => {}
                Some(f) => f.faces.push(face),
                None => families.push(Family { name: p.family, faces: vec![face] }),
            }
        }
    }
    families.retain(|f| f.face(Weight::Regular).is_some());
    families.sort_by_key(|f| f.name.to_ascii_lowercase());
    families
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(list: &[(&str, &str)]) -> Vec<Family> {
        let entries: Vec<_> = list.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        parse(&entries, Path::new(r"C:\Windows\Fonts"))
    }

    #[test]
    fn groups_weights_and_skips_italics() {
        let f = run(&[
            ("SF Pro Text Regular (OpenType)", r"C:\u\SF-Pro-Text-Regular.otf"),
            ("SF Pro Text Semibold (OpenType)", r"C:\u\SF-Pro-Text-Semibold.otf"),
            ("SF Pro Text Semibold Italic (OpenType)", r"C:\u\SF-Pro-Text-SemiboldItalic.otf"),
            ("SF Pro Text Bold (OpenType)", r"C:\u\SF-Pro-Text-Bold.otf"),
            ("SF Pro Text Light (OpenType)", r"C:\u\SF-Pro-Text-Light.otf"),
        ]);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].name, "SF Pro Text");
        assert_eq!(f[0].faces.len(), 4);
        assert!(f[0].regular().unwrap().path.ends_with("SF-Pro-Text-Regular.otf"));
        assert!(f[0].semibold().unwrap().path.ends_with("SF-Pro-Text-Semibold.otf"));
    }

    #[test]
    fn semibold_falls_back_to_bold_then_regular() {
        let f = run(&[("Arial (TrueType)", "arial.ttf"), ("Arial Bold (TrueType)", "arialbd.ttf")]);
        assert!(f[0].semibold().unwrap().path.ends_with("arialbd.ttf"));
        let f = run(&[("Calibri (TrueType)", "calibri.ttf")]);
        assert!(f[0].semibold().unwrap().path.ends_with("calibri.ttf"));
        assert_eq!(f[0].regular(), f[0].semibold());
    }

    #[test]
    fn relative_files_live_in_the_fonts_folder() {
        let f = run(&[("Arial (TrueType)", "arial.ttf")]);
        assert_eq!(f[0].faces[0].path, Path::new(r"C:\Windows\Fonts\arial.ttf"));
    }

    #[test]
    fn collections_give_each_name_its_index() {
        let f = run(&[("Cambria & Cambria Math (TrueType)", "cambria.ttc")]);
        let cambria = f.iter().find(|f| f.name == "Cambria").unwrap();
        let math = f.iter().find(|f| f.name == "Cambria Math").unwrap();
        assert_eq!(cambria.faces[0].index, 0);
        assert_eq!(math.faces[0].index, 1);
    }

    #[test]
    fn skips_icons_symbols_and_other_files() {
        let f = run(&[
            ("Segoe UI Symbol (TrueType)", "seguisym.ttf"),
            ("Segoe MDL2 Assets (TrueType)", "segmdl2.ttf"),
            ("Segoe UI Emoji (TrueType)", "seguiemj.ttf"),
            ("Some Bitmap (VGA)", "vga.fon"),
            ("Slanted Italic (TrueType)", "x.ttf"),
        ]);
        assert!(f.is_empty(), "{f:?}");
    }

    #[test]
    fn a_family_needs_a_regular_face() {
        let f = run(&[("Thin Thing Bold (TrueType)", "a.ttf"), ("Thin Thing Light (TrueType)", "b.ttf")]);
        assert!(f.is_empty());
    }

    #[test]
    fn unstyled_name_takes_the_weight_from_the_file() {
        // The real registry points "Segoe UI Variable" at a Light file.
        let f = run(&[
            ("Segoe UI Variable (TrueType)", "fontman-x-segoeuivar-display-light.ttf"),
            ("Fontman Segoe UI Variable Text Regular (TrueType)", "fontman-x-segoeuivar-text-regular.ttf"),
        ]);
        assert_eq!(f.len(), 1, "{f:?}");
        assert_eq!(f[0].name, "Segoe UI Variable Text");
    }

    #[test]
    fn first_entry_wins_per_weight() {
        let f = run(&[("Foo (TrueType)", "a.ttf"), ("Foo Regular (TrueType)", "b.ttf")]);
        assert_eq!(f[0].faces.len(), 1);
        assert!(f[0].faces[0].path.ends_with("a.ttf"));
    }

    #[test]
    fn sorted_by_name_ignoring_case() {
        let f = run(&[("zed (TrueType)", "z.ttf"), ("Alpha (TrueType)", "a.ttf")]);
        assert_eq!(f[0].name, "Alpha");
    }
}
