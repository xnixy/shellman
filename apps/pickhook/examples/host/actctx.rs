//! The registration-free experiment: does `CoCreateInstance(CLSID_FileOpenDialog)` go through
//! COM's ordinary class lookup, which a registration can redirect? Instead of writing the
//! registry, an activation context (a side-by-side manifest naming the hook DLL as the server for
//! both CLSIDs) is made active on this thread only. It lasts as long as this process, and
//! touches nothing outside it.
//!
//! For each dialog it prints which module the object `CoCreateInstance` returned lives in, first
//! without the context (comdlg32.dll), then with it. With `legacy`, it also calls
//! `GetOpenFileNameW` under the context, to see whether the old API goes through the same lookup
//! (the picker, or the real dialog, appears; the result is printed).

use std::ffi::c_void;
use std::path::Path;
use windows::core::{w, Interface, GUID, PCWSTR};
use windows::Win32::Foundation::{HANDLE, HMODULE, HWND};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::System::LibraryLoader::{
    GetModuleFileNameW, GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
    GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
};
use windows::Win32::UI::Shell::IFileDialog;

#[repr(C)]
struct ActCtx {
    size: u32,
    flags: u32,
    source: PCWSTR,
    arch: u16,
    lang: u16,
    assembly_dir: PCWSTR,
    resource: PCWSTR,
    app_name: PCWSTR,
    module: HMODULE,
}

/// Just the fields `GetOpenFileNameW` needs, laid out as OPENFILENAMEW.
#[repr(C)]
struct OpenFileName {
    size: u32,
    owner: HWND,
    instance: HMODULE,
    filter: PCWSTR,
    custom_filter: *mut u16,
    max_custom: u32,
    filter_index: u32,
    file: *mut u16,
    max_file: u32,
    file_title: *mut u16,
    max_file_title: u32,
    initial_dir: PCWSTR,
    title: PCWSTR,
    flags: u32,
    file_offset: u16,
    file_ext: u16,
    def_ext: PCWSTR,
    cust_data: isize,
    hook: *mut c_void,
    template: PCWSTR,
    reserved: *mut c_void,
    reserved2: u32,
    flags_ex: u32,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateActCtxW(ctx: *const ActCtx) -> HANDLE;
    fn ActivateActCtx(ctx: HANDLE, cookie: *mut usize) -> i32;
    fn DeactivateActCtx(flags: u32, cookie: usize) -> i32;
    fn ReleaseActCtx(ctx: HANDLE);
}

#[link(name = "comdlg32")]
unsafe extern "system" {
    fn GetOpenFileNameW(ofn: *mut OpenFileName) -> i32;
    fn CommDlgExtendedError() -> u32;
}

const ACTCTX_FLAG_ASSEMBLY_DIRECTORY_VALID: u32 = 0x4;
const OFN_EXPLORER: u32 = 0x80000;
const OFN_FILEMUSTEXIST: u32 = 0x1000;

fn module_of(object: &IFileDialog) -> String {
    unsafe {
        let vtable = *(object.as_raw() as *const *const c_void);
        let mut module = HMODULE::default();
        if GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            PCWSTR(vtable as *const u16),
            &mut module,
        )
        .is_err()
        {
            return "(unknown module)".into();
        }
        let mut buf = [0u16; 512];
        let n = GetModuleFileNameW(Some(module), &mut buf) as usize;
        String::from_utf16_lossy(&buf[..n])
    }
}

fn probe(label: &str) {
    for (name, clsid) in [
        ("FileOpenDialog", shellman_pickhook::real::CLSID_OPEN),
        ("FileSaveDialog", shellman_pickhook::real::CLSID_SAVE),
    ] {
        let made: windows::core::Result<IFileDialog> =
            unsafe { CoCreateInstance(&clsid as *const GUID, None, CLSCTX_INPROC_SERVER) };
        match made {
            Ok(d) => println!("{label}: CoCreateInstance({name}) -> {}", module_of(&d)),
            Err(e) => println!("{label}: CoCreateInstance({name}) failed: {e}"),
        }
    }
}

pub fn experiment(target: &Path, legacy: bool) {
    probe("no context");
    let dir = Path::new(r"C:\Temp\picktest\actctx");
    let _ = std::fs::create_dir_all(dir);
    let dll = target.join("shellman_pickhook.dll");
    if let Err(e) = std::fs::copy(&dll, dir.join("shellman_pickhook.dll")) {
        println!("couldn't copy {}: {e}", dll.display());
        return;
    }
    let manifest = dir.join("pickhook.manifest");
    let text = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity type="win32" name="shellman.pickhook.experiment" version="1.0.0.0"/>
  <file name="shellman_pickhook.dll">
    <comClass clsid="{DC1C5A9C-E88A-4dde-A5A1-60F82A20AEF7}" threadingModel="Apartment"/>
    <comClass clsid="{C0B4E2F3-BA21-4773-8DBA-335EC946EB8B}" threadingModel="Apartment"/>
  </file>
</assembly>
"#;
    std::fs::write(&manifest, text).expect("write the manifest");
    let wide = |p: &Path| -> Vec<u16> { p.as_os_str().to_string_lossy().encode_utf16().chain(Some(0)).collect() };
    let (source, assembly_dir) = (wide(&manifest), wide(dir));
    let ctx = ActCtx {
        size: size_of::<ActCtx>() as u32,
        flags: ACTCTX_FLAG_ASSEMBLY_DIRECTORY_VALID,
        source: PCWSTR(source.as_ptr()),
        arch: 0,
        lang: 0,
        assembly_dir: PCWSTR(assembly_dir.as_ptr()),
        resource: PCWSTR::null(),
        app_name: PCWSTR::null(),
        module: HMODULE::default(),
    };
    unsafe {
        let handle = CreateActCtxW(&ctx);
        if handle.0 as isize == -1 {
            println!("CreateActCtxW failed: {:?}", windows::core::Error::from_win32());
            return;
        }
        let mut cookie = 0usize;
        if ActivateActCtx(handle, &mut cookie) == 0 {
            println!("ActivateActCtx failed");
            ReleaseActCtx(handle);
            return;
        }
        probe("with the context");
        if legacy {
            let mut file = [0u16; 1024];
            let mut ofn = OpenFileName {
                size: size_of::<OpenFileName>() as u32,
                owner: HWND::default(),
                instance: HMODULE::default(),
                filter: w!("Text files\0*.txt;*.md\0All files\0*.*\0\0"),
                custom_filter: std::ptr::null_mut(),
                max_custom: 0,
                filter_index: 1,
                file: file.as_mut_ptr(),
                max_file: file.len() as u32,
                file_title: std::ptr::null_mut(),
                max_file_title: 0,
                initial_dir: w!(r"C:\Temp\picktest\files"),
                title: w!("GetOpenFileNameW under the experiment"),
                flags: OFN_EXPLORER | OFN_FILEMUSTEXIST,
                file_offset: 0,
                file_ext: 0,
                def_ext: PCWSTR::null(),
                cust_data: 0,
                hook: std::ptr::null_mut(),
                template: PCWSTR::null(),
                reserved: std::ptr::null_mut(),
                reserved2: 0,
                flags_ex: 0,
            };
            let ok = GetOpenFileNameW(&mut ofn);
            let len = file.iter().position(|&c| c == 0).unwrap_or(0);
            println!(
                "GetOpenFileNameW -> {ok} ({}), error {:#x}",
                String::from_utf16_lossy(&file[..len]),
                CommDlgExtendedError()
            );
        }
        let _ = DeactivateActCtx(0, cookie);
        ReleaseActCtx(handle);
    }
    probe("context gone");
}
