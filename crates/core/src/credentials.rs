//! Secrets in Windows Credential Manager (generic credentials of the user), never in a settings
//! file or a log: the calendar's app password lives at `shellman/caldav`. A test copy
//! (`SHELLMAN_INSTANCE`) keeps its own, `shellman/caldav/<instance>`.

use windows::core::{HSTRING, PWSTR};
use windows::Win32::Foundation::FILETIME;
use windows::Win32::Security::Credentials::{
    CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_FLAGS, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
};

/// The name a secret is kept under, with the test instance if any.
pub fn target(name: &str) -> String {
    match std::env::var("SHELLMAN_INSTANCE").ok().filter(|i| !i.is_empty()) {
        Some(i) => format!("shellman/{name}/{i}"),
        None => format!("shellman/{name}"),
    }
}

/// A saved secret: the user name it was saved with, and the secret.
pub struct Secret {
    pub user: String,
    pub secret: String,
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never the secret, even in a debug print.
        f.debug_struct("Secret").field("user", &self.user).finish_non_exhaustive()
    }
}

/// Read the secret saved as `name`.
pub fn read(name: &str) -> Option<Secret> {
    let t = HSTRING::from(target(name));
    unsafe {
        let mut cred: *mut CREDENTIALW = std::ptr::null_mut();
        CredReadW(&t, CRED_TYPE_GENERIC, None, &mut cred).ok()?;
        let c = &*cred;
        let blob = std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize);
        let secret = decode(blob);
        let user = if c.UserName.is_null() { String::new() } else { c.UserName.to_string().unwrap_or_default() };
        CredFree(cred as *const _);
        Some(Secret { user, secret: secret? })
    }
}

/// Save `secret` as `name` for `user`, replacing what was there.
pub fn write(name: &str, user: &str, secret: &str) -> Result<(), String> {
    let mut t: Vec<u16> = target(name).encode_utf16().chain(Some(0)).collect();
    let mut u: Vec<u16> = user.encode_utf16().chain(Some(0)).collect();
    // UTF-16, as Windows' own tools (cmdkey, Credential Manager) write a generic credential.
    let mut blob: Vec<u8> = secret.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let cred = CREDENTIALW {
        Flags: CRED_FLAGS(0),
        Type: CRED_TYPE_GENERIC,
        TargetName: PWSTR(t.as_mut_ptr()),
        Comment: PWSTR::null(),
        LastWritten: FILETIME::default(),
        CredentialBlobSize: blob.len() as u32,
        CredentialBlob: blob.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        AttributeCount: 0,
        Attributes: std::ptr::null_mut(),
        TargetAlias: PWSTR::null(),
        UserName: PWSTR(u.as_mut_ptr()),
    };
    let r = unsafe { CredWriteW(&cred, 0) }.map_err(|e| format!("Windows could not save it ({})", e.code()));
    // Wipe the copy of the secret.
    blob.iter_mut().for_each(|b| *b = 0);
    r
}

/// A credential's blob as text: UTF-16 (as written here and by Windows' tools), else UTF-8.
fn decode(blob: &[u8]) -> Option<String> {
    if blob.len() % 2 == 0 && !blob.is_empty() {
        let units: Vec<u16> = blob.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        if let Ok(s) = String::from_utf16(&units) {
            if !s.contains('\0') {
                return Some(s);
            }
        }
    }
    String::from_utf8(blob.to_vec()).ok().filter(|s| !s.contains('\0'))
}

/// Forget the secret saved as `name`.
pub fn delete(name: &str) {
    let t = HSTRING::from(target(name));
    unsafe {
        let _ = CredDeleteW(&t, CRED_TYPE_GENERIC, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_carry_the_instance() {
        // The environment is the process's; only check the shape.
        let t = target("caldav");
        assert!(t == "shellman/caldav" || t.starts_with("shellman/caldav/"));
    }

    #[test]
    fn blobs_in_either_encoding() {
        let utf16: Vec<u8> = "pässword".encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert_eq!(decode(&utf16).as_deref(), Some("pässword"));
        assert_eq!(decode(b"abc").as_deref(), Some("abc"));
        assert_eq!(decode(b""), Some(String::new()));
    }

    #[test]
    fn debug_never_shows_the_secret() {
        let s = Secret { user: "a".into(), secret: "hunter2".into() };
        assert!(!format!("{s:?}").contains("hunter2"));
    }
}
