//! The per-user registry, as far as setup touches it: string and number values under HKCU, and
//! whole keys of its own. Behind a trait so every change is tested against a fake; and with
//! `SHELLMAN_SETUP_REGISTRY=<file>` the whole tool (and the installer) uses a file standing in
//! for HKCU, so a test install never writes the real registry.
//!
//! Key paths are relative to HKCU, `\`-separated; the value name `""` is the key's default value.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

pub trait Registry: Send + Sync {
    fn get(&self, key: &str, name: &str) -> Option<String>;
    fn set(&self, key: &str, name: &str, value: &str) -> Result<(), String>;
    fn set_dword(&self, key: &str, name: &str, value: u32) -> Result<(), String>;
    /// Deleting a value that is not there is not an error.
    fn delete_value(&self, key: &str, name: &str) -> Result<(), String>;
    /// The key, its values and every subkey. A missing key is not an error.
    fn delete_tree(&self, key: &str) -> Result<(), String>;
    fn key_exists(&self, key: &str) -> bool;
    /// Whether this is the real registry (the installer calls the apps' own `--install` and
    /// `--register-default-terminal` only then; a stand-in is written directly).
    fn is_real(&self) -> bool;
}

/// The registry to use: the stand-in file when `SHELLMAN_SETUP_REGISTRY` names one, else HKCU.
pub fn open() -> Box<dyn Registry> {
    match std::env::var("SHELLMAN_SETUP_REGISTRY").ok().filter(|v| !v.trim().is_empty()) {
        Some(path) => Box::new(FileReg::load(PathBuf::from(path))),
        None => Box::new(Hkcu),
    }
}

fn norm(key: &str) -> String {
    key.trim_matches('\\').to_ascii_lowercase()
}

/// HKCU itself.
pub struct Hkcu;

mod win {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR};
    use windows::Win32::System::Registry::{
        RegCloseKey, RegDeleteKeyValueW, RegDeleteTreeW, RegGetValueW, RegOpenKeyExW, RegSetKeyValueW, HKEY,
        HKEY_CURRENT_USER, KEY_READ, REG_DWORD, REG_SZ, RRF_RT_DWORD, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ,
        RRF_NOEXPAND,
    };

    fn ok(r: WIN32_ERROR, what: &str) -> Result<(), String> {
        if r == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(format!("{what}: {}", windows::core::Error::from(r.to_hresult()).message()))
        }
    }

    fn missing_ok(r: WIN32_ERROR, what: &str) -> Result<(), String> {
        if r == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            ok(r, what)
        }
    }

    pub fn get(key: &str, name: &str) -> Option<String> {
        let mut buf = vec![0u16; 4096];
        let mut size = (buf.len() * 2) as u32;
        let r = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                &HSTRING::from(key),
                &HSTRING::from(name),
                RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ | RRF_NOEXPAND,
                None,
                Some(buf.as_mut_ptr() as *mut _),
                Some(&mut size),
            )
        };
        if r == ERROR_SUCCESS {
            let chars = (size as usize / 2).saturating_sub(1).min(buf.len());
            return Some(String::from_utf16_lossy(&buf[..chars]));
        }
        let mut n = 0u32;
        let mut size = 4u32;
        let r = unsafe {
            RegGetValueW(HKEY_CURRENT_USER, &HSTRING::from(key), &HSTRING::from(name), RRF_RT_DWORD, None, Some(&mut n as *mut u32 as *mut _), Some(&mut size))
        };
        (r == ERROR_SUCCESS).then(|| n.to_string())
    }

    pub fn set(key: &str, name: &str, value: &str) -> Result<(), String> {
        let wide: Vec<u16> = value.encode_utf16().chain(Some(0)).collect();
        let r = unsafe {
            RegSetKeyValueW(HKEY_CURRENT_USER, &HSTRING::from(key), &HSTRING::from(name), REG_SZ.0, Some(wide.as_ptr() as *const _), (wide.len() * 2) as u32)
        };
        ok(r, &format!("cannot write HKCU\\{key} {name}"))
    }

    pub fn set_dword(key: &str, name: &str, value: u32) -> Result<(), String> {
        let r = unsafe {
            RegSetKeyValueW(HKEY_CURRENT_USER, &HSTRING::from(key), &HSTRING::from(name), REG_DWORD.0, Some(&value as *const u32 as *const _), 4)
        };
        ok(r, &format!("cannot write HKCU\\{key} {name}"))
    }

    pub fn delete_value(key: &str, name: &str) -> Result<(), String> {
        let r = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, &HSTRING::from(key), &HSTRING::from(name)) };
        missing_ok(r, &format!("cannot delete HKCU\\{key} {name}"))
    }

    pub fn delete_tree(key: &str) -> Result<(), String> {
        let r = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(key)) };
        missing_ok(r, &format!("cannot delete HKCU\\{key}"))
    }

    pub fn key_exists(key: &str) -> bool {
        let mut h = HKEY::default();
        let r = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(HSTRING::from(key).as_ptr()), Some(0), KEY_READ, &mut h) };
        if r == ERROR_SUCCESS {
            unsafe {
                let _ = RegCloseKey(h);
            }
            true
        } else {
            false
        }
    }
}

impl Registry for Hkcu {
    fn get(&self, key: &str, name: &str) -> Option<String> {
        win::get(key, name)
    }
    fn set(&self, key: &str, name: &str, value: &str) -> Result<(), String> {
        win::set(key, name, value)
    }
    fn set_dword(&self, key: &str, name: &str, value: u32) -> Result<(), String> {
        win::set_dword(key, name, value)
    }
    fn delete_value(&self, key: &str, name: &str) -> Result<(), String> {
        win::delete_value(key, name)
    }
    fn delete_tree(&self, key: &str) -> Result<(), String> {
        win::delete_tree(key)
    }
    fn key_exists(&self, key: &str) -> bool {
        win::key_exists(key)
    }
    fn is_real(&self) -> bool {
        true
    }
}

/// An in-memory registry, saved to a file after every change when it has one. Lines are
/// `key<TAB>name<TAB>value`; a key with no values is a line with an empty name and `\u{1}`.
pub struct FileReg {
    file: Option<PathBuf>,
    map: Mutex<BTreeMap<(String, String), String>>,
}

const KEY_ONLY: &str = "\u{1}";

impl FileReg {
    pub fn memory() -> FileReg {
        FileReg { file: None, map: Mutex::new(BTreeMap::new()) }
    }

    pub fn load(file: PathBuf) -> FileReg {
        let mut map = BTreeMap::new();
        if let Ok(text) = std::fs::read_to_string(&file) {
            for line in text.lines() {
                let mut it = line.splitn(3, '\t');
                if let (Some(k), Some(n), Some(v)) = (it.next(), it.next(), it.next()) {
                    map.insert((k.to_string(), n.to_string()), v.to_string());
                }
            }
        }
        FileReg { file: Some(file), map: Mutex::new(map) }
    }

    fn save(&self, map: &BTreeMap<(String, String), String>) -> Result<(), String> {
        let Some(f) = &self.file else { return Ok(()) };
        let mut s = String::new();
        for ((k, n), v) in map {
            s.push_str(&format!("{k}\t{n}\t{v}\n"));
        }
        std::fs::write(f, s).map_err(|e| format!("cannot write the stand-in registry {}: {e}", f.display()))
    }

    fn edit(&self, f: impl FnOnce(&mut BTreeMap<(String, String), String>)) -> Result<(), String> {
        let mut map = self.map.lock().map_err(|_| "registry lock poisoned".to_string())?;
        f(&mut map);
        self.save(&map)
    }

    /// Every `(key, name, value)` (for tests).
    pub fn dump(&self) -> Vec<(String, String, String)> {
        self.map.lock().map(|m| m.iter().filter(|(_, v)| v.as_str() != KEY_ONLY).map(|((k, n), v)| (k.clone(), n.clone(), v.clone())).collect()).unwrap_or_default()
    }
}

impl Registry for FileReg {
    fn get(&self, key: &str, name: &str) -> Option<String> {
        let m = self.map.lock().ok()?;
        m.get(&(norm(key), name.to_ascii_lowercase())).filter(|v| v.as_str() != KEY_ONLY).cloned()
    }
    fn set(&self, key: &str, name: &str, value: &str) -> Result<(), String> {
        self.edit(|m| {
            m.insert((norm(key), name.to_ascii_lowercase()), value.replace(['\t', '\n'], " "));
        })
    }
    fn set_dword(&self, key: &str, name: &str, value: u32) -> Result<(), String> {
        self.set(key, name, &value.to_string())
    }
    fn delete_value(&self, key: &str, name: &str) -> Result<(), String> {
        self.edit(|m| {
            let k = norm(key);
            if m.remove(&(k.clone(), name.to_ascii_lowercase())).is_some() && !m.keys().any(|(kk, _)| *kk == k) {
                m.insert((k, String::new()), KEY_ONLY.into());
            }
        })
    }
    fn delete_tree(&self, key: &str) -> Result<(), String> {
        let k = norm(key);
        let below = format!("{k}\\");
        self.edit(|m| m.retain(|(kk, _), _| *kk != k && !kk.starts_with(&below)))
    }
    fn key_exists(&self, key: &str) -> bool {
        let k = norm(key);
        let below = format!("{k}\\");
        self.map.lock().is_ok_and(|m| m.keys().any(|(kk, _)| *kk == k || kk.starts_with(&below)))
    }
    fn is_real(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stand_in_behaves_like_a_registry() {
        let r = FileReg::memory();
        r.set(r"Software\A\B", "x", "1").unwrap();
        r.set(r"Software\A", "", "dflt").unwrap();
        assert_eq!(r.get(r"software\a\b", "X").as_deref(), Some("1"));
        assert!(r.key_exists(r"Software\A"));
        r.delete_value(r"Software\A\B", "x").unwrap();
        assert!(r.key_exists(r"Software\A\B"), "an emptied key stays");
        assert_eq!(r.get(r"Software\A\B", "x"), None);
        r.delete_tree(r"Software\A").unwrap();
        assert!(!r.key_exists(r"Software\A") && !r.key_exists(r"Software\A\B"));
        r.delete_tree(r"Software\none").unwrap();
        r.delete_value(r"Software\none", "x").unwrap();
    }

    #[test]
    fn the_real_registry_round_trips_on_a_throwaway_key() {
        let base = format!(r"Software\shellman-setup-selftest-{}", std::process::id());
        let r = Hkcu;
        r.set(&format!(r"{base}\Sub"), "s", r"C:\a b\c.exe").unwrap();
        r.set(&base, "", "default").unwrap();
        r.set_dword(&base, "n", 42).unwrap();
        assert_eq!(r.get(&format!(r"{base}\Sub"), "s").as_deref(), Some(r"C:\a b\c.exe"));
        assert_eq!(r.get(&base, "").as_deref(), Some("default"));
        assert_eq!(r.get(&base, "n").as_deref(), Some("42"));
        assert!(r.key_exists(&base) && r.key_exists(&format!(r"{base}\Sub")));
        r.delete_value(&base, "n").unwrap();
        r.delete_value(&base, "n").unwrap();
        assert_eq!(r.get(&base, "n"), None);
        r.delete_tree(&base).unwrap();
        assert!(!r.key_exists(&base));
        r.delete_tree(&base).unwrap();
        r.delete_value(&base, "x").unwrap();
    }

    #[test]
    fn the_stand_in_file_round_trips() {
        let f = std::env::temp_dir().join(format!("shellman-setup-reg-test-{}.txt", std::process::id()));
        let r = FileReg::load(f.clone());
        r.set(r"Software\K", "v", "C:\\x y\\z.exe").unwrap();
        r.set_dword(r"Software\K", "n", 7).unwrap();
        let again = FileReg::load(f.clone());
        assert_eq!(again.get(r"Software\K", "v").as_deref(), Some("C:\\x y\\z.exe"));
        assert_eq!(again.get(r"Software\K", "n").as_deref(), Some("7"));
        let _ = std::fs::remove_file(f);
    }
}
