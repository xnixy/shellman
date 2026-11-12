//! The message apps send when they add, change or remove a tray icon.
//!
//! `Shell_NotifyIcon` packs a `NOTIFYICONDATAW` behind a small header and sends it to the tray
//! window with `WM_COPYDATA`. Window and icon handles inside are 32 bits wide even for 64-bit
//! apps (handles only use the low 32 bits), so the layout is fixed and read by offset.

/// The header's first field, checked so stray `WM_COPYDATA` is ignored.
pub const SIGNATURE: u32 = 0x3475_3423;

pub const NIM_ADD: u32 = 0;
pub const NIM_MODIFY: u32 = 1;
pub const NIM_DELETE: u32 = 2;
pub const NIM_SETVERSION: u32 = 4;

pub const NIF_MESSAGE: u32 = 0x01;
pub const NIF_ICON: u32 = 0x02;
pub const NIF_TIP: u32 = 0x04;
pub const NIF_STATE: u32 = 0x08;
pub const NIF_GUID: u32 = 0x20;
pub const NIS_HIDDEN: u32 = 0x01;

/// One tray request.
#[derive(Debug, Clone, PartialEq)]
pub struct Notify {
    pub message: u32,
    pub hwnd: isize,
    pub uid: u32,
    pub flags: u32,
    pub callback: u32,
    pub icon: isize,
    pub tip: String,
    pub state: u32,
    pub state_mask: u32,
    pub version: u32,
    pub guid: Option<[u8; 16]>,
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn text_at(b: &[u8], at: usize, chars: usize) -> String {
    let Some(raw) = b.get(at..at + chars * 2) else {
        return String::new();
    };
    let units: Vec<u16> = raw
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .take_while(|&c| c != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

/// Read a tray request. Older apps send a shorter structure, so later fields are read only when
/// its size says they are there.
pub fn parse(bytes: &[u8]) -> Option<Notify> {
    if u32_at(bytes, 0)? != SIGNATURE {
        return None;
    }
    let message = u32_at(bytes, 4)?;
    let nid = bytes.get(8..)?;
    let size = u32_at(nid, 0)? as usize;
    // The first version of the structure had a 64-character tip; later ones 128.
    let tip_chars = if size >= 280 { 128 } else { 64 };
    let has = |end: usize| size >= end && nid.len() >= end;
    Some(Notify {
        message,
        hwnd: u32_at(nid, 4)? as i32 as isize,
        uid: u32_at(nid, 8)?,
        flags: u32_at(nid, 12)?,
        callback: u32_at(nid, 16)?,
        icon: u32_at(nid, 20)? as i32 as isize,
        tip: text_at(nid, 24, tip_chars),
        state: if has(284) { u32_at(nid, 280)? } else { 0 },
        state_mask: if has(288) { u32_at(nid, 284)? } else { 0 },
        version: if has(804) { u32_at(nid, 800)? } else { 0 },
        guid: if has(952) {
            Some(nid.get(936..952)?.try_into().ok()?)
        } else {
            None
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(size: u32, tip: &str) -> Vec<u8> {
        let mut b = vec![0u8; 8 + 956];
        b[0..4].copy_from_slice(&SIGNATURE.to_le_bytes());
        b[4..8].copy_from_slice(&NIM_ADD.to_le_bytes());
        let n = &mut b[8..];
        n[0..4].copy_from_slice(&size.to_le_bytes());
        n[4..8].copy_from_slice(&0x0012_3456u32.to_le_bytes());
        n[8..12].copy_from_slice(&7u32.to_le_bytes());
        n[12..16].copy_from_slice(&(NIF_MESSAGE | NIF_ICON | NIF_TIP).to_le_bytes());
        n[16..20].copy_from_slice(&0x8001u32.to_le_bytes());
        n[20..24].copy_from_slice(&0xFFFF_FF00u32.to_le_bytes());
        for (i, u) in tip.encode_utf16().enumerate() {
            n[24 + i * 2..26 + i * 2].copy_from_slice(&u.to_le_bytes());
        }
        n[800..804].copy_from_slice(&4u32.to_le_bytes());
        b
    }

    #[test]
    fn reads_a_full_request() {
        let n = parse(&request(956, "Discord")).unwrap();
        assert_eq!((n.message, n.hwnd, n.uid, n.callback), (NIM_ADD, 0x12_3456, 7, 0x8001));
        assert_eq!(n.icon, -256, "handles are sign-extended from 32 bits");
        assert_eq!(n.tip, "Discord");
        assert_eq!(n.version, 4);
        assert!(n.guid.is_some());
    }

    #[test]
    fn old_short_requests_skip_later_fields() {
        let n = parse(&request(152, "old app")).unwrap();
        assert_eq!(n.tip, "old app");
        assert_eq!((n.version, n.guid), (0, None));
    }

    #[test]
    fn ignores_other_copydata() {
        assert_eq!(parse(&[0u8; 16]), None);
    }
}
