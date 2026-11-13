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

/// One key event as Windows' console sees it (`KEY_EVENT_RECORD`), sent whole while the console
/// asked for win32-input-mode (`CSI ? 9001 h`, which ConPTY does at its start): console programs
/// then get real key events (PSReadLine's Shift+arrow selection and its own Ctrl+C copy, any
/// modifier combination), not the VT sequences above. See microsoft/terminal
/// `doc/specs/#4999 - Improved keyboard handling in Conpty.md`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Win32Key {
    pub vk: u16,
    pub sc: u16,
    /// A UTF-16 code unit; 0 for none.
    pub ch: u16,
    pub down: bool,
    /// `dwControlKeyState`: the [`cks`] bits.
    pub state: u32,
    pub repeat: u16,
}

impl Win32Key {
    /// `ESC [ Vk ; Sc ; Uc ; Kd ; Cs ; Rc _`, every parameter written out.
    pub fn encode(&self) -> String {
        format!("\x1b[{};{};{};{};{};{}_", self.vk, self.sc, self.ch, self.down as u8, self.state, self.repeat.max(1))
    }
}

/// `dwControlKeyState` bits (wincon.h).
pub mod cks {
    pub const RIGHT_ALT: u32 = 0x1;
    pub const LEFT_ALT: u32 = 0x2;
    pub const RIGHT_CTRL: u32 = 0x4;
    pub const LEFT_CTRL: u32 = 0x8;
    pub const SHIFT: u32 = 0x10;
    pub const NUMLOCK: u32 = 0x20;
    pub const SCROLLLOCK: u32 = 0x40;
    pub const CAPSLOCK: u32 = 0x80;
    pub const ENHANCED: u32 = 0x100;
}

/// Virtual-key codes used here (winuser.h).
pub mod vk {
    pub const SHIFT: u16 = 0x10;
    pub const CONTROL: u16 = 0x11;
    pub const MENU: u16 = 0x12;
    pub const CAPITAL: u16 = 0x14;
    pub const NUMLOCK: u16 = 0x90;
    pub const SCROLL: u16 = 0x91;
    pub const LSHIFT: u16 = 0xA0;
    pub const RSHIFT: u16 = 0xA1;
    pub const LCONTROL: u16 = 0xA2;
    pub const RCONTROL: u16 = 0xA3;
    pub const LMENU: u16 = 0xA4;
    pub const RMENU: u16 = 0xA5;
    pub const LWIN: u16 = 0x5B;
    pub const RWIN: u16 = 0x5C;
    pub const PROCESSKEY: u16 = 0xE5;
}

/// The control key state from which keys are down (`down`) and which locks are on (`toggled`),
/// both by virtual-key code; `enhanced` for keys of the extended set (arrows, the right Ctrl...).
pub fn control_state(down: impl Fn(u16) -> bool, toggled: impl Fn(u16) -> bool, enhanced: bool) -> u32 {
    let mut s = 0;
    for (k, bit) in [
        (vk::RMENU, cks::RIGHT_ALT),
        (vk::LMENU, cks::LEFT_ALT),
        (vk::RCONTROL, cks::RIGHT_CTRL),
        (vk::LCONTROL, cks::LEFT_CTRL),
        (vk::SHIFT, cks::SHIFT),
    ] {
        if down(k) {
            s |= bit;
        }
    }
    for (k, bit) in [(vk::NUMLOCK, cks::NUMLOCK), (vk::SCROLL, cks::SCROLLLOCK), (vk::CAPITAL, cks::CAPSLOCK)] {
        if toggled(k) {
            s |= bit;
        }
    }
    if enhanced {
        s |= cks::ENHANCED;
    }
    s
}

/// The modifier and lock keys: their own events go to the console, and they are left to the
/// window too (which tracks modifiers for the mouse and the terminal's shortcuts).
pub fn is_modifier(v: u16) -> bool {
    matches!(
        v,
        vk::SHIFT
            | vk::CONTROL
            | vk::MENU
            | vk::LSHIFT
            | vk::RSHIFT
            | vk::LCONTROL
            | vk::RCONTROL
            | vk::LMENU
            | vk::RMENU
            | vk::CAPITAL
            | vk::NUMLOCK
            | vk::SCROLL
    )
}

/// A key as Windows reports it (`WM_KEYDOWN` and friends).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RawKey {
    pub vk: u16,
    pub sc: u16,
    pub state: u32,
    pub repeat: u16,
}

/// Pairs key presses with the characters Windows makes of them (`WM_CHAR` comes after its
/// `WM_KEYDOWN`), as the console itself does: a key that types sends one key-down event per
/// UTF-16 unit it makes, with the character; a dead key sends nothing (the next key carries the
/// composed character); a key that types nothing sends its key-down bare.
#[derive(Default)]
pub struct Composer {
    /// A key down whose characters are coming.
    pending: Option<RawKey>,
}

/// What `WM_KEYDOWN` is followed by, from a look at the message queue.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Next {
    Char,
    DeadChar,
    Nothing,
}

impl Composer {
    pub fn key_down(&mut self, key: RawKey, next: Next) -> Option<Win32Key> {
        match next {
            Next::Char => {
                self.pending = Some(key);
                None
            }
            Next::DeadChar => {
                self.pending = None;
                None
            }
            Next::Nothing => {
                self.pending = None;
                Some(Win32Key { vk: key.vk, sc: key.sc, ch: 0, down: true, state: key.state, repeat: key.repeat })
            }
        }
    }

    /// A `WM_CHAR` (one UTF-16 unit). A character with no key before it (sent by another
    /// program, or an input method) goes with no key code.
    pub fn char(&mut self, unit: u16) -> Win32Key {
        let key = self.pending.unwrap_or(RawKey { vk: 0, sc: 0, state: 0, repeat: 1 });
        Win32Key { vk: key.vk, sc: key.sc, ch: unit, down: true, state: key.state, repeat: key.repeat }
    }

    /// A `WM_DEADCHAR`: the accent waits for the next key.
    pub fn dead_char(&mut self) {
        self.pending = None;
    }

    pub fn key_up(&mut self, key: RawKey, ch: u16) -> Win32Key {
        if self.pending.is_some_and(|p| p.vk == key.vk) {
            self.pending = None;
        }
        Win32Key { vk: key.vk, sc: key.sc, ch, down: false, state: key.state, repeat: key.repeat }
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

    fn w(vk: u16, sc: u16, ch: u16, down: bool, state: u32) -> String {
        Win32Key { vk, sc, ch, down, state, repeat: 1 }.encode()
    }

    /// The spec's own examples (#4999, "win32-input-mode sequences"), byte for byte.
    #[test]
    fn win32_input_mode_matches_the_spec() {
        // Ctrl+F1.
        assert_eq!(w(17, 29, 0, true, 8), "\x1b[17;29;0;1;8;1_");
        assert_eq!(w(112, 59, 0, true, 8), "\x1b[112;59;0;1;8;1_");
        assert_eq!(w(112, 59, 0, false, 8), "\x1b[112;59;0;0;8;1_");
        assert_eq!(w(17, 29, 0, false, 0), "\x1b[17;29;0;0;0;1_");
        // Ctrl+Alt+A: no character.
        assert_eq!(w(18, 56, 0, true, 10), "\x1b[18;56;0;1;10;1_");
        assert_eq!(w(65, 30, 0, true, 10), "\x1b[65;30;0;1;10;1_");
        // Shift+A: 'A' down, 'a' up.
        assert_eq!(w(16, 42, 0, true, 16), "\x1b[16;42;0;1;16;1_");
        assert_eq!(w(65, 30, 65, true, 16), "\x1b[65;30;65;1;16;1_");
        assert_eq!(w(65, 30, 97, false, 0), "\x1b[65;30;97;0;0;1_");
        // A repeat count of 0 is never sent (the spec's default is 1).
        assert_eq!(Win32Key { vk: 65, sc: 30, ch: 97, down: true, state: 0, repeat: 0 }.encode(), "\x1b[65;30;97;1;0;1_");
    }

    #[test]
    fn control_key_state_bits() {
        let none = |_: u16| false;
        assert_eq!(control_state(|k| k == vk::LCONTROL, none, false), cks::LEFT_CTRL);
        // AltGr is right Alt with left Ctrl.
        assert_eq!(control_state(|k| k == vk::RMENU || k == vk::LCONTROL, none, false), cks::RIGHT_ALT | cks::LEFT_CTRL);
        assert_eq!(control_state(|k| k == vk::SHIFT, |k| k == vk::NUMLOCK, false), 0x30);
        assert_eq!(control_state(none, |k| k == vk::CAPITAL || k == vk::SCROLL, true), cks::CAPSLOCK | cks::SCROLLLOCK | cks::ENHANCED);
    }

    #[test]
    fn keys_pair_with_the_characters_they_make() {
        let mut c = Composer::default();
        let ctrl_c = RawKey { vk: 0x43, sc: 0x2E, state: cks::LEFT_CTRL, repeat: 1 };
        // Ctrl+C makes the character 3: one key-down with it, as the console's own.
        assert_eq!(c.key_down(ctrl_c, Next::Char), None);
        assert_eq!(c.char(3), Win32Key { vk: 0x43, sc: 0x2E, ch: 3, down: true, state: cks::LEFT_CTRL, repeat: 1 });
        assert_eq!(c.key_up(ctrl_c, 3).encode(), "\x1b[67;46;3;0;8;1_");
        // An arrow makes no character: its key-down goes bare.
        let left = RawKey { vk: 0x25, sc: 0x4B, state: cks::SHIFT | cks::ENHANCED, repeat: 1 };
        assert_eq!(c.key_down(left, Next::Nothing).unwrap().encode(), "\x1b[37;75;0;1;272;1_");
        // A dead key sends nothing; the next key carries the composed character.
        let acute = RawKey { vk: 0xDD, sc: 0x0D, state: 0, repeat: 1 };
        assert_eq!(c.key_down(acute, Next::DeadChar), None);
        c.dead_char();
        let e = RawKey { vk: 0x45, sc: 0x12, state: 0, repeat: 1 };
        assert_eq!(c.key_down(e, Next::Char), None);
        assert_eq!(c.char(0xE9).encode(), "\x1b[69;18;233;1;0;1_");
        // A character outside the BMP is two events, one per UTF-16 unit, with the same key.
        let emoji = RawKey { vk: 0xE7, sc: 0, state: 0, repeat: 1 };
        c.key_down(emoji, Next::Char);
        assert_eq!(c.char(0xD83D).ch, 0xD83D);
        assert_eq!(c.char(0xDE00).vk, 0xE7);
    }

    #[test]
    fn modifiers_are_modifiers() {
        assert!(is_modifier(vk::LSHIFT) && is_modifier(vk::CONTROL) && is_modifier(vk::RMENU) && is_modifier(vk::CAPITAL));
        assert!(!is_modifier(0x41) && !is_modifier(vk::LWIN));
    }

    #[test]
    fn local_keys() {
        assert_eq!(local(Key::PageUp, SHIFT), Some(Local::ScrollPage(1)));
        assert_eq!(local(Key::PageDown, SHIFT), Some(Local::ScrollPage(-1)));
        assert_eq!(local(Key::PageUp, Mods::NONE), None);
        assert_eq!(local(Key::Char('v'), CTRL), None);
    }
}
