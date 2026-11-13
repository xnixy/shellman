//! Starting an app, or running whatever the user typed, through the shell.

use super::App;
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

/// Start an app the way the Start menu does.
pub fn launch(app: &App) -> bool {
    let path = app.shell_path();
    // Without Explorer the shell refuses Store apps; the activation manager does not.
    execute(&path, None) || super::activate::retry(&path)
}

/// ShellExecute the text as typed: a path, a URL, a program name, or `program arguments`.
pub fn run_text(text: &str) -> bool {
    let text = text.trim();
    if text.is_empty() {
        return false;
    }
    // A folder opens in Files; ShellExecute would hand it to Explorer.
    if std::path::Path::new(text.trim_matches('"')).is_dir()
        && crate::sibling::start_with("files", &[text.trim_matches('"')])
    {
        return true;
    }
    if execute(text, None) {
        return true;
    }
    match split_command(text) {
        Some((file, args)) => execute(file, Some(args)),
        None => false,
    }
}

/// "program rest of the line", or a quoted program then the rest.
fn split_command(text: &str) -> Option<(&str, &str)> {
    if let Some(rest) = text.strip_prefix('"') {
        let end = rest.find('"')?;
        return Some((&rest[..end], rest[end + 1..].trim()));
    }
    let (file, args) = text.split_once(char::is_whitespace)?;
    Some((file, args.trim()))
}

/// ShellExecute reports success with a value above 32.
fn execute(file: &str, args: Option<&str>) -> bool {
    let args = args.map(HSTRING::from);
    let params = args.as_ref().map_or(PCWSTR::null(), |a| PCWSTR(a.as_ptr()));
    let result = unsafe {
        ShellExecuteW(
            None,
            &HSTRING::from("open"),
            &HSTRING::from(file),
            params,
            None,
            SW_SHOWNORMAL,
        )
    };
    result.0 as usize > 32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_program_and_arguments() {
        assert_eq!(split_command("cmd /c dir"), Some(("cmd", "/c dir")));
        assert_eq!(
            split_command(r#""C:\Program Files\x.exe" -a b"#),
            Some((r"C:\Program Files\x.exe", "-a b"))
        );
        assert_eq!(split_command("notepad"), None);
    }
}
