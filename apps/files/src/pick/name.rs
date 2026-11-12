//! Names in the picker's name field: checking a name to save under, adding the default
//! extension, and the quoted lists that stand for several chosen files.

use std::path::Path;

/// Names Windows keeps for devices, which no file may have (with any extension).
const RESERVED: [&str; 22] = [
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Why a file name (the last part of a path) cannot be saved under, if it cannot.
pub fn check(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("Type a name to save under.".into());
    }
    if let Some(c) = name
        .chars()
        .find(|&c| r#"\/:*?"<>|"#.contains(c) || (c as u32) < 32)
    {
        return Err(if (c as u32) < 32 {
            "A name can't contain control characters.".into()
        } else {
            format!(r#"A name can't contain {c}. These are not allowed: \ / : * ? " < > |"#)
        });
    }
    if name.ends_with('.') || name.ends_with(' ') {
        return Err("A name can't end with a dot or a space.".into());
    }
    let stem = name.split('.').next().unwrap_or("").trim_end().to_lowercase();
    if RESERVED.contains(&stem.as_str()) {
        return Err(format!("{name} is a name Windows keeps for itself."));
    }
    Ok(())
}

/// The extension to add to a saved name that has none: the chosen type's own (`txt` for
/// `*.txt`) when it has one, else the app's default. None when the app gave no default, as
/// Windows adds nothing then either.
pub fn extension_to_add<'a>(default_ext: &'a str, type_ext: Option<&'a str>) -> Option<&'a str> {
    let default_ext = default_ext.trim().trim_start_matches('.');
    if default_ext.is_empty() {
        return None;
    }
    Some(type_ext.filter(|e| !e.is_empty()).unwrap_or(default_ext))
}

/// `name` with `.ext` added when it has no extension of its own.
pub fn with_extension(name: &str, ext: Option<&str>) -> String {
    match ext {
        Some(ext) if Path::new(name).extension().is_none() => format!("{name}.{ext}"),
        _ => name.to_owned(),
    }
}

/// The names in the field: several when quoted (`"a.txt" "b c.txt"`), else the one typed.
pub fn split(typed: &str) -> Vec<String> {
    let t = typed.trim();
    if !t.starts_with('"') {
        return if t.is_empty() { Vec::new() } else { vec![t.to_owned()] };
    }
    let mut out = Vec::new();
    let mut rest = t;
    while let Some(start) = rest.find('"') {
        let after = &rest[start + 1..];
        match after.find('"') {
            Some(end) => {
                let name = after[..end].trim();
                if !name.is_empty() {
                    out.push(name.to_owned());
                }
                rest = &after[end + 1..];
            }
            None => {
                // An unclosed quote: the rest is one name.
                let name = after.trim();
                if !name.is_empty() {
                    out.push(name.to_owned());
                }
                break;
            }
        }
    }
    out
}

/// How the field shows chosen names: one as it is, several each in quotes.
pub fn join(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        many => many
            .iter()
            .map(|n| format!("\"{n}\""))
            .collect::<Vec<_>>()
            .join(" "),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn good_names_pass() {
        for n in ["notes.txt", "a b", "résumé.docx", ".gitignore", "con-artist.txt", "x.con"] {
            assert!(check(n).is_ok(), "{n}");
        }
    }

    #[test]
    fn bad_names_say_why() {
        for n in ["", "  ", "a:b", "a*b", "a?", "<x>", "a|b", "a\"b", "tab\there", "dot.", "space "] {
            assert!(check(n).is_err(), "{n:?}");
        }
        assert!(check("a*b").unwrap_err().contains('*'));
        for n in ["CON", "nul.txt", "Com1.log", "lpt9"] {
            assert!(check(n).unwrap_err().contains("keeps"), "{n}");
        }
    }

    #[test]
    fn the_type_extension_wins_over_the_default_but_needs_one() {
        assert_eq!(extension_to_add("txt", Some("md")), Some("md"));
        assert_eq!(extension_to_add(".txt", None), Some("txt"));
        assert_eq!(extension_to_add("txt", Some("")), Some("txt"));
        // No default from the app: nothing is added, whatever the type.
        assert_eq!(extension_to_add("", Some("md")), None);
    }

    #[test]
    fn an_extension_is_added_only_when_missing() {
        assert_eq!(with_extension("notes", Some("txt")), "notes.txt");
        assert_eq!(with_extension("notes.md", Some("txt")), "notes.md");
        assert_eq!(with_extension("notes", None), "notes");
        assert_eq!(with_extension("v1.2 draft", Some("txt")), "v1.2 draft");
    }

    #[test]
    fn quoted_lists_split_and_join() {
        assert_eq!(split(r#""a.txt" "b c.txt""#), ["a.txt", "b c.txt"]);
        assert_eq!(split(r#""a.txt""b.txt""#), ["a.txt", "b.txt"]);
        assert_eq!(split("  plain name.txt "), ["plain name.txt"]);
        assert_eq!(split(r#""a.txt" "unclosed"#), ["a.txt", "unclosed"]);
        assert!(split("   ").is_empty());
        let names = vec!["a.txt".to_string(), "b c.txt".to_string()];
        assert_eq!(join(&names), r#""a.txt" "b c.txt""#);
        assert_eq!(split(&join(&names)), names);
        assert_eq!(join(&names[..1]), "a.txt");
        assert_eq!(join(&[]), "");
    }
}
