//! The Settings list: one row per startup item that has a switch, named, sorted, with what is
//! known about its target. Pure: what a target looks like on disk comes in as a closure.

use super::items::{Item, Origin};
use std::path::{Path, PathBuf};

/// What the disk says about an item's target.
#[derive(Default, Clone, Debug)]
pub struct Found {
    /// The program's own description ("Microsoft OneDrive"), when it has one.
    pub description: Option<String>,
    /// The program to take the icon from.
    pub exe: Option<PathBuf>,
    /// The target is known and is not there any more.
    pub missing: bool,
}

#[derive(Clone, Debug)]
pub struct Row {
    pub item: Item,
    pub title: String,
    pub exe: Option<PathBuf>,
    pub missing: bool,
}

impl Row {
    /// Identifies the row between refreshes.
    pub fn key(&self) -> String {
        format!("{}|{}", self.item.place, self.item.name)
    }

    /// The muted second line: for everyone on the machine, then the command.
    pub fn detail(&self) -> String {
        let command = &self.item.command;
        if self.item.machine_wide() { format!("All users - {command}") } else { command.clone() }
    }
}

/// The program a command line starts: the quoted part, or the longest run of words that names a
/// file (a path with spaces is often not quoted), or else the first word.
pub fn exe_of_command(command: &str, exists: &dyn Fn(&Path) -> bool) -> Option<PathBuf> {
    let c = command.trim();
    if let Some(rest) = c.strip_prefix('"') {
        let end = rest.find('"').unwrap_or(rest.len());
        return Some(PathBuf::from(&rest[..end])).filter(|p| !p.as_os_str().is_empty());
    }
    let words: Vec<&str> = c.split(' ').collect();
    for n in (1..=words.len()).rev() {
        let candidate = words[..n].join(" ");
        if candidate.is_empty() {
            continue;
        }
        for p in [PathBuf::from(&candidate), PathBuf::from(format!("{candidate}.exe"))] {
            if exists(&p) {
                return Some(p);
            }
        }
    }
    words.first().filter(|w| !w.is_empty()).map(PathBuf::from)
}

/// A shortcut's file name without `.lnk`, the way Explorer shows it.
fn display_stem(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".lnk") || lower.ends_with(".url") { name[..name.len() - 4].to_string() } else { name.to_string() }
}

/// The rows for `items`: only those with a switch, sorted by name without regard to case.
pub fn build(items: Vec<Item>, probe: &dyn Fn(&Item) -> Found) -> Vec<Row> {
    let mut rows: Vec<Row> = items
        .into_iter()
        .filter(|i| i.area().is_some())
        .map(|item| {
            let found = probe(&item);
            let fallback = match item.origin {
                Origin::Folder { .. } => display_stem(&item.name),
                Origin::Run { .. } => item.name.clone(),
            };
            let title = found.description.filter(|d| !d.trim().is_empty()).unwrap_or(fallback);
            Row { item, title, exe: found.exe, missing: found.missing }
        })
        .collect();
    rows.sort_by_cached_key(|r| (r.title.to_lowercase(), r.item.name.to_lowercase()));
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{fake::Fake, Hive, Value};
    use crate::startup::items::{collect, FolderFile};

    fn exists_only(list: &'static [&'static str]) -> impl Fn(&Path) -> bool {
        move |p| list.iter().any(|l| Path::new(l) == p)
    }

    #[test]
    fn the_program_is_the_quoted_part() {
        let none = |_: &Path| false;
        assert_eq!(exe_of_command("\"C:\\A B\\x.exe\" /q", &none), Some("C:\\A B\\x.exe".into()));
        assert_eq!(exe_of_command("\"\"", &none), None);
    }

    #[test]
    fn an_unquoted_path_with_spaces_is_found_by_trying() {
        let e = exists_only(&["C:\\Program Files\\A\\a.exe"]);
        assert_eq!(exe_of_command("C:\\Program Files\\A\\a.exe -min", &e), Some("C:\\Program Files\\A\\a.exe".into()));
        let e = exists_only(&["C:\\Tools\\t.exe"]);
        assert_eq!(exe_of_command("C:\\Tools\\t", &e), Some("C:\\Tools\\t.exe".into()));
        assert_eq!(exe_of_command("gone.exe /x", &|_: &Path| false), Some("gone.exe".into()));
        assert_eq!(exe_of_command("  ", &|_: &Path| false), None);
    }

    #[test]
    fn rows_are_named_sorted_and_only_switchable_ones() {
        let reg = Fake::default();
        let cv = "Software\\Microsoft\\Windows\\CurrentVersion";
        reg.put(Hive::Hkcu, &format!("{cv}\\Run"), "zeta", Value::Text("z.exe".into()));
        reg.put(Hive::Hkcu, &format!("{cv}\\Run"), "Odd", Value::Text("o.exe".into()));
        reg.put(Hive::Hkcu, &format!("{cv}\\RunOnce"), "Once", Value::Text("x.exe".into()));
        let files = [FolderFile { common: true, path: "C:\\S\\Notes.lnk".into() }];
        let items = collect(&reg, &files, &|s| s.to_string(), true);
        let rows = build(items, &|i| match i.name.as_str() {
            "Odd" => Found { description: Some("Alpha Tool".into()), exe: Some("o.exe".into()), missing: true },
            _ => Found::default(),
        });
        let titles: Vec<_> = rows.iter().map(|r| r.title.as_str()).collect();
        assert_eq!(titles, ["Alpha Tool", "Notes", "zeta"]);
        assert!(rows[0].missing);
        assert_eq!(rows[1].detail(), "All users - C:\\S\\Notes.lnk");
        assert_eq!(rows[2].detail(), "z.exe");
    }
}
