//! One installed program as the page lists it, and what may be done to it. Pure.

use super::act::Action;
use super::cmdline::{self, Launch};
use super::format;
use std::path::PathBuf;

/// What a classic (Win32) program's Uninstall key says about removing and changing it.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Classic {
    /// The command line to remove it (UninstallString, or the quiet one when that is missing).
    pub uninstall: Option<String>,
    pub modify: Option<String>,
    /// The product code, when Windows Installer owns the program.
    pub msi: Option<String>,
    pub no_remove: bool,
    pub no_modify: bool,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Kind {
    Classic(Classic),
    /// A Store (MSIX) app, removed by its package full name. `removable` is false for the
    /// system components Windows does not let a person remove.
    Packaged { full_name: String, removable: bool },
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct App {
    /// Tells the app apart between refreshes.
    pub id: String,
    pub name: String,
    pub version: String,
    pub publisher: String,
    pub size_kb: Option<u64>,
    /// `yyyymmdd`.
    pub date: Option<u32>,
    /// A file to take the icon from: a program, an .ico, or a package's logo.
    pub icon: Option<PathBuf>,
    pub kind: Kind,
}

impl App {
    /// What Uninstall does, or None when the program cannot be removed from here.
    pub fn uninstall(&self) -> Option<Action> {
        match &self.kind {
            Kind::Packaged { removable: false, .. } => None,
            Kind::Packaged { full_name, .. } => Some(Action::RemovePackage(full_name.clone())),
            Kind::Classic(c) if c.no_remove => None,
            Kind::Classic(Classic { msi: Some(product), .. }) => Some(Action::Launch(cmdline::msi_uninstall(product))),
            Kind::Classic(c) => {
                let l = cmdline::parse(c.uninstall.as_deref()?)?;
                Some(Action::Launch(cmdline::msi_install_to_remove(l)))
            }
        }
    }

    /// What Modify does, or None when the program has no such thing.
    pub fn modify(&self) -> Option<Launch> {
        let Kind::Classic(c) = &self.kind else { return None };
        if c.no_modify {
            return None;
        }
        match (&c.modify, &c.msi) {
            (Some(path), _) => cmdline::parse(path),
            (None, Some(product)) => Some(cmdline::msi_modify(product)),
            (None, None) => None,
        }
    }

    /// The muted line under the name: publisher, version, size and date, whichever are known.
    pub fn detail(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        for text in [&self.publisher, &self.version] {
            if !text.is_empty() {
                parts.push(text.clone());
            }
        }
        parts.extend(self.size_kb.map(format::size));
        parts.extend(self.date.map(format::date));
        parts.join(" \u{b7} ")
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn classic(name: &str, c: Classic) -> App {
        App {
            id: name.to_string(),
            name: name.to_string(),
            version: String::new(),
            publisher: String::new(),
            size_kb: None,
            date: None,
            icon: None,
            kind: Kind::Classic(c),
        }
    }

    fn line(a: &App) -> Option<String> {
        match a.uninstall()? {
            Action::Launch(l) => Some(l.command_line()),
            Action::RemovePackage(n) => Some(format!("remove {n}")),
        }
    }

    #[test]
    fn msi_products_are_removed_by_product_code() {
        let a = classic("A", Classic { msi: Some("{G-1}".into()), uninstall: Some("MsiExec.exe /I{G-1}".into()), ..Default::default() });
        assert_eq!(line(&a).unwrap(), r#""msiexec.exe" /x {G-1}"#);
        assert_eq!(a.modify().unwrap().command_line(), r#""msiexec.exe" /i {G-1}"#);
    }

    #[test]
    fn others_run_their_uninstall_string() {
        let a = classic("A", Classic { uninstall: Some(r#""C:\p q\unins.exe" /x"#.into()), ..Default::default() });
        assert_eq!(line(&a).unwrap(), r#""C:\p q\unins.exe" /x"#);
        assert_eq!(a.modify(), None);
    }

    #[test]
    fn a_registered_msiexec_install_is_a_removal() {
        let a = classic("A", Classic { uninstall: Some("MsiExec.exe /I{G-2}".into()), ..Default::default() });
        assert_eq!(line(&a).unwrap(), r#""MsiExec.exe" /x{G-2}"#);
    }

    #[test]
    fn no_remove_and_no_modify_hide_the_buttons() {
        let c = Classic { msi: Some("{G}".into()), uninstall: Some("x.exe".into()), modify: Some("m.exe".into()), no_remove: true, no_modify: true };
        let a = classic("A", c);
        assert_eq!(a.uninstall(), None);
        assert_eq!(a.modify(), None);
        assert_eq!(classic("B", Classic::default()).uninstall(), None);
    }

    #[test]
    fn a_modify_path_is_run_as_it_is() {
        let c = Classic { modify: Some("MsiExec.exe /I{G-3}".into()), ..Default::default() };
        assert_eq!(classic("A", c).modify().unwrap().command_line(), r#""MsiExec.exe" /I{G-3}"#);
    }

    #[test]
    fn packaged_apps_are_removed_by_full_name() {
        let mut a = classic("A", Classic::default());
        a.kind = Kind::Packaged { full_name: "Pub.App_1.0_x64__abc".into(), removable: true };
        assert_eq!(line(&a).unwrap(), "remove Pub.App_1.0_x64__abc");
        assert_eq!(a.modify(), None);
        a.kind = Kind::Packaged { full_name: "X_1".into(), removable: false };
        assert_eq!(a.uninstall(), None);
    }

    #[test]
    fn the_detail_leaves_out_what_is_missing() {
        let mut a = classic("A", Classic::default());
        assert_eq!(a.detail(), "");
        a.publisher = "Acme".into();
        a.version = "1.2".into();
        assert_eq!(a.detail(), "Acme \u{b7} 1.2");
        a.size_kb = Some(2048);
        a.date = Some(20240102);
        assert_eq!(a.detail(), "Acme \u{b7} 1.2 \u{b7} 2.0 MB \u{b7} 2024-01-02");
        a.publisher.clear();
        assert_eq!(a.detail(), "1.2 \u{b7} 2.0 MB \u{b7} 2024-01-02");
    }
}
