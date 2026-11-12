//! Reading font files: names, weight, style, and variation axes, without loading the glyphs.

pub mod catalog;
pub mod names;
pub mod rewrite;
pub mod segoe;
pub mod validate;

use read_fonts::{FileRef, FontRef, TableProvider};
use serde::{Deserialize, Serialize};
use skrifa::{MetadataProvider, Tag};
use std::path::{Path, PathBuf};

/// A font file held in memory, mapped rather than read so a large file costs only what is touched.
pub struct FontFile {
    pub path: PathBuf,
    map: memmap2::Mmap,
}

impl FontFile {
    pub fn open(path: &Path) -> Result<Self, String> {
        let file = std::fs::File::open(path).map_err(|err| format!("{}: {err}", path.display()))?;
        // Safety: font files are not written while Windows has them registered, and every read
        // goes through bounds-checked parsing, so a file that changes underneath gives an error.
        let map = unsafe { memmap2::Mmap::map(&file) }
            .map_err(|err| format!("{}: {err}", path.display()))?;
        Ok(Self {
            path: path.to_path_buf(),
            map,
        })
    }

    pub fn bytes(&self) -> &[u8] {
        &self.map
    }

    pub fn face(&self, index: u32) -> Result<FontRef<'_>, String> {
        FontRef::from_index(self.bytes(), index)
            .map_err(|err| format!("{} (face {index}): {err}", self.path.display()))
    }

    /// How many faces the file holds: one, or several for a .ttc collection.
    pub fn face_count(&self) -> u32 {
        match FileRef::new(self.bytes()) {
            Ok(FileRef::Font(_)) => 1,
            Ok(FileRef::Collection(collection)) => collection.len(),
            Err(_) => 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Outlines {
    TrueType,
    Cff,
    Cff2,
    /// Bitmap-only or otherwise without outlines Fontman can redraw.
    Other,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AxisInfo {
    pub tag: String,
    pub min: f32,
    pub default: f32,
    pub max: f32,
}

/// A named instance of a variable font, like "Semibold" at wght 600.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InstanceInfo {
    pub name: String,
    pub coords: Vec<(String, f32)>,
}

/// One face of a font file, as far as choosing and mapping it needs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FaceInfo {
    pub path: PathBuf,
    pub index: u32,
    /// The family people know it by: the typographic family, or the legacy one.
    pub family: String,
    /// Name ID 1, which may carry the weight, like "Segoe UI Semibold".
    pub legacy_family: String,
    /// Typographic subfamily, or the legacy one: "Semibold Italic".
    pub style: String,
    pub full_name: String,
    pub postscript: String,
    pub weight: u16,
    pub width: u16,
    pub italic: bool,
    pub axes: Vec<AxisInfo>,
    pub instances: Vec<InstanceInfo>,
    pub outlines: Outlines,
    /// Covers the basic Latin letters, so it can be a UI font at all.
    pub latin: bool,
    /// OS/2 embedding bits. 0x0002 means the license restricts it.
    pub fs_type: u16,
}

impl FaceInfo {
    pub fn axis(&self, tag: &str) -> Option<&AxisInfo> {
        self.axes.iter().find(|axis| axis.tag == tag)
    }

    pub fn is_variable(&self) -> bool {
        !self.axes.is_empty()
    }

    /// The weights this face can give: a range for a variable font, one value otherwise.
    pub fn weight_range(&self) -> (f32, f32) {
        match self.axis("wght") {
            Some(axis) => (axis.min, axis.max),
            None => (self.weight as f32, self.weight as f32),
        }
    }

    /// Whether this face can be drawn upright and slanted, through an ital or slnt axis.
    pub fn italic_axis(&self) -> bool {
        self.axis("ital").is_some_and(|axis| axis.max >= 1.0)
            || self.axis("slnt").is_some_and(|axis| axis.min < 0.0)
    }

    pub fn restricted(&self) -> bool {
        self.fs_type & 0x000F == 0x0002
    }
}

/// Every face in a font file.
pub fn read_faces(path: &Path) -> Result<Vec<FaceInfo>, String> {
    let file = FontFile::open(path)?;
    let mut faces = Vec::new();
    for index in 0..file.face_count() {
        if let Ok(font) = file.face(index) {
            if let Some(info) = face_info(&font, path, index) {
                faces.push(info);
            }
        }
    }
    if faces.is_empty() {
        return Err(format!(
            "{} is not a font Fontman can read.",
            path.display()
        ));
    }
    Ok(faces)
}

pub fn face_info(font: &FontRef, path: &Path, index: u32) -> Option<FaceInfo> {
    let name = |id: u16| names::get(font, id);
    let legacy_family = name(1)?;
    let family = name(16).unwrap_or_else(|| legacy_family.clone());
    let style = name(17)
        .or_else(|| name(2))
        .unwrap_or_else(|| "Regular".into());
    let os2 = font.os2().ok();
    let weight = os2.as_ref().map(|os2| os2.us_weight_class()).unwrap_or(400);
    let width = os2.as_ref().map(|os2| os2.us_width_class()).unwrap_or(5);
    let fs_selection = os2
        .as_ref()
        .map(|os2| os2.fs_selection().bits())
        .unwrap_or(0);
    let mac_style = font.head().map(|head| head.mac_style().bits()).unwrap_or(0);
    let italic = fs_selection & 1 != 0 || mac_style & 2 != 0;
    let fs_type = os2.as_ref().map(|os2| os2.fs_type()).unwrap_or(0);
    let axes: Vec<AxisInfo> = font
        .axes()
        .iter()
        .map(|axis| AxisInfo {
            tag: axis.tag().to_string(),
            min: axis.min_value(),
            default: axis.default_value(),
            max: axis.max_value(),
        })
        .collect();
    let tags: Vec<String> = axes.iter().map(|axis| axis.tag.clone()).collect();
    let instances = font
        .named_instances()
        .iter()
        .map(|instance| InstanceInfo {
            name: names::get(font, instance.subfamily_name_id().to_u16()).unwrap_or_default(),
            coords: tags.iter().cloned().zip(instance.user_coords()).collect(),
        })
        .collect();
    let outlines = if font.table_data(Tag::new(b"glyf")).is_some() {
        Outlines::TrueType
    } else if font.table_data(Tag::new(b"CFF ")).is_some() {
        Outlines::Cff
    } else if font.table_data(Tag::new(b"CFF2")).is_some() {
        Outlines::Cff2
    } else {
        Outlines::Other
    };
    let charmap = font.charmap();
    let latin = ['A', 'Z', 'a', 'z', '0', '9', '.', ' ']
        .into_iter()
        .all(|ch| charmap.map(ch).is_some_and(|gid| gid.to_u32() != 0));
    Some(FaceInfo {
        path: path.to_path_buf(),
        index,
        full_name: name(4).unwrap_or_else(|| format!("{family} {style}")),
        postscript: name(6).unwrap_or_default(),
        family,
        legacy_family,
        style,
        weight,
        width,
        italic,
        axes,
        instances,
        outlines,
        latin,
        fs_type,
    })
}

/// Whether egui can draw with these bytes. Its font loader panics on a font it cannot parse, so
/// a window checks before it hands a face over. (From Fontman's window.)
pub fn usable(bytes: &[u8], index: u32) -> bool {
    use ab_glyph::Font;
    match ab_glyph::FontRef::try_from_slice_and_index(bytes, index) {
        Ok(font) => font.units_per_em().is_some() && font.glyph_id('A').0 != 0,
        Err(_) => false,
    }
}

/// The Windows fonts folder.
pub fn windows_fonts_dir() -> PathBuf {
    let windir = std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into());
    PathBuf::from(windir).join("Fonts")
}

/// A registry font value's data as a full path: bare file names live in the fonts folder.
pub fn resolve_font_path(data: &str) -> PathBuf {
    let path = PathBuf::from(data);
    if path.is_absolute() {
        path
    } else {
        windows_fonts_dir().join(path)
    }
}
