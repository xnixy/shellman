//! What the emulator does not keep but the window wants, read from the shell's output on its way
//! to the parser: the working directory a shell reports (OSC 7 `file://host/path`, or ConEmu's
//! OSC 9;9 `path` that Windows Terminal also reads), and X10 mouse mode (DECSET 9), which
//! `alacritty_terminal` does not track.
//!
//! A small state machine over the bytes, so a sequence split between two reads is still found.
//! Most output is plain text and skipped a run at a time up to the next ESC.

/// Something found in the output.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Found {
    /// The shell's working directory: a Windows path (`C:\...`) or, from WSL, a POSIX one.
    Cwd(String),
    /// DECSET / DECRST 9.
    X10(bool),
    /// DECSET / DECRST 9001: win32-input-mode, which ConPTY asks for at its start.
    Win32Input(bool),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum State {
    Ground,
    Esc,
    Osc,
    /// ESC inside an OSC: a string terminator if `\` follows.
    OscEsc,
    Csi,
}

/// Longest OSC or CSI kept; longer ones are not what is looked for and are skipped.
const MAX: usize = 4096;

pub struct Scanner {
    state: State,
    buf: Vec<u8>,
    /// The sequence grew past `MAX`: skip to its end.
    over: bool,
}

impl Default for Scanner {
    fn default() -> Self {
        Self { state: State::Ground, buf: Vec::with_capacity(256), over: false }
    }
}

impl Scanner {
    /// Read `bytes`, calling `found` for each sequence of interest.
    pub fn feed(&mut self, bytes: &[u8], mut found: impl FnMut(Found)) {
        let mut i = 0;
        while i < bytes.len() {
            if self.state == State::Ground {
                match bytes[i..].iter().position(|&b| b == 0x1b) {
                    Some(k) => {
                        i += k + 1;
                        self.state = State::Esc;
                    }
                    None => return,
                }
                continue;
            }
            let b = bytes[i];
            i += 1;
            match self.state {
                State::Ground => unreachable!(),
                State::Esc => {
                    self.buf.clear();
                    self.over = false;
                    self.state = match b {
                        b']' => State::Osc,
                        b'[' => State::Csi,
                        0x1b => State::Esc,
                        _ => State::Ground,
                    };
                }
                State::Osc => match b {
                    0x07 => {
                        self.end_osc(&mut found);
                        self.state = State::Ground;
                    }
                    0x1b => self.state = State::OscEsc,
                    // CAN and SUB abort a sequence.
                    0x18 | 0x1a => self.state = State::Ground,
                    _ => self.push(b),
                },
                State::OscEsc => {
                    if b == b'\\' {
                        self.end_osc(&mut found);
                        self.state = State::Ground;
                    } else {
                        // A new sequence began: this one was cut off.
                        self.state = State::Esc;
                        i -= 1;
                    }
                }
                State::Csi => match b {
                    0x40..=0x7e => {
                        if !self.over && (b == b'h' || b == b'l') {
                            if let Some(params) = self.buf.strip_prefix(b"?") {
                                if params.split(|&c| c == b';').any(|p| p == b"9") {
                                    found(Found::X10(b == b'h'));
                                }
                                if params.split(|&c| c == b';').any(|p| p == b"9001") {
                                    found(Found::Win32Input(b == b'h'));
                                }
                            }
                        }
                        self.state = State::Ground;
                    }
                    0x1b => self.state = State::Esc,
                    0x18 | 0x1a => self.state = State::Ground,
                    _ => self.push(b),
                },
            }
        }
    }

    fn push(&mut self, b: u8) {
        if self.buf.len() < MAX {
            self.buf.push(b);
        } else {
            self.over = true;
        }
    }

    fn end_osc(&mut self, found: &mut impl FnMut(Found)) {
        if self.over {
            return;
        }
        let Ok(text) = std::str::from_utf8(&self.buf) else { return };
        let cwd = if let Some(uri) = text.strip_prefix("7;") {
            cwd_from_uri(uri)
        } else if let Some(path) = text.strip_prefix("9;9;") {
            let path = path.trim().trim_matches('"');
            (!path.is_empty()).then(|| path.to_owned())
        } else {
            None
        };
        if let Some(cwd) = cwd.filter(|c| reported_cwd_ok(c)) {
            found(Found::Cwd(cwd));
        }
    }
}

/// The path of a `file://host/path` URI: percent-decoded, and `/C:/x` made `C:\x`.
pub fn cwd_from_uri(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("file://")?;
    // The host, up to the path's first slash (empty for `file:///`).
    let path = &rest[rest.find('/')?..];
    let path = percent_decode(path)?;
    let b = path.as_bytes();
    if b.len() >= 3 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b':' {
        let mut win = path[1..].replace('/', "\\");
        if win.len() == 2 {
            win.push('\\');
        }
        return Some(win);
    }
    Some(path)
}

fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = std::str::from_utf8(&b[i + 1..i + 3]).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Whether a directory a program reported may be kept at all: an absolute path of a kind a new
/// tab can start in (`C:\...`, or `/...` from WSL), with no control characters and no `"`.
/// Output is anyone's (a file shown with `cat`), so what it reports is checked before it is
/// stored; it reaches a command line only through `quote_arg` anyway.
pub fn reported_cwd_ok(p: &str) -> bool {
    if p.chars().any(|c| c.is_control() || c == '"') {
        return false;
    }
    let b = p.as_bytes();
    let drive = b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\';
    let posix = b.first() == Some(&b'/');
    drive || posix
}

/// Whether a reported directory is a Windows path (one a Windows shell can start in).
pub fn is_windows_path(p: &str) -> bool {
    let b = p.as_bytes();
    (b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':') || p.starts_with("\\\\")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(chunks: &[&[u8]]) -> Vec<Found> {
        let mut s = Scanner::default();
        let mut out = Vec::new();
        for c in chunks {
            s.feed(c, |f| out.push(f));
        }
        out
    }

    #[test]
    fn osc_7_gives_the_directory() {
        assert_eq!(scan(&[b"x\x1b]7;file://pc/C:/Users/me/My%20Stuff\x07y"]), [Found::Cwd(r"C:\Users\me\My Stuff".into())]);
        assert_eq!(scan(&[b"\x1b]7;file://debian/home/me\x1b\\"]), [Found::Cwd("/home/me".into())]);
        assert_eq!(scan(&[b"\x1b]7;file:///D:/\x07"]), [Found::Cwd(r"D:\".into())]);
    }

    #[test]
    fn osc_9_9_gives_the_directory() {
        assert_eq!(scan(&[b"\x1b]9;9;\"C:\\Windows\"\x1b\\"]), [Found::Cwd(r"C:\Windows".into())]);
        assert_eq!(scan(&[b"\x1b]9;9;C:\\x\x07"]), [Found::Cwd(r"C:\x".into())]);
        // Other OSC 9 (notifications) are not a directory.
        assert_eq!(scan(&[b"\x1b]9;hello\x07"]), []);
    }

    #[test]
    fn a_sequence_split_between_reads_is_found() {
        assert_eq!(scan(&[b"ab\x1b", b"]7;file://h", b"/tmp\x1b", b"\\"]), [Found::Cwd("/tmp".into())]);
    }

    #[test]
    fn titles_and_colours_are_not_directories() {
        assert_eq!(scan(&[b"\x1b]0;title\x07\x1b[31mred\x1b[0m\x1b]2;x\x1b\\"]), []);
    }

    #[test]
    fn decset_9_is_x10_mouse() {
        assert_eq!(scan(&[b"\x1b[?9h"]), [Found::X10(true)]);
        assert_eq!(scan(&[b"\x1b[?1000;9l"]), [Found::X10(false)]);
        assert_eq!(scan(&[b"\x1b[?1049h\x1b[9h\x1b[?19h"]), []);
    }

    #[test]
    fn decset_9001_is_win32_input_mode() {
        // ConPTY's opening request, split across reads, beside focus events.
        assert_eq!(scan(&[b"\x1b[?9001h\x1b[?1004h"]), [Found::Win32Input(true)]);
        assert_eq!(scan(&[b"\x1b[?90", b"01;1004l"]), [Found::Win32Input(false)]);
        assert_eq!(scan(&[b"\x1b[?900h\x1b[9001h"]), []);
    }

    #[test]
    fn an_unterminated_sequence_cut_by_another_is_dropped() {
        assert_eq!(scan(&[b"\x1b]7;file://h/a\x1b[0m\x1b]7;file://h/b\x07"]), [Found::Cwd("/b".into())]);
    }

    #[test]
    fn hostile_directories_are_not_kept() {
        // A quote, control characters (here percent-encoded), relative paths.
        for seq in [
            &b"\x1b]7;file://h/home/x%22%20;calc%20%22\x07"[..],
            b"\x1b]7;file://h/home/x%0Acalc\x07",
            b"\x1b]7;file://h/home/x%1B[201~\x07",
            b"\x1b]7;file://h/C:/x%22y\x07",
            b"\x1b]9;9;C:\\x\"y\x07",
            b"\x1b]9;9;relative\\dir\x07",
            b"\x1b]9;9;C:relative\x07",
            b"\x1b]9;9;C:\\a\tb\x07",
        ] {
            assert_eq!(scan(&[seq]), [], "{}", String::from_utf8_lossy(seq));
        }
        assert!(reported_cwd_ok(r"C:\Users\me"));
        assert!(reported_cwd_ok("/home/me/my dir"));
        assert!(!reported_cwd_ok("~/x"));
    }

    #[test]
    fn windows_paths() {
        assert!(is_windows_path(r"C:\x"));
        assert!(is_windows_path(r"\\server\share"));
        assert!(!is_windows_path("/home/me"));
    }
}
