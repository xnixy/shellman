//! The little of the registry the session needs, behind a trait. Everything that decides what to
//! read, set or delete (startup items, RunOnce deletion, the shell swap) is written against the
//! trait and tested with `fake::Fake`; only `Real` touches the machine.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hive {
    Hkcu,
    Hklm,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Value {
    Text(String),
    /// REG_EXPAND_SZ: the text still has `%VARS%` in it.
    Expand(String),
    Binary(Vec<u8>),
    Dword(u32),
}

impl Value {
    pub fn text(&self) -> Option<&str> {
        match self {
            Value::Text(s) | Value::Expand(s) => Some(s),
            _ => None,
        }
    }
}

pub trait Registry {
    /// Every value of a key; empty when the key is missing.
    fn list(&self, hive: Hive, key: &str) -> Vec<(String, Value)>;
    fn get(&self, hive: Hive, key: &str, name: &str) -> Option<Value>;
    fn set_text(&self, hive: Hive, key: &str, name: &str, text: &str) -> Result<(), String>;
    fn delete(&self, hive: Hive, key: &str, name: &str) -> Result<(), String>;
    /// Create a key that disappears when the user logs off. `Ok(true)` when it was new.
    fn create_volatile(&self, hive: Hive, key: &str) -> Result<bool, String>;
}

#[cfg(windows)]
pub use real::Real;

#[cfg(windows)]
mod real {
    use super::*;
    use windows::core::{HSTRING, PCWSTR, PWSTR};
    use windows::Win32::Foundation::ERROR_NO_MORE_ITEMS;
    use windows::Win32::System::Registry::*;

    pub struct Real;

    fn root(h: Hive) -> HKEY {
        match h {
            Hive::Hkcu => HKEY_CURRENT_USER,
            Hive::Hklm => HKEY_LOCAL_MACHINE,
        }
    }

    fn open(hive: Hive, key: &str, access: REG_SAM_FLAGS) -> Option<HKEY> {
        let mut hk = HKEY::default();
        unsafe { RegOpenKeyExW(root(hive), &HSTRING::from(key), None, access, &mut hk) }.is_ok().then_some(hk)
    }

    fn query(hk: HKEY, name: &str) -> Option<Value> {
        let name = HSTRING::from(name);
        let (mut kind, mut len) = (REG_VALUE_TYPE(0), 0u32);
        unsafe {
            RegQueryValueExW(hk, &name, None, Some(&mut kind), None, Some(&mut len)).ok().ok()?;
            let mut data = vec![0u8; len as usize + 2];
            len = data.len() as u32;
            RegQueryValueExW(hk, &name, None, Some(&mut kind), Some(data.as_mut_ptr()), Some(&mut len)).ok().ok()?;
            data.truncate(len as usize);
            let text = |d: &[u8]| {
                let w: Vec<u16> = d.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
                String::from_utf16_lossy(&w).trim_end_matches('\0').to_string()
            };
            Some(match kind {
                REG_SZ => Value::Text(text(&data)),
                REG_EXPAND_SZ => Value::Expand(text(&data)),
                REG_DWORD if data.len() >= 4 => Value::Dword(u32::from_le_bytes([data[0], data[1], data[2], data[3]])),
                _ => Value::Binary(data),
            })
        }
    }

    impl Registry for Real {
        fn list(&self, hive: Hive, key: &str) -> Vec<(String, Value)> {
            let Some(hk) = open(hive, key, KEY_READ) else { return Vec::new() };
            let mut out = Vec::new();
            for i in 0.. {
                let mut buf = vec![0u16; 16384];
                let mut len = buf.len() as u32;
                let r = unsafe { RegEnumValueW(hk, i, Some(PWSTR(buf.as_mut_ptr())), &mut len, None, None, None, None) };
                if r == ERROR_NO_MORE_ITEMS || r.is_err() {
                    break;
                }
                let name = String::from_utf16_lossy(&buf[..len as usize]);
                if let Some(v) = query(hk, &name) {
                    out.push((name, v));
                }
            }
            unsafe {
                let _ = RegCloseKey(hk);
            }
            out
        }

        fn get(&self, hive: Hive, key: &str, name: &str) -> Option<Value> {
            let hk = open(hive, key, KEY_READ)?;
            let v = query(hk, name);
            unsafe {
                let _ = RegCloseKey(hk);
            }
            v
        }

        fn set_text(&self, hive: Hive, key: &str, name: &str, text: &str) -> Result<(), String> {
            // The key may not be there yet (shellman's own key on a first install), so create it.
            let mut hk = HKEY::default();
            unsafe {
                RegCreateKeyExW(
                    root(hive),
                    &HSTRING::from(key),
                    None,
                    PCWSTR::null(),
                    REG_OPTION_NON_VOLATILE,
                    KEY_SET_VALUE,
                    None,
                    &mut hk,
                    None,
                )
            }
            .ok()
            .map_err(|e| format!("cannot open the key ({e})"))?;
            let wide: Vec<u8> = text.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect();
            let r = unsafe { RegSetValueExW(hk, &HSTRING::from(name), None, REG_SZ, Some(&wide)) };
            unsafe {
                let _ = RegCloseKey(hk);
            }
            r.ok().map_err(|e| e.to_string())
        }

        fn delete(&self, hive: Hive, key: &str, name: &str) -> Result<(), String> {
            let hk = open(hive, key, KEY_SET_VALUE).ok_or("cannot open the key")?;
            let r = unsafe { RegDeleteValueW(hk, &HSTRING::from(name)) };
            unsafe {
                let _ = RegCloseKey(hk);
            }
            r.ok().map_err(|e| e.to_string())
        }

        fn create_volatile(&self, hive: Hive, key: &str) -> Result<bool, String> {
            let mut hk = HKEY::default();
            let mut disposition = REG_CREATE_KEY_DISPOSITION(0);
            let r = unsafe {
                RegCreateKeyExW(
                    root(hive),
                    &HSTRING::from(key),
                    None,
                    PCWSTR::null(),
                    REG_OPTION_VOLATILE,
                    KEY_WRITE,
                    None,
                    &mut hk,
                    Some(&mut disposition),
                )
            };
            r.ok().map_err(|e| e.to_string())?;
            unsafe {
                let _ = RegCloseKey(hk);
            }
            Ok(disposition == REG_CREATED_NEW_KEY)
        }
    }
}

/// An in-memory registry for tests. Keys compare case-insensitively, as in Windows.
#[cfg(test)]
pub mod fake {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    type Key = (bool, String, String);

    #[derive(Default)]
    pub struct Fake {
        values: RefCell<BTreeMap<Key, (String, Value)>>,
        volatile: RefCell<Vec<(bool, String)>>,
        /// Names whose delete or set must fail, to test the failure paths.
        pub refuse: RefCell<Vec<String>>,
        pub log: RefCell<Vec<String>>,
    }

    fn k(hive: Hive, key: &str, name: &str) -> Key {
        (hive == Hive::Hklm, key.to_lowercase(), name.to_lowercase())
    }

    impl Fake {
        pub fn put(&self, hive: Hive, key: &str, name: &str, v: Value) {
            self.values.borrow_mut().insert(k(hive, key, name), (name.to_string(), v));
        }
    }

    impl Registry for Fake {
        fn list(&self, hive: Hive, key: &str) -> Vec<(String, Value)> {
            let (h, key) = (hive == Hive::Hklm, key.to_lowercase());
            self.values.borrow().iter().filter(|((a, b, _), _)| *a == h && *b == key).map(|(_, (n, v))| (n.clone(), v.clone())).collect()
        }

        fn get(&self, hive: Hive, key: &str, name: &str) -> Option<Value> {
            self.values.borrow().get(&k(hive, key, name)).map(|(_, v)| v.clone())
        }

        fn set_text(&self, hive: Hive, key: &str, name: &str, text: &str) -> Result<(), String> {
            if self.refuse.borrow().iter().any(|r| r.eq_ignore_ascii_case(name)) {
                return Err("refused".into());
            }
            self.log.borrow_mut().push(format!("set {name}={text}"));
            self.put(hive, key, name, Value::Text(text.to_string()));
            Ok(())
        }

        fn delete(&self, hive: Hive, key: &str, name: &str) -> Result<(), String> {
            if self.refuse.borrow().iter().any(|r| r.eq_ignore_ascii_case(name)) {
                return Err("refused".into());
            }
            self.log.borrow_mut().push(format!("delete {name}"));
            self.values.borrow_mut().remove(&k(hive, key, name)).map(|_| ()).ok_or_else(|| "not found".to_string())
        }

        fn create_volatile(&self, hive: Hive, key: &str) -> Result<bool, String> {
            let entry = (hive == Hive::Hklm, key.to_lowercase());
            let mut v = self.volatile.borrow_mut();
            if v.contains(&entry) {
                return Ok(false);
            }
            v.push(entry);
            Ok(true)
        }
    }
}
