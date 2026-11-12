//! What of the shell's menu Files shows, and what a pick on it means. Entries Files has its own
//! version of are dropped, matched by verb (labels are translated, verbs are not), and nothing
//! that would open a folder in Explorer is ever run by the shell.

use super::model::{Cascade, Item};

/// Verbs Files already offers in its own part of the menu.
const NATIVE: &[&str] = &[
    "open",
    "cut",
    "copy",
    "paste",
    // Files' own "Paste shortcut".
    "pastelink",
    "delete",
    "rename",
    "properties",
    // Files' "Copy path" quotes paths the same way.
    "copyaspath",
    // The background's Refresh and New > Folder.
    "refresh",
    "newfolder",
];

/// Verbs that open a folder window. Files has its own way to do each (open, open in a new tab),
/// so the shell's never shows; run by the shell they would start Explorer.
const OPENS_EXPLORER: &[&str] = &[
    "explore",
    "opennewwindow",
    "opennewtab",
    "opennewprocess",
    // Explorer's search window.
    "find",
    // "Extract All..." on a zip: its wizard ends by opening the extracted folder in Explorer,
    // ticked by default.
    "extract",
];

/// What happens when an entry is picked.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Route {
    /// The shell runs it.
    Invoke,
    /// Files opens the target folders itself.
    OpenFolders,
    /// Files opens the target folders in new tabs.
    OpenFoldersInTabs,
    /// Files renames in place.
    Rename,
    /// Files shows the item a shortcut points to, in its folder.
    OpenTargetFolder,
}

/// How a picked verb is handled. A safety net under `tidy`: the folder verbs are dropped from the
/// menu already, but whatever still carries one is turned around here rather than handed to
/// Windows.
pub fn route(verb: Option<&str>, folders_only: bool) -> Route {
    match verb {
        Some("rename") => Route::Rename,
        Some("openfilelocation" | "opencontaining") => Route::OpenTargetFolder,
        Some("open" | "explore" | "find") if folders_only => Route::OpenFolders,
        Some("opennewwindow" | "opennewtab" | "opennewprocess") => Route::OpenFoldersInTabs,
        _ => Route::Invoke,
    }
}

/// Whether Files hides this entry: it has its own, or the shell's would open Explorer.
pub fn hidden(verb: Option<&str>) -> bool {
    verb.is_some_and(|v| NATIVE.contains(&v) || OPENS_EXPLORER.contains(&v))
}

/// The shell's entries as Files shows them: its own duplicates and the Explorer openers gone,
/// submenus that came out empty gone, separators only between groups (none first, last, or
/// twice in a row), at every depth.
pub fn tidy(items: Vec<Item>) -> Vec<Item> {
    let kept = items
        .into_iter()
        .filter(|i| !hidden(i.verb.as_deref()))
        .map(|mut i| {
            i.submenu = tidy(std::mem::take(&mut i.submenu));
            i
        })
        .filter(|i| !(i.cascade == Cascade::Filled && i.submenu.is_empty()));
    collapse_separators(kept)
}

/// No leading, trailing or doubled separators.
pub fn collapse_separators(items: impl IntoIterator<Item = Item>) -> Vec<Item> {
    let mut out: Vec<Item> = Vec::new();
    for item in items {
        if item.separator && out.last().is_none_or(|l| l.separator) {
            continue;
        }
        out.push(item);
    }
    while out.last().is_some_and(|l| l.separator) {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verb(v: &str) -> Item {
        Item { label: v.to_uppercase(), verb: Some(v.into()), ..Default::default() }
    }
    fn label(l: &str) -> Item {
        Item { label: l.into(), ..Default::default() }
    }
    fn names(items: &[Item]) -> Vec<String> {
        items
            .iter()
            .map(|i| if i.separator { "-".into() } else { i.title() })
            .collect()
    }

    #[test]
    fn duplicates_go_by_verb_not_by_label() {
        // A translated "Copy" still has the verb "copy"; a third-party "Copy" with another verb
        // stays.
        let mut german = verb("copy");
        german.label = "Kopieren".into();
        let items = vec![verb("open"), german, verb("7zip.copy"), verb("properties"), label("Share")];
        assert_eq!(names(&tidy(items)), ["7ZIP.COPY", "Share"]);
    }

    #[test]
    fn explorer_openers_are_hidden() {
        let items = vec![verb("opennewwindow"), verb("explore"), verb("find"), verb("runas")];
        assert_eq!(names(&tidy(items)), ["RUNAS"]);
    }

    #[test]
    fn separators_collapse_at_every_depth() {
        let s = Item::separator;
        let mut new = label("New");
        new.cascade = Cascade::Filled;
        new.submenu = vec![verb("newfolder"), s(), label("Text Document"), s()];
        let items = vec![
            s(),
            verb("open"),
            s(),
            s(),
            label("Open with"),
            s(),
            verb("cut"),
            verb("copy"),
            s(),
            new,
            s(),
        ];
        let out = tidy(items);
        assert_eq!(names(&out), ["Open with", "-", "New"]);
        assert_eq!(names(&out[2].submenu), ["Text Document"]);
    }

    #[test]
    fn submenus_that_came_out_empty_go_but_unfilled_ones_stay() {
        let sub = |label_: &str, cascade, submenu: Vec<Item>| Item {
            cascade,
            submenu,
            ..label(label_)
        };
        let items = vec![
            // Only a separator inside, which collapses away.
            sub("CRC SHA", Cascade::Filled, vec![Item::separator()]),
            sub("Give access to", Cascade::Filled, vec![]),
            sub("Send to", Cascade::Pending, vec![]),
            sub("Slow", Cascade::Deferred, vec![]),
        ];
        assert_eq!(names(&tidy(items)), ["Send to", "Slow"]);
    }

    #[test]
    fn picks_are_routed_away_from_explorer() {
        assert_eq!(route(Some("opencontaining"), false), Route::OpenTargetFolder);
        assert!(hidden(Some("extract")));
        assert_eq!(route(Some("open"), true), Route::OpenFolders);
        assert_eq!(route(Some("explore"), true), Route::OpenFolders);
        assert_eq!(route(Some("opennewwindow"), false), Route::OpenFoldersInTabs);
        assert_eq!(route(Some("rename"), false), Route::Rename);
        assert_eq!(route(Some("openfilelocation"), false), Route::OpenTargetFolder);
        assert_eq!(route(Some("open"), false), Route::Invoke);
        assert_eq!(route(Some("7zip.add"), true), Route::Invoke);
        assert_eq!(route(None, true), Route::Invoke);
    }
}
