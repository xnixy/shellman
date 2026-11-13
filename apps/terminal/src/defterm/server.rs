//! The COM side of a console handoff: COM starts `shellman-terminal.exe -Embedding` for our class,
//! this registers the class factory, and OpenConsole calls
//! `ITerminalHandoff3::EstablishPtyHandoff` on the object it makes.
//!
//! Handle rules (COM `system_handle` marshaling, as Windows Terminal's `ConptyConnection`
//! treats them): the `[in]` handles belong to the call and are closed when it returns, so each is
//! duplicated first; the `[out]` pipe ends we hand back are given away (COM duplicates them into
//! OpenConsole and closes ours). Every handle is checked for being what it claims (a pipe, a
//! process) before anything is kept.
//!
//! One handoff per process (`REGCLS_SINGLEUSE`): each console program gets a window of its own,
//! as Windows Terminal's default "new window", and the process ends with its window.

use super::CLSID;
use crate::pty::Handles;
use std::ffi::c_void;
use std::os::windows::io::{FromRawHandle, OwnedHandle};
use std::sync::mpsc;
use std::sync::Mutex;
use windows::core::{implement, interface, IUnknown, IUnknown_Vtbl, Interface, GUID, HRESULT};
use windows::Win32::Foundation::{
    DuplicateHandle, CLASS_E_NOAGGREGATION, DUPLICATE_SAME_ACCESS, E_INVALIDARG, E_POINTER, HANDLE, S_OK,
};
use windows::Win32::Storage::FileSystem::{GetFileType, FILE_TYPE_PIPE};
use windows::Win32::System::Com::{
    CoInitializeEx, CoRegisterClassObject, IClassFactory, IClassFactory_Impl, CLSCTX_LOCAL_SERVER, COINIT_MULTITHREADED,
    REGCLS_SINGLEUSE,
};
use windows::Win32::System::Pipes::CreatePipe;
use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessId, QueryFullProcessImageNameW, PROCESS_NAME_WIN32};
use windows_core::{Ref, BOOL};

/// `TERMINAL_STARTUP_INFO` (`ITerminalHandoff.idl`). The strings are BSTRs owned by the call.
#[repr(C)]
pub struct StartupInfo {
    pub title: *const u16,
    pub icon_path: *const u16,
    pub icon_index: i32,
    pub x: u32,
    pub y: u32,
    pub x_size: u32,
    pub y_size: u32,
    pub x_count_chars: u32,
    pub y_count_chars: u32,
    pub fill_attribute: u32,
    pub flags: u32,
    pub show_window: u16,
}

// The IDL's method, with its seven arguments.
#[allow(non_snake_case, clippy::too_many_arguments)]
mod iface {
    use super::*;
    #[interface("6F23DA90-15C5-4203-9DB0-64E73F1B1B00")]
    pub unsafe trait ITerminalHandoff3: IUnknown {
        fn EstablishPtyHandoff(
            &self,
            input: *mut HANDLE,
            output: *mut HANDLE,
            signal: HANDLE,
            reference: HANDLE,
            server: HANDLE,
            client: HANDLE,
            startup: *const StartupInfo,
        ) -> HRESULT;
    }
}
pub use iface::{ITerminalHandoff3, ITerminalHandoff3_Impl};

/// What arrived: the console, and how its program asked to be shown.
pub struct Arrived {
    pub handles: Handles,
    /// The window title it was started with (a shortcut's name), if any.
    pub title: Option<String>,
    /// The client's exe name ("cmd").
    pub program: String,
    /// `wShowWindow`, when `STARTF_USESHOWWINDOW` was set.
    pub show: Option<u16>,
    /// Where the window was asked to be (screen pixels), when `STARTF_USEPOSITION` was set.
    pub position: Option<(i32, i32)>,
}

impl Arrived {
    /// Started hidden (SW_HIDE): it gets no window.
    pub fn hidden(&self) -> bool {
        self.show == Some(0)
    }
    /// Started minimized (SW_SHOWMINIMIZED, SW_MINIMIZE, SW_SHOWMINNOACTIVE, SW_FORCEMINIMIZE).
    pub fn minimized(&self) -> bool {
        matches!(self.show, Some(2 | 6 | 7 | 11))
    }
}

/// `wShowWindow` counts only with STARTF_USESHOWWINDOW.
pub fn show_flag(flags: u32, show: u16) -> Option<u16> {
    (flags & 1 != 0).then_some(show)
}

#[implement(ITerminalHandoff3)]
struct Handoff {
    tx: Mutex<Option<mpsc::Sender<Arrived>>>,
}

#[allow(clippy::too_many_arguments)]
impl ITerminalHandoff3_Impl for Handoff_Impl {
    unsafe fn EstablishPtyHandoff(
        &self,
        input: *mut HANDLE,
        output: *mut HANDLE,
        signal: HANDLE,
        reference: HANDLE,
        server: HANDLE,
        client: HANDLE,
        startup: *const StartupInfo,
    ) -> HRESULT {
        let Some(tx) = self.tx.lock().unwrap().take() else { return E_INVALIDARG };
        match unsafe { accept(input, output, signal, reference, server, client, startup) } {
            Ok(arrived) => {
                let _ = tx.send(arrived);
                S_OK
            }
            Err(e) => e,
        }
    }
}

/// Take the handed-over console: check and duplicate the handles, make the input and output
/// pipes, and fill in `input` / `output` with OpenConsole's ends.
///
/// # Safety
/// `input` / `output` must be writable when not null; `startup` readable when not null (COM's
/// contract for this call).
#[allow(clippy::too_many_arguments)]
pub unsafe fn accept(
    input: *mut HANDLE,
    output: *mut HANDLE,
    signal: HANDLE,
    reference: HANDLE,
    server: HANDLE,
    client: HANDLE,
    startup: *const StartupInfo,
) -> Result<Arrived, HRESULT> {
    if input.is_null() || output.is_null() {
        return Err(E_POINTER);
    }
    let usable = |h: HANDLE| !h.is_invalid() && !h.0.is_null();
    if ![signal, reference, server, client].into_iter().all(usable) {
        return Err(E_INVALIDARG);
    }
    unsafe {
        if GetFileType(signal) != FILE_TYPE_PIPE || GetProcessId(server) == 0 || GetProcessId(client) == 0 {
            return Err(E_INVALIDARG);
        }
    }
    let signal = dup(signal)?;
    let reference = dup(reference)?;
    let server = dup(server)?;
    let client = dup(client)?;
    // Input: OpenConsole reads `in_read`, we write `in_write`. Output: the other way round.
    // Both ends stay owned until OpenConsole's are given away below, so a failure on the way
    // closes them all.
    let (in_read, in_write) = pipe()?;
    let (out_read, out_write) = pipe()?;
    let (title, show, position) = match unsafe { startup.as_ref() } {
        Some(s) => (unsafe { bstr(s.title) }, show_flag(s.flags, s.show_window), (s.flags & 4 != 0).then_some((s.x as i32, s.y as i32))),
        None => (None, None, None),
    };
    let program = image_name(&client).unwrap_or_else(|| "console".into());
    let into_raw = |h: OwnedHandle| HANDLE(std::os::windows::io::IntoRawHandle::into_raw_handle(h));
    unsafe {
        *input = into_raw(in_read);
        *output = into_raw(out_write);
    }
    Ok(Arrived {
        handles: Handles {
            input: in_write,
            output: out_read,
            signal,
            reference,
            server,
            client,
        },
        title: title.filter(|t| !t.trim().is_empty()),
        program,
        show,
        position,
    })
}

fn dup(h: HANDLE) -> Result<OwnedHandle, HRESULT> {
    let mut out = HANDLE::default();
    unsafe {
        let me = GetCurrentProcess();
        DuplicateHandle(me, h, me, &mut out, 0, false, DUPLICATE_SAME_ACCESS).map_err(|e| e.code())?;
        Ok(OwnedHandle::from_raw_handle(out.0))
    }
}

/// An anonymous pipe: its read end and write end, both owned (closed if dropped).
fn pipe() -> Result<(OwnedHandle, OwnedHandle), HRESULT> {
    let (mut r, mut w) = (HANDLE::default(), HANDLE::default());
    unsafe { CreatePipe(&mut r, &mut w, None, 128 * 1024) }.map_err(|e| e.code())?;
    Ok(unsafe { (OwnedHandle::from_raw_handle(r.0), OwnedHandle::from_raw_handle(w.0)) })
}

/// A BSTR's text (its length is the u32 before it, in bytes), at most 4096 characters.
unsafe fn bstr(p: *const u16) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let bytes = unsafe { *(p as *const u32).sub(1) } as usize;
    let len = (bytes / 2).min(4096);
    Some(String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(p, len) }))
}

/// "cmd" for `C:\Windows\System32\cmd.exe`.
fn image_name(process: &OwnedHandle) -> Option<String> {
    use std::os::windows::io::AsRawHandle;
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    unsafe {
        QueryFullProcessImageNameW(HANDLE(process.as_raw_handle()), PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut len).ok()?;
    }
    Some(crate::session::program_name(&format!("\"{}\"", String::from_utf16_lossy(&buf[..len as usize]))))
}

#[implement(IClassFactory)]
struct Factory {
    tx: Mutex<Option<mpsc::Sender<Arrived>>>,
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
impl IClassFactory_Impl for Factory_Impl {
    fn CreateInstance(&self, outer: Ref<'_, IUnknown>, iid: *const GUID, out: *mut *mut c_void) -> windows::core::Result<()> {
        if out.is_null() || iid.is_null() {
            return Err(E_POINTER.into());
        }
        unsafe { *out = std::ptr::null_mut() };
        if outer.is_some() {
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        let tx = self.tx.lock().unwrap().clone();
        let object: IUnknown = Handoff { tx: Mutex::new(tx) }.into();
        unsafe { object.query(iid, out).ok() }
    }

    fn LockServer(&self, _lock: BOOL) -> windows::core::Result<()> {
        Ok(())
    }
}

/// Register the class factory on a COM thread of its own (multithreaded, so calls come straight
/// in on COM's threads) and keep it for the life of the process. Handoffs arrive on the
/// returned receiver.
pub fn serve() -> Result<mpsc::Receiver<Arrived>, String> {
    let (tx, rx) = mpsc::channel();
    let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
    std::thread::Builder::new()
        .name("com-handoff".into())
        .spawn(move || {
            let registered = unsafe {
                CoInitializeEx(None, COINIT_MULTITHREADED)
                    .ok()
                    .and_then(|_| {
                        let factory: IClassFactory = Factory { tx: Mutex::new(Some(tx)) }.into();
                        let clsid = GUID::try_from(&CLSID[1..37]).map_err(|_| windows::core::Error::from(E_INVALIDARG))?;
                        CoRegisterClassObject(&clsid, &factory, CLSCTX_LOCAL_SERVER, REGCLS_SINGLEUSE)
                    })
                    .map(|_| ())
                    .map_err(|e| format!("could not register the handoff class: {e}"))
            };
            let ok = registered.is_ok();
            let _ = ready_tx.send(registered);
            // The class stays registered, and this apartment alive, until the process ends.
            if ok {
                loop {
                    std::thread::park();
                }
            }
        })
        .map_err(|e| e.to_string())?;
    ready_rx.recv().map_err(|e| e.to_string())??;
    Ok(rx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::io::AsRawHandle;

    #[test]
    fn show_window_counts_only_with_its_flag() {
        assert_eq!(show_flag(0, 0), None);
        assert_eq!(show_flag(1, 0), Some(0));
        assert_eq!(show_flag(0x101, 7), Some(7));
    }

    #[test]
    fn hidden_and_minimized() {
        let a = |show| Arrived {
            handles: fake_handles(),
            title: None,
            program: String::new(),
            show,
            position: None,
        };
        assert!(a(Some(0)).hidden() && !a(None).hidden() && !a(Some(1)).hidden());
        assert!(a(Some(7)).minimized() && a(Some(2)).minimized() && !a(Some(5)).minimized());
    }

    fn fake_handles() -> Handles {
        let (r, w) = pipe().unwrap();
        Handles {
            input: w.try_clone().unwrap(),
            output: r.try_clone().unwrap(),
            signal: w.try_clone().unwrap(),
            reference: r.try_clone().unwrap(),
            server: w,
            client: r,
        }
    }

    /// The handles a call brings are checked: a process where a process is due, a pipe for the
    /// signal; good ones are duplicated and the pipes made.
    #[test]
    fn handed_handles_are_checked_and_kept() {
        let (sig_r, sig_w) = pipe().unwrap();
        let me_owned = dup(unsafe { GetCurrentProcess() }).unwrap();
        let me = HANDLE(me_owned.as_raw_handle());
        let (mut i, mut o) = (HANDLE::default(), HANDLE::default());
        let raw = |h: &OwnedHandle| HANDLE(h.as_raw_handle());
        let info = StartupInfo {
            title: std::ptr::null(),
            icon_path: std::ptr::null(),
            icon_index: 0,
            x: 0,
            y: 0,
            x_size: 0,
            y_size: 0,
            x_count_chars: 0,
            y_count_chars: 0,
            fill_attribute: 0,
            flags: 1,
            show_window: 7,
        };
        let got = unsafe { accept(&mut i, &mut o, raw(&sig_w), raw(&sig_r), me, me, &info) }.unwrap();
        assert!(!i.is_invalid() && !o.is_invalid());
        assert!(got.minimized());
        assert!(got.program.to_ascii_lowercase().starts_with("shellman_terminal") || !got.program.is_empty());
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(i);
            let _ = windows::Win32::Foundation::CloseHandle(o);
        }
        // A pipe is not a process; a process is not a pipe; nulls are refused.
        assert_eq!(unsafe { accept(&mut i, &mut o, raw(&sig_w), raw(&sig_r), raw(&sig_w), me, &info) }.err(), Some(E_INVALIDARG));
        assert_eq!(unsafe { accept(&mut i, &mut o, me, raw(&sig_r), me, me, &info) }.err(), Some(E_INVALIDARG));
        assert_eq!(unsafe { accept(&mut i, &mut o, raw(&sig_w), HANDLE::default(), me, me, &info) }.err(), Some(E_INVALIDARG));
        assert_eq!(unsafe { accept(std::ptr::null_mut(), &mut o, raw(&sig_w), raw(&sig_r), me, me, &info) }.err(), Some(E_POINTER));
    }
}
