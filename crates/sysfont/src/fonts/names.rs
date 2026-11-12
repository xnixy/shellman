//! The name table: reading a font's names, and writing a new table with the names it should
//! have under Windows.

use read_fonts::{FontRef, TableProvider};
use serde::{Deserialize, Serialize};

const WINDOWS: u16 = 3;
const UNICODE_BMP: u16 = 1;
const ENGLISH_US: u16 = 0x0409;

/// The name a face gets from the name table, preferring US English on the Windows platform.
pub fn get(font: &FontRef, id: u16) -> Option<String> {
    let name = font.name().ok()?;
    let data = name.string_data();
    let mut best: Option<(u8, String)> = None;
    for record in name.name_record() {
        if record.name_id().to_u16() != id {
            continue;
        }
        let rank = match (
            record.platform_id(),
            record.encoding_id(),
            record.language_id(),
        ) {
            (WINDOWS, _, ENGLISH_US) => 0,
            (WINDOWS, _, _) => 2,
            (0, _, _) => 3,
            (1, 0, 0) => 1,
            _ => 4,
        };
        if best.as_ref().is_some_and(|(have, _)| *have <= rank) {
            continue;
        }
        let Ok(text) = record.string(data) else {
            continue;
        };
        let text: String = text.chars().collect();
        let text = text.trim().to_string();
        if !text.is_empty() {
            best = Some((rank, text));
        }
    }
    best.map(|(_, text)| text)
}

/// The names Windows matches a face by. These are copied from the Segoe UI face being replaced,
/// so the replacement answers to exactly the same requests.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NameSet {
    /// ID 1, "Segoe UI Semibold".
    pub family: String,
    /// ID 2, one of Regular, Italic, Bold, Bold Italic.
    pub subfamily: String,
    /// ID 4.
    pub full: String,
    /// ID 6.
    pub postscript: String,
    /// ID 16, "Segoe UI", when it differs from ID 1.
    pub typo_family: Option<String>,
    /// ID 17, "Semibold Italic".
    pub typo_subfamily: Option<String>,
    /// ID 21 and 22, the weight-width-slope family, for faces whose typographic family also
    /// varies by something else, like optical size.
    pub wws_family: Option<String>,
    pub wws_subfamily: Option<String>,
}

impl NameSet {
    pub fn from_font(font: &FontRef) -> Option<Self> {
        Some(Self {
            family: get(font, 1)?,
            subfamily: get(font, 2).unwrap_or_else(|| "Regular".into()),
            full: get(font, 4).unwrap_or_default(),
            postscript: get(font, 6).unwrap_or_default(),
            typo_family: get(font, 16),
            typo_subfamily: get(font, 17),
            wws_family: get(font, 21),
            wws_subfamily: get(font, 22),
        })
    }
}

/// Name IDs carried over from the source font: copyright, version, trademark, the makers,
/// description, links, and license. Everything that identifies the face comes from the target.
const KEPT_FROM_SOURCE: &[u16] = &[0, 5, 7, 8, 9, 10, 11, 12, 13, 14, 19];

/// A complete format 0 name table, Windows Unicode only, with the target's identifying names and
/// the source's credits. `note` is added to the description so the file says where it came from.
pub fn build_table(source: &FontRef, names: &NameSet, note: &str) -> Vec<u8> {
    let mut records: Vec<(u16, String)> = Vec::new();
    for &id in KEPT_FROM_SOURCE {
        if let Some(text) = get(source, id) {
            if id == 10 {
                continue;
            }
            records.push((id, text));
        }
    }
    let description = match get(source, 10) {
        Some(text) => format!("{note} {text}"),
        None => note.to_string(),
    };
    records.push((10, description));
    records.push((1, names.family.clone()));
    records.push((2, names.subfamily.clone()));
    records.push((3, format!("Fontman: {}", names.full)));
    records.push((4, names.full.clone()));
    records.push((6, names.postscript.clone()));
    let optional = [
        (16, &names.typo_family),
        (17, &names.typo_subfamily),
        (21, &names.wws_family),
        (22, &names.wws_subfamily),
    ];
    for (id, value) in optional {
        if let Some(text) = value {
            records.push((id, text.clone()));
        }
    }
    records.sort_by_key(|(id, _)| *id);
    encode(&records)
}

fn encode(records: &[(u16, String)]) -> Vec<u8> {
    let mut strings: Vec<u8> = Vec::new();
    let mut header = Vec::new();
    let count = records.len() as u16;
    let storage_offset = 6 + 12 * count;
    header.extend_from_slice(&0u16.to_be_bytes());
    header.extend_from_slice(&count.to_be_bytes());
    header.extend_from_slice(&storage_offset.to_be_bytes());
    for (id, text) in records {
        let encoded: Vec<u8> = text.encode_utf16().flat_map(u16::to_be_bytes).collect();
        // Name strings are limited to what a 16-bit length can hold.
        let encoded = &encoded[..encoded.len().min(0xFFFE)];
        let offset = strings.len() as u16;
        for value in [
            WINDOWS,
            UNICODE_BMP,
            ENGLISH_US,
            *id,
            encoded.len() as u16,
            offset,
        ] {
            header.extend_from_slice(&value.to_be_bytes());
        }
        strings.extend_from_slice(encoded);
    }
    header.extend_from_slice(&strings);
    header
}

/// A PostScript name: printable ASCII without spaces or the characters it reserves, at most 63.
pub fn postscript_safe(text: &str) -> String {
    text.chars()
        .filter(|ch| ch.is_ascii_graphic() && !"[](){}<>/%".contains(*ch))
        .take(63)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn postscript_names_drop_spaces_and_reserved_characters() {
        assert_eq!(
            postscript_safe("Segoe UI-Semi Bold(x)"),
            "SegoeUI-SemiBoldx"
        );
        assert_eq!(postscript_safe(&"a".repeat(80)).len(), 63);
    }

    #[test]
    fn encoded_table_has_header_records_and_strings() {
        let bytes = encode(&[(1, "Ab".into()), (2, "C".into())]);
        assert_eq!(&bytes[0..6], &[0, 0, 0, 2, 0, 30]);
        // Second record's string starts after the first's four bytes.
        assert_eq!(&bytes[6 + 12 + 10..6 + 12 + 12], &[0, 4]);
        assert_eq!(&bytes[30..], &[0, b'A', 0, b'b', 0, b'C']);
    }
}
