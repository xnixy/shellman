//! The clipboard's text, for a right-click paste (egui only hands over the clipboard on its own
//! paste keys), and following a link a program printed (Ctrl+click).

use std::path::PathBuf;
use windows::core::HSTRING;
use windows::Win32::Foundation::{HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{CloseClipboard, GetClipboardData, OpenClipboard};
use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
use windows::Win32::System::Ole::CF_UNICODETEXT;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

/// The clipboard's text, or None when it holds none (or another program has it open).
pub fn text() -> Option<String> {
    unsafe {
        // Another program may hold it a moment: try a few times.
        let mut open = false;
        for _ in 0..5 {
            if OpenClipboard(None).is_ok() {
                open = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        if !open {
            return None;
        }
        let text = (|| {
            let h: HANDLE = GetClipboardData(CF_UNICODETEXT.0 as u32).ok()?;
            let g = HGLOBAL(h.0);
            let p = GlobalLock(g) as *const u16;
            if p.is_null() {
                return None;
            }
            let max = GlobalSize(g) / 2;
            let s = std::slice::from_raw_parts(p, max);
            let len = s.iter().position(|&c| c == 0).unwrap_or(max);
            let t = String::from_utf16_lossy(&s[..len]);
            let _ = GlobalUnlock(g);
            Some(t)
        })();
        let _ = CloseClipboard();
        text
    }
}

/// What a link a program printed (OSC 8, or a URL in the text) leads to, when it is one the
/// terminal follows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Link {
    /// An `http`, `https` or `mailto` link: Windows opens it with its handler (the browser, the
    /// mail app).
    Web(String),
    /// A `file:` link to a path on a local drive: never opened (run), only shown in Files.
    File(PathBuf),
}

/// Longest link followed.
const MAX_URI: usize = 4096;

/// What `uri` leads to, or None for a link the terminal does not follow: any other scheme, any
/// control character, blank or `"` in it, a malformed `%` escape, a web link without a host, or
/// a `file:` link that is not a drive-letter path on this computer (a host other than
/// `localhost`, UNC, `\\?\`, a network drive). Percent-decoded once.
pub fn parse_link(uri: &str) -> Option<Link> {
    if uri.is_empty() || uri.len() > MAX_URI || uri.chars().any(|c| c.is_control() || c.is_whitespace() || c == '"') {
        return None;
    }
    let (scheme, rest) = uri.split_once(':')?;
    if scheme.is_empty() || !scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')) {
        return None;
    }
    match scheme.to_ascii_lowercase().as_str() {
        "http" | "https" => {
            let after = rest.strip_prefix("//")?;
            let host = &after[..after.find(['/', '?', '#']).unwrap_or(after.len())];
            // The host (and port), without a user name: `https://good.com@evil.com` is refused.
            let ok = !host.is_empty() && !host.contains('@') && !host.contains('\\');
            ok.then(|| Link::Web(uri.to_owned()))
        }
        "mailto" => (!rest.is_empty()).then(|| Link::Web(uri.to_owned())),
        "file" => file_path(rest).map(Link::File),
        _ => None,
    }
}

/// The local path of a `file:` link's part after the scheme: `///C:/x`, `//localhost/C:/x` or
/// `/C:/x`, its query and fragment dropped.
fn file_path(rest: &str) -> Option<PathBuf> {
    let rest = rest.split(['?', '#']).next()?;
    let path = if let Some(after) = rest.strip_prefix("//") {
        let slash = after.find('/')?;
        let host = &after[..slash];
        if !(host.is_empty() || host.eq_ignore_ascii_case("localhost")) {
            return None;
        }
        &after[slash..]
    } else if rest.starts_with('/') {
        rest
    } else {
        return None;
    };
    let decoded = percent_decode(path)?;
    let mut win = decoded.strip_prefix('/')?.replace('/', "\\");
    if win.len() == 2 {
        win.push('\\');
    }
    crate::local::drive_path(&win)
}

/// `%xx` escapes decoded, once; None for a malformed one or bytes that are not UTF-8.
fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// What the hover label over a link says: where it goes (cut in the middle when very long, its
/// start, with the host, always whole), and what Ctrl+click does with it.
pub fn describe(uri: &str) -> String {
    const HEAD: usize = 120;
    const TAIL: usize = 40;
    let chars: Vec<char> = uri.chars().filter(|c| !c.is_control()).collect();
    let shown: String = if chars.len() > HEAD + TAIL + 1 {
        chars[..HEAD].iter().chain(['…'].iter()).chain(chars[chars.len() - TAIL..].iter()).collect()
    } else {
        chars.into_iter().collect()
    };
    let action = match parse_link(uri) {
        Some(Link::Web(_)) => "Ctrl+click to open".to_owned(),
        Some(Link::File(p)) => format!("Ctrl+click to show {} in Files", p.display()),
        None => "Not opened: only web, mail and local file links are".to_owned(),
    };
    format!("{shown}\n{action}")
}

/// Follow `uri` (Ctrl+click on a link): a web or mail link opens with its handler; a local
/// file's folder opens in shellman Files (a folder itself opens there); anything else is
/// ignored. Off the UI thread, as the handler or the drive may take a moment.
pub fn open(uri: &str) {
    let Some(link) = parse_link(uri) else { return };
    std::thread::spawn(move || match link {
        Link::Web(uri) => unsafe {
            ShellExecuteW(None, &HSTRING::from("open"), &HSTRING::from(uri.as_str()), None, None, SW_SHOWNORMAL);
        },
        Link::File(path) => {
            if !crate::local::on_local_drive(&path) {
                return;
            }
            let shown = path.display().to_string();
            let started = if path.is_dir() {
                shellman_core::sibling::start_with("files", &[&shown])
            } else {
                shellman_core::sibling::start_with("files", &["--select", &shown])
            };
            if !started {
                eprintln!("terminal: could not start Files for {shown}");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_and_mail_links_open() {
        for uri in ["https://example.com/a?b=c#d", "http://example.com", "HTTPS://Example.com:8080/x", "mailto:me@example.com"] {
            assert_eq!(parse_link(uri), Some(Link::Web(uri.into())), "{uri}");
        }
    }

    #[test]
    fn other_schemes_and_odd_links_do_not() {
        for uri in [
            "file://attacker.invalid/share/x.exe",
            "file:////attacker.invalid/share/x.exe",
            "file:///%5C%5Cattacker.invalid%5Cshare%5Cx.exe",
            "file://localhost//attacker.invalid/share",
            "file:///%5C%5C%3F%5CC:%5Cx.exe",
            "file:x.exe",
            "file:///Z",
            "ftp://example.com/x",
            "javascript:alert(1)",
            "ms-settings:privacy",
            "search-ms:query=x",
            "C:\\Windows\\calc.exe",
            "https://",
            "https://good.example@evil.example/",
            "https://example.com/a\"b",
            "https://example.com/a b",
            "https://example.com/\u{1b}[201~",
            "https://example.com/\u{9b}",
            "file:///C:/x%0A.exe",
            "file:///C:/x%ZZ",
            "file:///C:/x.exe:stream",
            "",
        ] {
            assert_eq!(parse_link(uri), None, "{uri}");
        }
    }

    #[test]
    fn local_file_links_are_paths_to_show() {
        assert_eq!(parse_link("file:///C:/Users/me/My%20File.txt"), Some(Link::File(PathBuf::from(r"C:\Users\me\My File.txt"))));
        assert_eq!(parse_link("file://localhost/D:/x?y#z"), Some(Link::File(PathBuf::from(r"D:\x"))));
        assert_eq!(parse_link("file:/C:/"), Some(Link::File(PathBuf::from(r"C:\"))));
        // Decoded once only: %2541 is "%41", not "A".
        assert_eq!(parse_link("file:///C:/a%2541"), Some(Link::File(PathBuf::from(r"C:\a%41"))));
    }

    #[test]
    fn the_label_says_where_a_link_goes() {
        let d = describe("file://attacker.invalid/share/x.exe");
        assert!(d.starts_with("file://attacker.invalid/share/x.exe\nNot opened"), "{d}");
        assert!(describe("https://example.com").ends_with("Ctrl+click to open"));
        assert!(describe("file:///C:/x/y.exe").contains(r"show C:\x\y.exe in Files"));
        let long = format!("https://example.com/{}", "a".repeat(500));
        let d = describe(&long);
        assert!(d.starts_with("https://example.com/") && d.contains('…') && d.len() < 300);
    }
}