//! The Service Control Manager, read-only: `OpenSCManagerW` with enumerate rights,
//! `EnumServicesStatusExW` for names, states and PIDs, and per service `QueryServiceConfigW` and
//! `QueryServiceConfig2W` (delayed start, description) through a handle opened with query-config
//! rights only. A service whose config is refused still appears, with what the list gave.

use super::reader::Cache;
use super::{Service, Startup, State};
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::ERROR_MORE_DATA;
use windows::Win32::System::Services::{
    CloseServiceHandle, EnumServicesStatusExW, OpenSCManagerW, OpenServiceW, QueryServiceConfig2W, QueryServiceConfigW,
    ENUM_SERVICE_STATUS_PROCESSW, QUERY_SERVICE_CONFIGW, SC_ENUM_PROCESS_INFO, SC_HANDLE, SC_MANAGER_ENUMERATE_SERVICE,
    SERVICE_CONFIG_DELAYED_AUTO_START_INFO, SERVICE_CONFIG_DESCRIPTION, SERVICE_DELAYED_AUTO_START_INFO,
    SERVICE_DESCRIPTIONW, SERVICE_QUERY_CONFIG, SERVICE_STATE_ALL, SERVICE_WIN32,
};
use windows::Win32::UI::Shell::SHLoadIndirectString;

/// An SCM or service handle, closed when dropped.
struct Handle(SC_HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseServiceHandle(self.0);
        }
    }
}

/// A buffer the Win32 structures can be read from in place: 8-byte aligned.
struct Buffer(Vec<u64>);

impl Buffer {
    fn new(bytes: usize) -> Self {
        Buffer(vec![0; bytes.div_ceil(8).max(1)])
    }

    fn bytes(&mut self) -> &mut [u8] {
        let len = self.0.len() * 8;
        // SAFETY: a u64 slice is valid as bytes, of the same total size.
        unsafe { std::slice::from_raw_parts_mut(self.0.as_mut_ptr().cast::<u8>(), len) }
    }
}

/// Every Win32 service, in no particular order. Empty when the SCM cannot be opened.
pub fn read(cache: &mut Cache) -> Vec<Service> {
    let Ok(scm) = (unsafe { OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_ENUMERATE_SERVICE) }) else {
        return Vec::new();
    };
    let scm = Handle(scm);
    let mut services = enumerate(&scm);
    for s in &mut services {
        configure(&scm, s, cache);
    }
    services
}

fn enumerate(scm: &Handle) -> Vec<Service> {
    let mut out = Vec::new();
    let mut resume = 0u32;
    let mut buffer = Buffer::new(64 * 1024);
    loop {
        let (mut needed, mut returned) = (0u32, 0u32);
        let result = unsafe {
            EnumServicesStatusExW(
                scm.0,
                SC_ENUM_PROCESS_INFO,
                SERVICE_WIN32,
                SERVICE_STATE_ALL,
                Some(buffer.bytes()),
                &mut needed,
                &mut returned,
                Some(&mut resume),
                PCWSTR::null(),
            )
        };
        let more = matches!(&result, Err(e) if e.code() == ERROR_MORE_DATA.to_hresult());
        if result.is_err() && !more {
            break;
        }
        let entries = buffer.bytes().as_ptr().cast::<ENUM_SERVICE_STATUS_PROCESSW>();
        for i in 0..returned as usize {
            // SAFETY: the call wrote `returned` entries at the start of the buffer, aligned.
            let e = unsafe { &*entries.add(i) };
            let status = &e.ServiceStatusProcess;
            out.push(Service {
                name: wide(e.lpServiceName),
                display: wide(e.lpDisplayName),
                state: State::from_raw(status.dwCurrentState.0),
                pid: if State::from_raw(status.dwCurrentState.0) == State::Stopped { 0 } else { status.dwProcessId },
                ..Default::default()
            });
        }
        if !more {
            break;
        }
        // The rest did not fit; a bigger buffer, from where it stopped (`resume`).
        if needed as usize > buffer.0.len() * 8 {
            buffer = Buffer::new(needed as usize);
        }
    }
    out
}

/// The startup type and the description, from the service's own config. Quietly leaves what it
/// cannot read.
fn configure(scm: &Handle, s: &mut Service, cache: &mut Cache) {
    let name: Vec<u16> = s.name.encode_utf16().chain(Some(0)).collect();
    let Ok(h) = (unsafe { OpenServiceW(scm.0, PCWSTR(name.as_ptr()), SERVICE_QUERY_CONFIG) }) else {
        s.description = cache.get(&s.name).cloned().flatten();
        return;
    };
    let h = Handle(h);
    if let Some(raw) = start_type(&h) {
        s.startup = Startup::from_raw(raw, raw == 2 && delayed(&h));
    }
    s.description = match cache.get(&s.name) {
        Some(d) => d.clone(),
        None => {
            let d = description(&h);
            cache.insert(s.name.clone(), d.clone());
            d
        }
    };
}

fn start_type(h: &Handle) -> Option<u32> {
    let mut needed = 0u32;
    // The first call fails by design and says how big the answer is.
    let _ = unsafe { QueryServiceConfigW(h.0, None, 0, &mut needed) };
    if needed == 0 {
        return None;
    }
    let mut buffer = Buffer::new(needed as usize);
    let config = buffer.bytes().as_mut_ptr().cast::<QUERY_SERVICE_CONFIGW>();
    unsafe { QueryServiceConfigW(h.0, Some(config), needed, &mut needed) }.ok()?;
    // SAFETY: the call filled the structure at the start of the buffer.
    Some(unsafe { (*config).dwStartType.0 })
}

fn delayed(h: &Handle) -> bool {
    let mut buffer = Buffer::new(std::mem::size_of::<SERVICE_DELAYED_AUTO_START_INFO>());
    let mut needed = 0u32;
    let ok = unsafe { QueryServiceConfig2W(h.0, SERVICE_CONFIG_DELAYED_AUTO_START_INFO, Some(buffer.bytes()), &mut needed) };
    // SAFETY: filled by the call when it succeeded.
    ok.is_ok() && unsafe { (*buffer.bytes().as_ptr().cast::<SERVICE_DELAYED_AUTO_START_INFO>()).fDelayedAutostart.as_bool() }
}

fn description(h: &Handle) -> Option<String> {
    let mut needed = 0u32;
    let _ = unsafe { QueryServiceConfig2W(h.0, SERVICE_CONFIG_DESCRIPTION, None, &mut needed) };
    if needed == 0 {
        return None;
    }
    let mut buffer = Buffer::new(needed as usize);
    unsafe { QueryServiceConfig2W(h.0, SERVICE_CONFIG_DESCRIPTION, Some(buffer.bytes()), &mut needed) }.ok()?;
    // SAFETY: filled by the call; the string it points to is inside the same buffer.
    let text = unsafe { (*buffer.bytes().as_ptr().cast::<SERVICE_DESCRIPTIONW>()).lpDescription };
    if text.is_null() {
        return None;
    }
    let text = resolve(wide(text));
    (!text.trim().is_empty()).then(|| text.trim().to_owned())
}

/// A description may be a resource reference (`@%SystemRoot%\system32\x.dll,-100`); this is what
/// it says. The reference itself when it cannot be loaded.
fn resolve(text: String) -> String {
    if !text.starts_with('@') {
        return text;
    }
    let source: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
    let mut out = [0u16; 1024];
    match unsafe { SHLoadIndirectString(PCWSTR(source.as_ptr()), &mut out, None) } {
        Ok(()) => {
            let end = out.iter().position(|&c| c == 0).unwrap_or(out.len());
            String::from_utf16_lossy(&out[..end])
        }
        Err(_) => String::new(),
    }
}

fn wide(p: PWSTR) -> String {
    if p.is_null() {
        return String::new();
    }
    unsafe { p.to_string().unwrap_or_default() }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads this machine's real services: every Windows has the Event Log and the Service
    /// Control Manager's neighbours, and the read needs no rights.
    #[test]
    fn reads_the_real_services_without_elevation() {
        let mut cache = Cache::new();
        let all = read(&mut cache);
        assert!(all.len() > 20, "{} services", all.len());
        let rpc = all.iter().find(|s| s.name.eq_ignore_ascii_case("RpcSs")).expect("RpcSs");
        assert_eq!(rpc.state, State::Running);
        assert!(rpc.pid != 0 && rpc.startup == Some(Startup::Automatic));
        assert!(!rpc.display.is_empty());
        assert!(all.iter().all(|s| !s.name.is_empty()));
        assert!(all.iter().filter(|s| s.description.is_some()).count() > 10, "descriptions are read and resolved");
        assert!(all.iter().all(|s| s.description.as_deref().is_none_or(|d| !d.starts_with('@'))), "none left as a reference");
        assert!(all.iter().any(|s| s.startup == Some(Startup::Disabled)));
    }
}
