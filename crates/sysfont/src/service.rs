//! Stopping and starting a Windows service, for clearing the font cache.

use std::time::{Duration, Instant};
use windows::core::HSTRING;
use windows::Win32::System::Services::{
    CloseServiceHandle, ControlService, OpenSCManagerW, OpenServiceW, QueryServiceStatus,
    StartServiceW, SC_HANDLE, SC_MANAGER_CONNECT, SERVICE_CONTROL_STOP, SERVICE_QUERY_STATUS,
    SERVICE_START, SERVICE_STATUS, SERVICE_STOP, SERVICE_STOPPED,
};

struct Handle(SC_HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseServiceHandle(self.0);
        }
    }
}

fn open(name: &str) -> Option<(Handle, Handle)> {
    unsafe {
        let manager = Handle(OpenSCManagerW(None, None, SC_MANAGER_CONNECT).ok()?);
        let service = Handle(
            OpenServiceW(
                manager.0,
                &HSTRING::from(name),
                SERVICE_STOP | SERVICE_START | SERVICE_QUERY_STATUS,
            )
            .ok()?,
        );
        Some((manager, service))
    }
}

/// Stop a running service and wait for it. True if it was running and has stopped.
pub fn stop(name: &str) -> bool {
    let Some((_manager, service)) = open(name) else {
        return false;
    };
    unsafe {
        let mut status = SERVICE_STATUS::default();
        if QueryServiceStatus(service.0, &mut status).is_err()
            || status.dwCurrentState == SERVICE_STOPPED
        {
            return false;
        }
        if ControlService(service.0, SERVICE_CONTROL_STOP, &mut status).is_err() {
            return false;
        }
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(10) {
            if QueryServiceStatus(service.0, &mut status).is_ok()
                && status.dwCurrentState == SERVICE_STOPPED
            {
                return true;
            }
            std::thread::sleep(Duration::from_millis(150));
        }
    }
    false
}

pub fn start(name: &str) {
    if let Some((_manager, service)) = open(name) {
        unsafe {
            let _ = StartServiceW(service.0, None);
        }
    }
}
