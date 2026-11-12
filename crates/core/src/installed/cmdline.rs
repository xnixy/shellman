//! Turning an UninstallString into a program and its arguments, the way Windows reads one. Pure.

/// A program to start and what to hand it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Launch {
    pub exe: String,
    pub args: String,
}

impl Launch {
    pub fn new(exe: &str, args: &str) -> Self {
        Self { exe: exe.to_string(), args: args.to_string() }
    }

    /// The whole command line, with the program quoted so a path with spaces is not misread.
    pub fn command_line(&self) -> String {
        if self.args.is_empty() { format!("\"{}\"", self.exe) } else { format!("\"{}\" {}", self.exe, self.args) }
    }

    fn is_msiexec(&self) -> bool {
        let name = self.exe.rsplit(['\\', '/']).next().unwrap_or_default().to_ascii_lowercase();
        name == "msiexec" || name == "msiexec.exe"
    }
}

/// `msiexec /x {product}`: remove a Windows Installer product.
pub fn msi_uninstall(product: &str) -> Launch {
    Launch::new("msiexec.exe", &format!("/x {product}"))
}

/// `msiexec /i {product}`: change (repair, add or remove features of) a Windows Installer product.
pub fn msi_modify(product: &str) -> Launch {
    Launch::new("msiexec.exe", &format!("/i {product}"))
}

/// Split a command line into program and arguments: a quoted program ends at its closing quote; an
/// unquoted one (paths with spaces are often not quoted) ends at the first `.exe` that is a whole
/// word; otherwise the first word is the program (`rundll32 shell32.dll,Fn`).
pub fn parse(command: &str) -> Option<Launch> {
    let c = command.trim();
    if let Some(rest) = c.strip_prefix('"') {
        let end = rest.find('"').unwrap_or(rest.len());
        let exe = &rest[..end];
        let args = rest.get(end + 1..).unwrap_or_default().trim();
        return (!exe.is_empty()).then(|| Launch::new(exe, args));
    }
    let lower = c.to_ascii_lowercase();
    let mut from = 0;
    while let Some(at) = lower[from..].find(".exe") {
        let end = from + at + 4;
        if lower[end..].chars().next().is_none_or(char::is_whitespace) {
            return Some(Launch::new(&c[..end], c[end..].trim()));
        }
        from = end;
    }
    let (exe, args) = c.split_once(char::is_whitespace).unwrap_or((c, ""));
    (!exe.is_empty()).then(|| Launch::new(exe, args.trim()))
}

/// Many programs register `MsiExec.exe /I{product}` as their uninstaller (it opens the installer's
/// change page); Windows turns that into a removal, and so does this. Anything else is untouched.
pub fn msi_install_to_remove(l: Launch) -> Launch {
    if !l.is_msiexec() {
        return l;
    }
    let args = l
        .args
        .split(' ')
        .map(|tok| match tok.as_bytes() {
            [b'/' | b'-', b'i' | b'I', ..] if tok[2..].is_empty() || tok[2..].starts_with('{') => format!("{}x{}", &tok[..1], &tok[2..]),
            _ => tok.to_string(),
        })
        .collect::<Vec<_>>()
        .join(" ");
    Launch { exe: l.exe, args }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> (String, String) {
        let l = parse(s).unwrap();
        (l.exe, l.args)
    }

    #[test]
    fn a_quoted_path_ends_at_its_quote() {
        assert_eq!(p(r#""C:\Program Files\App\unins000.exe" /SILENT"#), (r"C:\Program Files\App\unins000.exe".into(), "/SILENT".into()));
        assert_eq!(p(r#""C:\a b\u.exe""#), (r"C:\a b\u.exe".into(), String::new()));
        assert_eq!(p(r#""C:\a\u.exe"/x"#), (r"C:\a\u.exe".into(), "/x".into()));
    }

    #[test]
    fn an_unquoted_path_with_spaces_ends_at_exe() {
        assert_eq!(p(r"C:\Program Files\My App\uninst.exe -u /q"), (r"C:\Program Files\My App\uninst.exe".into(), "-u /q".into()));
        assert_eq!(p(r"C:\Program Files\My App\uninst.EXE"), (r"C:\Program Files\My App\uninst.EXE".into(), String::new()));
        // ".exe" inside a longer word is not the end.
        assert_eq!(p(r"C:\x\a.exe-dir\u.exe /y"), (r"C:\x\a.exe-dir\u.exe".into(), "/y".into()));
    }

    #[test]
    fn no_exe_means_the_first_word() {
        assert_eq!(p(r"rundll32.exe dfshim.dll,ShArpMaintain app.appref-ms"), ("rundll32.exe".into(), "dfshim.dll,ShArpMaintain app.appref-ms".into()));
        assert_eq!(p(r#"rundll32 "C:\Program Files\X\x.dll",Uninstall"#), ("rundll32".into(), r#""C:\Program Files\X\x.dll",Uninstall"#.into()));
        assert_eq!(p("  setup  "), ("setup".into(), String::new()));
    }

    #[test]
    fn nothing_to_parse() {
        assert_eq!(parse(""), None);
        assert_eq!(parse("   "), None);
        assert_eq!(parse("\"\""), None);
    }

    fn fixed(s: &str) -> String {
        msi_install_to_remove(parse(s).unwrap()).command_line()
    }

    #[test]
    fn msiexec_install_becomes_remove() {
        assert_eq!(fixed("MsiExec.exe /I{ABC-1}"), r#""MsiExec.exe" /x{ABC-1}"#);
        assert_eq!(fixed("msiexec /i {ABC-1} /qb"), r#""msiexec" /x {ABC-1} /qb"#);
        assert_eq!(fixed(r"C:\Windows\System32\msiexec.exe -i{ABC-1}"), r#""C:\Windows\System32\msiexec.exe" -x{ABC-1}"#);
    }

    #[test]
    fn other_command_lines_are_left_alone() {
        assert_eq!(fixed("MsiExec.exe /X{ABC-1}"), r#""MsiExec.exe" /X{ABC-1}"#);
        assert_eq!(fixed(r"C:\a\setup.exe /Install"), r#""C:\a\setup.exe" /Install"#);
        assert_eq!(fixed("msiexec /Installer"), r#""msiexec" /Installer"#);
    }

    #[test]
    fn the_command_line_quotes_the_program() {
        assert_eq!(Launch::new(r"C:\a b\x.exe", "").command_line(), r#""C:\a b\x.exe""#);
        assert_eq!(msi_uninstall("{G}").command_line(), r#""msiexec.exe" /x {G}"#);
        assert_eq!(msi_modify("{G}").command_line(), r#""msiexec.exe" /i {G}"#);
    }
}
