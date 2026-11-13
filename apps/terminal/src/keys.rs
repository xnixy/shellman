//! What the shell is sent for a key: the bytes xterm sends, which is what ConPTY, PowerShell,
//! cmd, WSL and full-screen programs expect. No Win32 and no egui here, so it is tested as it is.

/// The modifier keys held with a key.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Mods {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

impl Mods {
    pub const NONE: Mods = Mods { shift: false, ctrl: false, alt: false };

    fn any(self) -> bool {
        self.shift || self.ctrl || self.alt
    }

    /// xterm's modifier parameter: 1 plus shift 1, alt 2, ctrl 4.
    fn param(self) -> u8 {
        1 + self.shift as u8 + 2 * self.alt as u8 + 4 * self.ctrl as u8
    }
}

/// A key that is not plain text. Printable keys come as text; `Char` is a printable key pressed
/// with Ctrl, which sends no text.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Enter,
    Backspace,
    Tab,
    Escape,
    Space,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    /// F1 to F12.
    F(u8),
    Char(char),
}

/// What the terminal does with a key itself instead of sending it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Local {
    /// Scroll the history a page up (positive) or down.
    ScrollPage(i32),

}

/// Keys the terminal keeps: Shift+PgUp/PgDn page through the history. (Ctrl+V, Ctrl+Shift+V
/// and Shift+Insert reach the app as a paste, not as keys.)
pub fn local(key: Key, mods: Mods) -> Option<Local> {
    match (key, mods) {
        (Key::PageUp, Mods { shift: true, ctrl: false, alt: false }) => Some(Local::ScrollPage(1)),
        (Key::PageDown, Mods { shift: true, ctrl: false, alt: false }) => Some(Local::ScrollPage(-1)),

        _ => None,
    }
}

/// The bytes for `key`. `app_cursor` is the application cursor-key mode (DECCKM) that `vim` and
/// `less` turn on: arrows, Home and End then send SS3 instead of CSI. None for a key that sends
/// nothing (a Ctrl+Alt letter is left to the text it makes, since AltGr is Ctrl+Alt).
pub fn key_bytes(key: Key, mods: Mods, app_cursor: bool) -> Option<Vec<u8>> {
    let esc = |rest: &[u8]| {
        let mut v = vec![0x1b];
        v.extend_from_slice(rest);
        v
    };
    // Alt sends ESC first, for the keys that have no modifier parameter.
    let alt = |bytes: Vec<u8>| if mods.alt { esc(&bytes) } else { bytes };
    let bytes = match key {
        Key::Enter => alt(vec![b'\r']),
        Key::Escape => alt(vec![0x1b]),
        Key::Backspace => alt(vec![if mods.ctrl { 0x08 } else { 0x7f }]),
        Key::Tab if mods.shift => esc(b"[Z"),
        Key::Tab => alt(vec![b'\t']),
        Key::Space if mods.ctrl => alt(vec![0]),
        Key::Space => alt(vec![b' ']),
        Key::Up => cursor(b'A', mods, app_cursor),
        Key::Down => cursor(b'B', mods, app_cursor),
        Key::Right => cursor(b'C', mods, app_cursor),
        Key::Left => cursor(b'D', mods, app_cursor),
        Key::Home => cursor(b'H', mods, app_cursor),
        Key::End => cursor(b'F', mods, app_cursor),
        Key::Insert => tilde(2, mods),
        Key::Delete => tilde(3, mods),
        Key::PageUp => tilde(5, mods),
        Key::PageDown => tilde(6, mods),
        Key::F(n @ 1..=4) => {
            let last = b'P' + (n - 1);
            if mods.any() {
                format!("\x1b[1;{}{}", mods.param(), last as char).into_bytes()
            } else {
                esc(&[b'O', last])
            }
        }
        Key::F(n @ 5..=12) => {
            const CODES: [u8; 8] = [15, 17, 18, 19, 20, 21, 23, 24];
            tilde(CODES[(n - 5) as usize], mods)
        }
        Key::F(_) => return None,
        Key::Char(c) => {
            if !mods.ctrl || mods.alt {
                return None;
            }
            alt(vec![ctrl_code(c)?])
        }
    };
    Some(bytes)
}

/// Arrows, Home and End: SS3 in application mode, CSI otherwise, CSI 1;m with modifiers.
fn cursor(last: u8, mods: Mods, app_cursor: bool) -> Vec<u8> {
    if mods.any() {
        format!("\x1b[1;{}{}", mods.param(), last as char).into_bytes()
    } else if app_cursor {
        vec![0x1b, b'O', last]
    } else {
        vec![0x1b, b'[', last]
    }
}

/// The `CSI n ~` keys, with `;m` for modifiers.
fn tilde(n: u8, mods: Mods) -> Vec<u8> {
    if mods.any() {
        format!("\x1b[{n};{}~", mods.param()).into_bytes()
    } else {
        format!("\x1b[{n}~").into_bytes()
    }
}

/// The control code Ctrl and a key make: letters to 1..26, and the punctuation xterm maps.
fn ctrl_code(c: char) -> Option<u8> {
    let c = c.to_ascii_lowercase();
    Some(match c {
        'a'..='z' => c as u8 - b'a' + 1,
        '@' | '2' | ' ' => 0,
        '[' | '3' => 0x1b,
        '\\' | '4' => 0x1c,
        ']' | '5' => 0x1d,
        '^' | '6' => 0x1e,
        '_' | '-' | '/' | '7' => 0x1f,
        '8' | '?' => 0x7f,
        _ => return None,
    })
}

/// Typed text, with ESC before it when Alt was held (Alt+b is "back a word" in readline).
pub fn text_bytes(text: &str, alt: bool) -> Vec<u8> {
    let mut v = Vec::with_capacity(text.len() + 1);
    if alt {
        v.push(0x1b);
    }
    v.extend_from_slice(text.as_bytes());
    v
}

/// Pasted text: line ends become CR (what Enter sends), and with bracketed paste on it is wrapped
/// so the shell takes it as one paste, with any end marker inside it taken out.
pub fn paste_bytes(text: &str, bracketed: bool) -> Vec<u8> {
    let text = text.replace("\r\n", "\r").replace('\n', "\r");
    if bracketed {
        let inner = text.replace("\x1b[201~", "");
        format!("\x1b[200~{inner}\x1b[201~").into_bytes()
    } else {
        text.into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CTRL: Mods = Mods { shift: false, ctrl: true, alt: false };
    const SHIFT: Mods = Mods { shift: true, ctrl: false, alt: false };
    const ALT: Mods = Mods { shift: false, ctrl: false, alt: true };

    fn bytes(key: Key, mods: Mods) -> Vec<u8> {
        key_bytes(key, mods, false).unwrap()
    }

    #[test]
    fn plain_keys() {
        assert_eq!(bytes(Key::Enter, Mods::NONE), b"\r");
        assert_eq!(bytes(Key::Backspace, Mods::NONE), b"\x7f");
        assert_eq!(bytes(Key::Tab, Mods::NONE), b"\t");
        assert_eq!(bytes(Key::Escape, Mods::NONE), b"\x1b");
        assert_eq!(bytes(Key::Tab, SHIFT), b"\x1b[Z");
        assert_eq!(bytes(Key::Backspace, CTRL), b"\x08");
    }

    #[test]
    fn arrows_follow_application_cursor_mode() {
        assert_eq!(bytes(Key::Up, Mods::NONE), b"\x1b[A");
        assert_eq!(key_bytes(Key::Up, Mods::NONE, true).unwrap(), b"\x1bOA");
        assert_eq!(key_bytes(Key::Left, Mods::NONE, true).unwrap(), b"\x1bOD");
        assert_eq!(key_bytes(Key::Home, Mods::NONE, true).unwrap(), b"\x1bOH");
        assert_eq!(bytes(Key::End, Mods::NONE), b"\x1b[F");
        // With a modifier it is always CSI 1;m, in either mode.
        assert_eq!(key_bytes(Key::Right, CTRL, true).unwrap(), b"\x1b[1;5C");
        assert_eq!(bytes(Key::Up, SHIFT), b"\x1b[1;2A");
        assert_eq!(bytes(Key::Down, ALT), b"\x1b[1;3B");
    }

    #[test]
    fn tilde_keys_and_function_keys() {
        assert_eq!(bytes(Key::PageUp, Mods::NONE), b"\x1b[5~");
        assert_eq!(bytes(Key::PageDown, Mods::NONE), b"\x1b[6~");
        assert_eq!(bytes(Key::Delete, Mods::NONE), b"\x1b[3~");
        assert_eq!(bytes(Key::Insert, Mods::NONE), b"\x1b[2~");
        assert_eq!(bytes(Key::Delete, CTRL), b"\x1b[3;5~");
        assert_eq!(bytes(Key::F(1), Mods::NONE), b"\x1bOP");
        assert_eq!(bytes(Key::F(4), Mods::NONE), b"\x1bOS");
        assert_eq!(bytes(Key::F(1), SHIFT), b"\x1b[1;2P");
        assert_eq!(bytes(Key::F(5), Mods::NONE), b"\x1b[15~");
        assert_eq!(bytes(Key::F(10), Mods::NONE), b"\x1b[21~");
        assert_eq!(bytes(Key::F(12), CTRL), b"\x1b[24;5~");
        assert_eq!(key_bytes(Key::F(13), Mods::NONE, false), None);
    }

    #[test]
    fn ctrl_letters_and_punctuation() {
        assert_eq!(bytes(Key::Char('c'), CTRL), [3]);
        assert_eq!(bytes(Key::Char('A'), CTRL), [1]);
        assert_eq!(bytes(Key::Char('z'), CTRL), [26]);
        assert_eq!(bytes(Key::Char('['), CTRL), [0x1b]);
        assert_eq!(bytes(Key::Char('\\'), CTRL), [0x1c]);
        assert_eq!(bytes(Key::Space, CTRL), [0]);
        // Ctrl+Alt is AltGr on many layouts: the character it types goes as text instead.
        let ctrl_alt = Mods { ctrl: true, alt: true, shift: false };
        assert_eq!(key_bytes(Key::Char('q'), ctrl_alt, false), None);
        // A character key without Ctrl sends its text, not this.
        assert_eq!(key_bytes(Key::Char('q'), Mods::NONE, false), None);
    }

    #[test]
    fn alt_puts_escape_first() {
        assert_eq!(bytes(Key::Enter, ALT), b"\x1b\r");
        assert_eq!(bytes(Key::Backspace, ALT), b"\x1b\x7f");
        assert_eq!(text_bytes("b", true), b"\x1bb");
        assert_eq!(text_bytes("é", false), "é".as_bytes());
    }

    #[test]
    fn paste_sends_cr_and_brackets_when_asked() {
        assert_eq!(paste_bytes("a\nb\r\nc", false), b"a\rb\rc");
        assert_eq!(paste_bytes("ls\n", true), b"\x1b[200~ls\r\x1b[201~");
        // A pasted end marker cannot end the paste early.
        assert_eq!(paste_bytes("x\x1b[201~y", true), b"\x1b[200~xy\x1b[201~");
    }

    #[test]
    fn local_keys() {
        assert_eq!(local(Key::PageUp, SHIFT), Some(Local::ScrollPage(1)));
        assert_eq!(local(Key::PageDown, SHIFT), Some(Local::ScrollPage(-1)));
        assert_eq!(local(Key::PageUp, Mods::NONE), None);
        assert_eq!(local(Key::Char('v'), CTRL), None);
    }
}
