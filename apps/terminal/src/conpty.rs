//! Making a pseudo-console and starting a program in it.
//!
//! Windows' own ConPTY (the inbox `conhost.exe`) is slow to pass output through: a 100 MB `cat`
//! in WSL took 9.5 s in it against 2.5 s through Microsoft's newer one, which ships as
//! `conpty.dll` + `OpenConsole.exe` (NuGet `Microsoft.Windows.Console.ConPTY`, MIT, the same code
//! Windows Terminal runs; see `third_party/conpty`). When both sit beside this exe, pseudo-consoles
//! are made through that `conpty.dll` (`ConptyCreatePseudoConsole` and friends, which start the
//! `OpenConsole.exe` beside it); otherwise through Windows' API. The dll is loaded only from this
//! exe's folder, never from PATH.

use crate::pty::{Console, Ending, Pty};
use alacritty_terminal::event::WindowSize;
use std::io;
use std::os::windows::io::{FromRawHandle, OwnedHandle};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use windows::core::{HRESULT, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HMODULE};
use windows::Win32::System::Console::{COORD, HPCON};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryExW, LOAD_WITH_ALTERED_SEARCH_PATH};

type CreateFn = unsafe extern "system" fn(COORD, HANDLE, HANDLE, u32, *mut HPCON) -> HRESULT;
type ResizeFn = unsafe extern "system" fn(HPCON, COORD) -> HRESULT;
type CloseFn = unsafe extern "system" fn(HPCON);
type ReleaseFn = unsafe extern "system" fn(HPCON) -> HRESULT;

/// `conpty.h`: measure glyph widths per code point as `wcswidth` does, which is how the emulator
/// (`alacritty_terminal`, by `unicode-width`) places them, so both agree where the cursor is
/// after an emoji. Only the bundled ConPTY knows the flag.
pub const PSEUDOCONSOLE_GLYPH_WIDTH_WCSWIDTH: u32 = 0x10;

/// One pseudo-console API: the bundled `conpty.dll`'s or Windows'.
pub struct Api {
    /// The bundled `conpty.dll` (and so `OpenConsole.exe`).
    pub bundled: bool,
    create: CreateFn,
    resize: ResizeFn,
    close: CloseFn,
    /// Lets the console host exit after its last program (newer Windows and the bundled dll).
    release: Option<ReleaseFn>,
}

impl Api {
    pub fn resize(&self, hpc: HPCON, cols: u16, rows: u16) {
        unsafe {
            let _ = (self.resize)(hpc, coord(cols, rows));
        }
    }

    pub fn close(&self, hpc: HPCON) {
        unsafe { (self.close)(hpc) }
    }
}

/// The API pseudo-consoles are made with, found once.
pub fn api() -> &'static Api {
    static API: OnceLock<Api> = OnceLock::new();
    API.get_or_init(|| {
        let beside = std::env::current_exe().ok().and_then(|e| e.parent().and_then(bundled_dll));
        beside.and_then(|dll| load_bundled(&dll)).unwrap_or_else(inbox)
    })
}

/// `conpty.dll` in `dir`, when the `OpenConsole.exe` it starts is there too (beside it, or in the
/// architecture's subfolder, as `conpty.dll` itself looks).
pub fn bundled_dll(dir: &Path) -> Option<PathBuf> {
    let dll = dir.join("conpty.dll");
    let host = dir.join("OpenConsole.exe").is_file() || dir.join(arch_folder()).join("OpenConsole.exe").is_file();
    (dll.is_file() && host).then_some(dll)
}

fn arch_folder() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "arm64"
    } else if cfg!(target_arch = "x86") {
        "x86"
    } else {
        "x64"
    }
}

fn load_bundled(dll: &Path) -> Option<Api> {
    let wide = wide(dll.as_os_str());
    // The full path, and its own folder for what it loads: never a conpty.dll from PATH.
    let module = unsafe { LoadLibraryExW(PCWSTR(wide.as_ptr()), None, LOAD_WITH_ALTERED_SEARCH_PATH) }.ok()?;
    unsafe {
        Some(Api {
            bundled: true,
            create: std::mem::transmute::<_, CreateFn>(proc(module, b"ConptyCreatePseudoConsole\0")?),
            resize: std::mem::transmute::<_, ResizeFn>(proc(module, b"ConptyResizePseudoConsole\0")?),
            close: std::mem::transmute::<_, CloseFn>(proc(module, b"ConptyClosePseudoConsole\0")?),
            release: proc(module, b"ConptyReleasePseudoConsole\0").map(|p| std::mem::transmute::<_, ReleaseFn>(p)),
        })
    }
}

fn inbox() -> Api {
    use windows::Win32::System::Console::{ClosePseudoConsole, CreatePseudoConsole, ResizePseudoConsole};
    unsafe extern "system" fn create(size: COORD, input: HANDLE, output: HANDLE, flags: u32, out: *mut HPCON) -> HRESULT {
        match CreatePseudoConsole(size, input, output, flags) {
            Ok(h) => {
                *out = h;
                HRESULT(0)
            }
            Err(e) => e.code(),
        }
    }
    unsafe extern "system" fn resize(hpc: HPCON, size: COORD) -> HRESULT {
        match ResizePseudoConsole(hpc, size) {
            Ok(()) => HRESULT(0),
            Err(e) => e.code(),
        }
    }
    unsafe extern "system" fn close(hpc: HPCON) {
        ClosePseudoConsole(hpc)
    }
    // ReleasePseudoConsole is newer than the rest (Windows 11 24H2): looked up, not linked.
    let release = unsafe {
        GetModuleHandleW(windows::core::w!("kernel32.dll"))
            .ok()
            .and_then(|k| proc(k, b"ReleasePseudoConsole\0"))
            .map(|p| std::mem::transmute::<_, ReleaseFn>(p))
    };
    Api { bundled: false, create, resize, close, release }
}

type Proc = unsafe extern "system" fn() -> isize;

unsafe fn proc(module: HMODULE, name: &[u8]) -> Option<Proc> {
    GetProcAddress(module, windows::core::PCSTR(name.as_ptr()))
}

fn coord(cols: u16, rows: u16) -> COORD {
    COORD { X: cols.clamp(1, i16::MAX as u16) as i16, Y: rows.clamp(1, i16::MAX as u16) as i16 }
}

fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    s.encode_wide().chain(std::iter::once(0)).collect()
}

/// A pipe: (read end, write end), neither inheritable.
fn pipe() -> io::Result<(OwnedHandle, OwnedHandle)> {
    let (mut r, mut w) = (HANDLE::default(), HANDLE::default());
    unsafe { windows::Win32::System::Pipes::CreatePipe(&mut r, &mut w, None, 128 * 1024) }.map_err(io::Error::other)?;
    Ok(unsafe { (OwnedHandle::from_raw_handle(r.0), OwnedHandle::from_raw_handle(w.0)) })
}

/// Make a pseudo-console of `size` and start `command` (a whole command line, as CreateProcess
/// takes it) in it, in `dir`.
pub fn spawn(command: &str, dir: Option<&Path>, size: WindowSize) -> io::Result<Pty> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::System::Threading::{
        CreateProcessW, DeleteProcThreadAttributeList, InitializeProcThreadAttributeList, UpdateProcThreadAttribute,
        EXTENDED_STARTUPINFO_PRESENT, LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION, PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE,
        STARTF_USESTDHANDLES, STARTUPINFOEXW,
    };
    let api = api();
    let (in_read, in_write) = pipe()?;
    let (out_read, out_write) = pipe()?;
    let flags = if api.bundled { PSEUDOCONSOLE_GLYPH_WIDTH_WCSWIDTH } else { 0 };
    let mut hpc = HPCON::default();
    let hr = unsafe { (api.create)(coord(size.num_cols, size.num_lines), HANDLE(in_read.as_raw_handle()), HANDLE(out_write.as_raw_handle()), flags, &mut hpc) };
    // The console has its own copies: ours must go, or its output pipe never breaks.
    drop(in_read);
    drop(out_write);
    hr.ok().map_err(io::Error::other)?;
    // From here on, a failure closes the console again.
    let close_on_error = |e: io::Error| {
        api.close(hpc);
        e
    };

    let mut attr_size = 0usize;
    unsafe {
        let _ = InitializeProcThreadAttributeList(None, 1, None, &mut attr_size);
    }
    let mut attr_buf = vec![0u8; attr_size.max(1)];
    let attrs = LPPROC_THREAD_ATTRIBUTE_LIST(attr_buf.as_mut_ptr().cast());
    unsafe { InitializeProcThreadAttributeList(Some(attrs), 1, None, &mut attr_size) }.map_err(io::Error::other).map_err(close_on_error)?;
    let result = (|| -> io::Result<PROCESS_INFORMATION> {
        unsafe {
            UpdateProcThreadAttribute(
                attrs,
                0,
                PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
                Some(hpc.0 as *const std::ffi::c_void),
                std::mem::size_of::<HPCON>(),
                None,
                None,
            )
            .map_err(io::Error::other)?;
        }
        let mut si = STARTUPINFOEXW::default();
        si.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
        // No standard handles of ours reach the program: the console gives it its own.
        si.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        si.lpAttributeList = attrs;
        let mut cmd = wide(std::ffi::OsStr::new(command));
        let cwd = dir.map(|d| wide(d.as_os_str()));
        let mut pi = PROCESS_INFORMATION::default();
        unsafe {
            CreateProcessW(
                None,
                Some(PWSTR(cmd.as_mut_ptr())),
                None,
                None,
                false,
                EXTENDED_STARTUPINFO_PRESENT,
                None,
                cwd.as_ref().map_or(PCWSTR::null(), |c| PCWSTR(c.as_ptr())),
                &si.StartupInfo,
                &mut pi,
            )
            .map_err(io::Error::other)?;
        }
        Ok(pi)
    })();
    unsafe { DeleteProcThreadAttributeList(attrs) };
    let pi = result.map_err(close_on_error)?;
    unsafe {
        let _ = CloseHandle(pi.hThread);
    }
    let client = unsafe { OwnedHandle::from_raw_handle(pi.hProcess.0) };
    // The console host may now exit after its last program: then its output pipe breaks, which
    // is how the end of the output is known (see `pty::drain`).
    let released = api.release.is_some_and(|release| unsafe { release(hpc) }.is_ok());
    let ending = if released { Ending::PipeBreaks } else { Ending::Quiet };
    Pty::new(Console::Created { api, hpc }, out_read, in_write, client, None, ending)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bundled_conpty_needs_its_console_host_beside_it() {
        let dir = std::env::temp_dir().join(format!("shellman-conpty-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("x64")).unwrap();
        assert_eq!(bundled_dll(&dir), None);
        std::fs::write(dir.join("conpty.dll"), b"").unwrap();
        // conpty.dll alone is not enough: it would fall back to Windows' conhost anyway.
        assert_eq!(bundled_dll(&dir), None);
        std::fs::write(dir.join("OpenConsole.exe"), b"").unwrap();
        assert_eq!(bundled_dll(&dir), Some(dir.join("conpty.dll")));
        // Or in the architecture's folder, as the NuGet package lays it out.
        std::fs::remove_file(dir.join("OpenConsole.exe")).unwrap();
        if arch_folder() == "x64" {
            std::fs::write(dir.join("x64").join("OpenConsole.exe"), b"").unwrap();
            assert_eq!(bundled_dll(&dir), Some(dir.join("conpty.dll")));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sizes_stay_within_what_conpty_takes() {
        let c = coord(0, 60000);
        assert_eq!((c.X, c.Y), (1, i16::MAX));
        let c = coord(120, 30);
        assert_eq!((c.X, c.Y), (120, 30));
    }

    /// A real pseudo-console (Windows' own, under `cargo test`, as no conpty.dll is beside the
    /// test exe): a program that prints and exits at once has all its output read before its
    /// end is told, and its exit code comes through.
    #[test]
    fn a_program_that_exits_at_once_loses_no_output() {
        use alacritty_terminal::tty::{ChildEvent, EventedPty, EventedReadWrite};
        use std::io::Read;
        let size = WindowSize { num_lines: 25, num_cols: 100, cell_width: 8, cell_height: 16 };
        let mut pty = spawn("cmd.exe /d /c \"echo first& echo LASTLINE& exit 7\"", None, size).unwrap();
        let mut out = Vec::new();
        let mut buf = [0u8; 4096];
        let start = std::time::Instant::now();
        let mut code = None;
        while start.elapsed() < std::time::Duration::from_secs(10) {
            let n = pty.reader().read(&mut buf).unwrap();
            out.extend_from_slice(&buf[..n]);
            if let Some(ChildEvent::Exited(status)) = pty.next_child_event() {
                let n = pty.reader().read(&mut buf).unwrap();
                out.extend_from_slice(&buf[..n]);
                code = status.and_then(|s| s.code());
                break;
            }
            if n == 0 {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
        let text = String::from_utf8_lossy(&out);
        assert!(text.contains("LASTLINE"), "output was {text:?}");
        assert_eq!(code, Some(7));
    }
}
