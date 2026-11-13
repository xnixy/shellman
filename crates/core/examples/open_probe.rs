//! Opening-speed helper (see docs/PLAN.md, "Opening speed"). Read-only towards other processes.
//!
//! - `open_probe fleet`: every running `shellman-*` process: memory, page faults, priority,
//!   memory priority and power throttling (efficiency mode).
//! - `open_probe signal <event name>`: print the precise time, then set the named event (how
//!   the top bar summons the launcher or control centre); for lining up with `timing-*.log`.
//! - `open_probe adapters`: the graphics adapters.
//! - `open_probe now`: the precise time, as the timing logs write it.

use windows::core::HSTRING;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX};
use windows::Win32::System::Threading::{
    GetPriorityClass, GetProcessInformation, GetProcessTimes, OpenEventW, OpenProcess, SetEvent,
    ProcessMemoryPriority, ProcessPowerThrottling, EVENT_MODIFY_STATE, MEMORY_PRIORITY_INFORMATION,
    PROCESS_POWER_THROTTLING_STATE, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("fleet") => fleet(),
        Some("signal") => signal(args.get(1).expect("event name")),
        Some("adapters") => adapters(),
        Some("now") => println!("{}", shellman_core::timing::now()),
        Some("gl") => gl(),
        Some("d3d") => d3d(),
        _ => eprintln!("usage: open_probe fleet | signal <event> | adapters | now | gl | d3d"),
    }
}

/// Milliseconds since this process was created (the loader counts).
fn since_start() -> f64 {
    let (mut c, mut e, mut k, mut u) = Default::default();
    unsafe {
        let _ = GetProcessTimes(windows::Win32::System::Threading::GetCurrentProcess(), &mut c, &mut e, &mut k, &mut u);
    }
    let created = (c.dwHighDateTime as u64) << 32 | c.dwLowDateTime as u64;
    (shellman_core::timing::now() - created) as f64 / 10_000.0
}

fn step<T>(what: &str, f: impl FnOnce() -> T) -> T {
    let t = std::time::Instant::now();
    let r = f();
    println!("{what:<34} {:7.1} ms   (at +{:.1})", t.elapsed().as_secs_f64() * 1000.0, since_start());
    r
}

/// What an OpenGL window costs on its own, step by step (as eframe's glow path does it, minus
/// egui): a window, a pixel format, the driver's context, making it current, the first string.
fn gl() {
    use windows::core::w;
    use windows::Win32::Graphics::Gdi::GetDC;
    use windows::Win32::Graphics::OpenGL::*;
    use windows::Win32::System::LibraryLoader::{GetModuleHandleW, LoadLibraryW};
    use windows::Win32::UI::WindowsAndMessaging::*;
    println!("process start -> main            {:7.1} ms", since_start());
    unsafe {
        step("LoadLibrary opengl32", || LoadLibraryW(w!("opengl32.dll")).unwrap());
        let hwnd = step("CreateWindow", || {
            let inst = GetModuleHandleW(None).unwrap();
            let class = WNDCLASSW { lpfnWndProc: Some(def_proc), hInstance: inst.into(), lpszClassName: w!("open_probe_gl"), ..Default::default() };
            RegisterClassW(&class);
            CreateWindowExW(Default::default(), w!("open_probe_gl"), w!("gl"), WS_OVERLAPPEDWINDOW, 0, 0, 64, 64, None, None, Some(inst.into()), None).unwrap()
        });
        let dc = GetDC(Some(hwnd));
        step("ChoosePixelFormat+Set", || {
            let pfd = PIXELFORMATDESCRIPTOR {
                nSize: size_of::<PIXELFORMATDESCRIPTOR>() as u16,
                nVersion: 1,
                dwFlags: PFD_DRAW_TO_WINDOW | PFD_SUPPORT_OPENGL | PFD_DOUBLEBUFFER,
                iPixelType: PFD_TYPE_RGBA,
                cColorBits: 32,
                cAlphaBits: 8,
                ..Default::default()
            };
            let f = ChoosePixelFormat(dc, &pfd);
            let _ = SetPixelFormat(dc, f, &pfd);
        });
        let ctx = step("wglCreateContext (driver load)", || wglCreateContext(dc).unwrap());
        step("wglMakeCurrent", || wglMakeCurrent(dc, ctx).unwrap());
        step("glGetString(RENDERER)", || {
            let s = glGetString(GL_RENDERER);
            if !s.is_null() {
                println!("  renderer: {}", std::ffi::CStr::from_ptr(s.cast()).to_string_lossy());
            }
        });
        step("glClear + SwapBuffers", || {
            glClear(GL_COLOR_BUFFER_BIT);
            let _ = windows::Win32::Graphics::OpenGL::SwapBuffers(dc);
            glFinish();
        });
        let _ = wglMakeCurrent(dc, HGLRC::default());
        let _ = wglDeleteContext(ctx);
        let _ = DestroyWindow(hwnd);
    }
}

unsafe extern "system" fn def_proc(h: windows::Win32::Foundation::HWND, m: u32, w: windows::Win32::Foundation::WPARAM, l: windows::Win32::Foundation::LPARAM) -> windows::Win32::Foundation::LRESULT {
    windows::Win32::UI::WindowsAndMessaging::DefWindowProcW(h, m, w, l)
}

/// What a Direct3D 11 device costs (what most other apps draw with), for comparison with `gl`.
fn d3d() {
    use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
    use windows::Win32::Graphics::Direct3D11::*;
    println!("process start -> main            {:7.1} ms", since_start());
    unsafe {
        let mut dev = None;
        let mut ctx = None;
        step("D3D11CreateDevice", || {
            let _ = D3D11CreateDevice(None, D3D_DRIVER_TYPE_HARDWARE, Default::default(), D3D11_CREATE_DEVICE_BGRA_SUPPORT, None, D3D11_SDK_VERSION, Some(&mut dev), None, Some(&mut ctx));
        });
        if let Some(ctx) = ctx {
            step("Flush", || ctx.Flush());
        }
    }
}

fn signal(name: &str) {
    unsafe {
        let event = OpenEventW(EVENT_MODIFY_STATE, false, &HSTRING::from(name)).expect("no such event");
        let t = shellman_core::timing::now();
        let _ = SetEvent(event);
        println!("{t}");
        let _ = CloseHandle(event);
    }
}

fn fleet() {
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).unwrap();
        let mut e = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut ok = Process32FirstW(snap, &mut e).is_ok();
        println!("pid\tname\tws_MB\tprivate_MB\tfaults\tcpu_s\tprio\tmemprio\tthrottle(ctl/state)");
        while ok {
            let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(0);
            let name = String::from_utf16_lossy(&e.szExeFile[..len]);
            if name.to_lowercase().starts_with("shellman-") {
                row(e.th32ProcessID, &name);
            }
            ok = Process32NextW(snap, &mut e).is_ok();
        }
        let _ = CloseHandle(snap);
    }
}

unsafe fn row(pid: u32, name: &str) {
    let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, false, pid)
        .or_else(|_| OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid))
    else {
        println!("{pid}\t{name}\t(no access)");
        return;
    };
    let mut m = PROCESS_MEMORY_COUNTERS_EX { cb: size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32, ..Default::default() };
    let _ = GetProcessMemoryInfo(h, (&raw mut m).cast(), m.cb);
    let (mut c, mut x, mut k, mut u) = Default::default();
    let _ = GetProcessTimes(h, &mut c, &mut x, &mut k, &mut u);
    let ft = |f: windows::Win32::Foundation::FILETIME| ((f.dwHighDateTime as u64) << 32 | f.dwLowDateTime as u64) as f64 / 1e7;
    let prio = GetPriorityClass(h);
    let mut mp = MEMORY_PRIORITY_INFORMATION::default();
    let mp_ok = GetProcessInformation(h, ProcessMemoryPriority, (&raw mut mp).cast(), size_of::<MEMORY_PRIORITY_INFORMATION>() as u32).is_ok();
    let mut pt = PROCESS_POWER_THROTTLING_STATE { Version: 1, ..Default::default() };
    let pt_ok = GetProcessInformation(h, ProcessPowerThrottling, (&raw mut pt).cast(), size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32).is_ok();
    println!(
        "{pid}\t{name}\t{:.1}\t{:.1}\t{}\t{:.2}\t0x{:x}\t{}\t{}",
        m.WorkingSetSize as f64 / 1048576.0,
        m.PrivateUsage as f64 / 1048576.0,
        m.PageFaultCount,
        ft(k) + ft(u),
        prio,
        if mp_ok { mp.MemoryPriority.0.to_string() } else { "?".into() },
        if pt_ok { format!("{:x}/{:x}", pt.ControlMask, pt.StateMask) } else { "?".into() },
    );
    let _ = CloseHandle(h);
}

fn adapters() {
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1};
    unsafe {
        let f: IDXGIFactory1 = CreateDXGIFactory1().unwrap();
        let mut i = 0;
        while let Ok(a) = f.EnumAdapters1(i) {
            let d = a.GetDesc1().unwrap();
            let len = d.Description.iter().position(|&c| c == 0).unwrap_or(0);
            println!(
                "{i}: {} vram={}MB flags={:x}",
                String::from_utf16_lossy(&d.Description[..len]),
                d.DedicatedVideoMemory / 1048576,
                d.Flags
            );
            i += 1;
        }
    }
}
