//! Small pure pieces: the install record, and what the registry values mean.

use std::collections::BTreeSet;

pub const RECORD: &str = "shellman-install.txt";

/// shellman Terminal's handoff class (`apps/terminal/src/defterm`).
pub const TERMINAL_CLSID: &str = "{39C0509F-90BF-4A6C-A66D-5CF96BDDC7E2}";

/// The install record: the files setup put in the install folder (relative names), so uninstall
/// removes exactly those.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Record {
    pub stamp: String,
    pub files: BTreeSet<String>,
}

impl Record {
    pub fn parse(text: &str) -> Record {
        let mut r = Record::default();
        for line in text.lines().map(str::trim) {
            if let Some(v) = line.strip_prefix("stamp=") {
                r.stamp = v.to_string();
            } else if let Some(v) = line.strip_prefix("file=") {
                if !v.is_empty() {
                    r.files.insert(v.to_string());
                }
            }
        }
        r
    }

    pub fn render(&self) -> String {
        let mut s = format!("# written by shellman-setup; uninstall removes exactly these files\nstamp={}\n", self.stamp);
        for f in &self.files {
            s.push_str(&format!("file={f}\n"));
        }
        s
    }
}

/// Is a Winlogon `Shell` value shellman's session?
pub fn shell_is_shellman(value: &str) -> bool {
    value.trim().trim_matches('"').to_ascii_lowercase().ends_with("shellman-session.exe")
}

/// Words for the default terminal state, from `HKCU\Console\%%Startup` `DelegationTerminal`.
pub fn terminal_state(value: Option<&str>) -> String {
    match value.map(str::trim) {
        None | Some("") => "Windows decides (no DelegationTerminal set)".into(),
        Some(v) if v.eq_ignore_ascii_case(TERMINAL_CLSID) => "shellman Terminal is the default terminal".into(),
        Some("{00000000-0000-0000-0000-000000000000}") => "Windows decides".into(),
        Some(v) => format!("another terminal is the default ({v})"),
    }
}

pub fn terminal_is_shellman(value: Option<&str>) -> bool {
    value.is_some_and(|v| v.trim().eq_ignore_ascii_case(TERMINAL_CLSID))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_record_round_trips() {
        let mut r = Record { stamp: "0.1.0+abc 2026-11-12".into(), ..Default::default() };
        r.files.insert("shellman-bar.exe".into());
        r.files.insert("sub\\conpty.dll".into());
        assert_eq!(Record::parse(&r.render()), r);
        assert_eq!(Record::parse("garbage\nfile=\n"), Record::default());
    }

    #[test]
    fn recognises_the_shell_value() {
        assert!(shell_is_shellman(r"C:\Users\u\AppData\Local\Programs\shellman\shellman-session.exe"));
        assert!(shell_is_shellman(r#""C:\Program Files\x\Shellman-Session.EXE""#));
        assert!(!shell_is_shellman("explorer.exe"));
        assert!(!shell_is_shellman(""));
    }

    #[test]
    fn terminal_words() {
        assert!(terminal_state(None).starts_with("Windows decides"));
        assert!(terminal_state(Some("{39c0509f-90bf-4a6c-a66d-5cf96bddc7e2}")).contains("shellman Terminal"));
        assert!(terminal_state(Some("{E12CFF52-A866-4C77-9A90-F570A7AA2C6B}")).contains("another"));
        assert!(terminal_is_shellman(Some(TERMINAL_CLSID)));
        assert!(!terminal_is_shellman(None));
    }
}
