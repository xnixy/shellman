//! Every font family that can be the new UI font: those Windows has installed, for all users or
//! just you, and font files added to Fontman without installing them.

use super::{read_faces, resolve_font_path, FaceInfo};
use crate::reg;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

/// Replacement files Fontman wrote are named like this, and never offered as a choice.
pub const OWN_PREFIX: &str = "fontman-";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Origin {
    Installed,
    Added,
}

#[derive(Clone, Debug)]
pub struct Family {
    pub name: String,
    pub faces: Vec<FaceInfo>,
    pub origin: Origin,
}

impl Family {
    pub fn is_variable(&self) -> bool {
        self.faces.iter().any(FaceInfo::is_variable)
    }

    /// The face to show the family with: regular weight, upright, normal width.
    pub fn regular(&self) -> Option<&FaceInfo> {
        self.faces.iter().min_by_key(|face| {
            let (low, high) = face.weight_range();
            let weight_gap = if (low..=high).contains(&400.0) {
                0.0
            } else {
                (low - 400.0).abs().min((high - 400.0).abs())
            };
            (
                face.italic as u8,
                (face.width as i32 - 5).abs(),
                weight_gap as i32,
                face.is_variable() as u8,
            )
        })
    }

    /// "4 styles", "Variable", or "Variable · 2 files".
    pub fn summary(&self) -> String {
        let files: HashSet<&Path> = self.faces.iter().map(|face| face.path.as_path()).collect();
        if self.is_variable() {
            let mut weights = String::new();
            if let Some((low, high)) = self
                .faces
                .iter()
                .filter(|face| face.is_variable())
                .map(FaceInfo::weight_range)
                .reduce(|a, b| (a.0.min(b.0), a.1.max(b.1)))
            {
                weights = format!(" · weights {low:.0}–{high:.0}");
            }
            return format!("Variable{weights}");
        }
        let count = self.faces.len();
        match (count, files.len()) {
            (1, _) => "1 style".into(),
            (n, _) => format!("{n} styles"),
        }
    }

    pub fn restricted(&self) -> bool {
        self.faces.iter().any(FaceInfo::restricted)
    }
}

/// Families Fontman does not offer: the faces it replaces, and fonts made only of symbols.
fn skipped_family(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower == "segoe ui" || lower == "segoe ui variable" || lower.starts_with("segoe ui variable ")
}

/// Font files registered with Windows, for the machine and for this user.
pub fn installed_files() -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut files = Vec::new();
    for root in [reg::HKEY_LOCAL_MACHINE, reg::HKEY_CURRENT_USER] {
        for (_, data) in reg::values(root, reg::FONTS_KEY) {
            let path = resolve_font_path(&data);
            let lower = path.to_string_lossy().to_ascii_lowercase();
            let is_outline = [".ttf", ".otf", ".ttc", ".otc"]
                .iter()
                .any(|ext| lower.ends_with(ext));
            let own = path.file_name().is_some_and(|name| {
                name.to_string_lossy()
                    .to_ascii_lowercase()
                    .starts_with(OWN_PREFIX)
            });
            if is_outline && !own && seen.insert(lower) {
                files.push(path);
            }
        }
    }
    files
}

/// Read every face of every file and group them into families, sorted by name.
pub fn families(installed: &[PathBuf], added: &[PathBuf]) -> Vec<Family> {
    let mut map: BTreeMap<String, Family> = BTreeMap::new();
    let groups = [(installed, Origin::Installed), (added, Origin::Added)];
    for (files, origin) in groups {
        for path in files {
            let Ok(faces) = read_faces(path) else {
                continue;
            };
            for face in faces {
                if !face.latin
                    || skipped_family(&face.family)
                    || face.outlines == super::Outlines::Other
                {
                    continue;
                }
                let key = format!("{}|{:?}", face.family.to_lowercase(), origin);
                let family = map.entry(key).or_insert_with(|| Family {
                    name: face.family.clone(),
                    faces: Vec::new(),
                    origin,
                });
                let duplicate = family.faces.iter().any(|have| {
                    have.style.eq_ignore_ascii_case(&face.style)
                        && have.is_variable() == face.is_variable()
                        && have.italic == face.italic
                });
                if !duplicate {
                    family.faces.push(face);
                }
            }
        }
    }
    let mut families: Vec<Family> = map.into_values().collect();
    for family in &mut families {
        family
            .faces
            .sort_by_key(|face| (face.is_variable(), face.italic, face.width, face.weight));
    }
    families.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then((a.origin == Origin::Added).cmp(&(b.origin == Origin::Added)))
    });
    families
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_replaced_families_are_not_offered() {
        assert!(skipped_family("Segoe UI"));
        assert!(skipped_family("Segoe UI Variable"));
        assert!(skipped_family("segoe ui variable display"));
        assert!(!skipped_family("Segoe Print"));
        assert!(!skipped_family("Inter"));
    }
}
