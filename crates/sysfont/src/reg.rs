//! Small helpers over the registry calls: string values only, which is all fonts use.
//!
//! Reading is always the real registry. Writing refuses in a dry run (see `write_allowed`), so a
//! dry run cannot change the registry even through a path that forgot to use its stand-ins.

use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegEnumValueW, RegGetValueW, RegOpenKeyExW,
    RegSetValueExW, HKEY, KEY_READ, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ, RRF_NOEXPAND,
    RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ,
};

pub use windows::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

pub const FONTS_KEY: &str = "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Fonts";
pub const SUBSTITUTES_KEY: &str =
    "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\FontSubstitutes";

struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

fn open(root: HKEY, path: &str, write: bool) -> Result<Key, String> {
    let mut key = HKEY::default();
    let access = if write {
        KEY_READ | KEY_WRITE
    } else {
        KEY_READ
    };
    let err = unsafe { RegOpenKeyExW(root, &HSTRING::from(path), Some(0), access, &mut key) };
    if err != ERROR_SUCCESS {
        return Err(format!("Could not open {path} ({}).", err.0));
    }
    Ok(Key(key))
}

fn create(root: HKEY, path: &str) -> Result<Key, String> {
    let mut key = HKEY::default();
    let err = unsafe {
        RegCreateKeyExW(
            root,
            &HSTRING::from(path),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_READ | KEY_WRITE,
            None,
            &mut key,
            None,
        )
    };
    if err != ERROR_SUCCESS {
        return Err(format!("Could not open {path} for writing ({}).", err.0));
    }
    Ok(Key(key))
}

/// Every string value under a key, in registry order. A missing key has none.
pub fn values(root: HKEY, path: &str) -> Vec<(String, String)> {
    let Ok(key) = open(root, path, false) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut index = 0u32;
    loop {
        let mut name = vec![0u16; 16384];
        let mut name_len = name.len() as u32;
        let mut data = vec![0u8; 8192];
        let mut data_len = data.len() as u32;
        let mut kind = 0u32;
        let err = unsafe {
            RegEnumValueW(
                key.0,
                index,
                Some(PWSTR(name.as_mut_ptr())),
                &mut name_len,
                None,
                Some(&mut kind),
                Some(data.as_mut_ptr()),
                Some(&mut data_len),
            )
        };
        index += 1;
        if err == ERROR_NO_MORE_ITEMS {
            break;
        }
        if err != ERROR_SUCCESS {
            // A value too long for the buffer is not a font entry worth reading.
            if index > 100_000 {
                break;
            }
            continue;
        }
        if kind != REG_SZ.0 && kind != 2 {
            continue;
        }
        let name = String::from_utf16_lossy(&name[..name_len as usize]);
        let wide: Vec<u16> = data[..data_len as usize]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect();
        let end = wide.iter().position(|ch| *ch == 0).unwrap_or(wide.len());
        out.push((name, String::from_utf16_lossy(&wide[..end])));
    }
    out
}

pub fn get(root: HKEY, path: &str, name: &str) -> Option<String> {
    let mut size = 0u32;
    let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ | RRF_NOEXPAND;
    let (path, name) = (HSTRING::from(path), HSTRING::from(name));
    let err = unsafe { RegGetValueW(root, &path, &name, flags, None, None, Some(&mut size)) };
    if err != ERROR_SUCCESS {
        return None;
    }
    let mut buf = vec![0u16; size as usize / 2 + 1];
    let mut size = (buf.len() * 2) as u32;
    let err = unsafe {
        RegGetValueW(
            root,
            &path,
            &name,
            flags,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if err != ERROR_SUCCESS {
        return None;
    }
    let end = buf.iter().position(|ch| *ch == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..end]))
}

/// Whether this run may write the registry at all: never in a dry run.
pub fn write_allowed(dry: bool) -> Result<(), String> {
    if dry {
        return Err("A dry run does not write the registry.".into());
    }
    Ok(())
}

pub fn set(root: HKEY, path: &str, name: &str, value: &str) -> Result<(), String> {
    write_allowed(crate::settings::dry_run())?;
    let key = create(root, path)?;
    let wide: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes: Vec<u8> = wide.iter().flat_map(|ch| ch.to_le_bytes()).collect();
    let err = unsafe { RegSetValueExW(key.0, &HSTRING::from(name), None, REG_SZ, Some(&bytes)) };
    if err != ERROR_SUCCESS {
        return Err(format!("Could not set {name} ({}).", err.0));
    }
    Ok(())
}

pub fn delete(root: HKEY, path: &str, name: &str) -> Result<(), String> {
    write_allowed(crate::settings::dry_run())?;
    let key = match open(root, path, true) {
        Ok(key) => key,
        Err(_) => return Ok(()),
    };
    let err = unsafe { RegDeleteValueW(key.0, &HSTRING::from(name)) };
    if err == ERROR_SUCCESS || err == ERROR_FILE_NOT_FOUND {
        Ok(())
    } else {
        Err(format!("Could not delete {name} ({}).", err.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dry_run_never_writes_the_registry() {
        assert!(write_allowed(true).is_err());
        assert!(write_allowed(false).is_ok());
        // Tests are dry: even a direct call is refused before the registry is opened.
        assert!(set(HKEY_CURRENT_USER, "Software\\shellman-sysfont-test", "x", "y").is_err());
        assert!(delete(HKEY_CURRENT_USER, "Software\\shellman-sysfont-test", "x").is_err());
    }
}
