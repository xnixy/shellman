//! A test app for the hook, used by hand. It gets its dialog straight from the hook's class
//! factory (never through the registry), shows it over a window of its own, and prints what
//! came back.
//!
//! ```text
//! cargo build -p shellman-files -p shellman-pickhook --examples
//! cargo run -p shellman-pickhook --example host -- <scenario> [folder]
//! ```
//!
//! Scenarios: `open`, `many`, `save`, `folder`, `veto` (an open whose first choice the app's
//! OnFileOk refuses), `custom` (adds a combo box: the real dialog), `missing` and `failing`
//! (the picker cannot start, or fails: the real dialog), `real` (the real dialog straight from
//! comdlg32, nothing else), `actctx` (the registration-free experiment: see `actctx` below) and
//! `legacy` (GetOpenFileNameW under that experiment).
//!
//! The picker is `target\<profile>\shellman-files.exe` unless SHELLMAN_PICKER_EXE says
//! otherwise; the hook's log goes to %TEMP%\shellman-pickhook-host.log.

use std::cell::Cell;
use std::path::PathBuf;
use windows::core::{implement, w, Interface, Result, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, S_FALSE, WPARAM};
use windows::Win32::Graphics::Gdi::{GetStockObject, HBRUSH, WHITE_BRUSH};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, IClassFactory, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    IFileDialog, IFileDialogCustomize, IFileDialogEvents, IFileDialogEvents_Impl, IFileOpenDialog,
    IFileSaveDialog, IShellItem, SHCreateItemFromParsingName, FDE_OVERWRITE_RESPONSE,
    FDE_SHAREVIOLATION_RESPONSE, FOS_ALLOWMULTISELECT, FOS_PICKFOLDERS, SIGDN_FILESYSPATH,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassW, ShowWindow, SW_SHOW,
    WINDOW_EX_STYLE, WNDCLASSW, WS_OVERLAPPEDWINDOW,
};

#[path = "host/actctx.rs"]
mod actctx;

fn main() {
    let mut args = std::env::args().skip(1);
    let scenario = args.next().unwrap_or_else(|| "open".into());
    let folder = args.next().map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Temp\picktest\files"));
    let me = std::env::current_exe().unwrap();
    let target = me.parent().and_then(|d| d.parent()).unwrap().to_path_buf();
    if std::env::var_os(shellman_pickhook::launch::EXE_VAR).is_none() {
        std::env::set_var(shellman_pickhook::launch::EXE_VAR, target.join("shellman-files.exe"));
    }
    if std::env::var_os(shellman_pickhook::log::VAR).is_none() {
        std::env::set_var(shellman_pickhook::log::VAR, std::env::temp_dir().join("shellman-pickhook-host.log"));
    }
    match scenario.as_str() {
        "missing" => std::env::set_var(shellman_pickhook::launch::EXE_VAR, r"C:\Temp\picktest\no-such-picker.exe"),
        // where.exe refuses `--pick` with exit code 2, the picker's "failed".
        "failing" => std::env::set_var(shellman_pickhook::launch::EXE_VAR, r"C:\Windows\System32\where.exe"),
        _ => {}
    }
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    println!("host pid {}", std::process::id());
    if scenario == "actctx" || scenario == "legacy" {
        actctx::experiment(&target, scenario == "legacy");
        return;
    }
    let owner = owner_window();
    println!("owner hwnd {:#x}", owner.0 as usize);
    if let Err(e) = run(&scenario, &folder, owner) {
        println!("error: {e}");
    }
    unsafe {
        let _ = DestroyWindow(owner);
    }
}

fn run(scenario: &str, folder: &std::path::Path, owner: HWND) -> Result<()> {
    let save = scenario == "save";
    let dialog: IFileDialog = if scenario == "real" {
        unsafe {
            CoCreateInstance(
                if save { &shellman_pickhook::real::CLSID_SAVE } else { &shellman_pickhook::real::CLSID_OPEN },
                None,
                CLSCTX_INPROC_SERVER,
            )?
        }
    } else {
        hook_dialog(save)?
    };
    unsafe {
        let types = [
            (w!("Text files"), w!("*.txt;*.md")),
            (w!("Spreadsheets"), w!("*.csv")),
            (w!("All files"), w!("*.*")),
        ];
        let specs: Vec<COMDLG_FILTERSPEC> = types
            .iter()
            .map(|(n, s)| COMDLG_FILTERSPEC { pszName: *n, pszSpec: *s })
            .collect();
        let mut options = dialog.GetOptions()?;
        match scenario {
            "many" => options |= FOS_ALLOWMULTISELECT,
            "folder" => options |= FOS_PICKFOLDERS,
            _ => {}
        }
        dialog.SetOptions(options)?;
        if scenario != "folder" {
            dialog.SetFileTypes(&specs)?;
            dialog.SetFileTypeIndex(1)?;
        }
        if save {
            dialog.SetDefaultExtension(w!("txt"))?;
            dialog.SetFileName(w!("report"))?;
            dialog.SetTitle(w!("Save the report"))?;
        }
        let wide: Vec<u16> = folder.as_os_str().to_string_lossy().encode_utf16().chain(Some(0)).collect();
        let item: IShellItem = SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None)?;
        dialog.SetFolder(&item)?;
        if scenario == "custom" {
            let custom: IFileDialogCustomize = dialog.cast()?;
            custom.AddComboBox(100)?;
            custom.AddControlItem(100, 1, w!("UTF-8"))?;
            custom.AddControlItem(100, 2, w!("UTF-16"))?;
            custom.SetSelectedControlItem(100, 1)?;
        }
        let events: IFileDialogEvents = Events { veto: Cell::new(scenario == "veto") }.into();
        let cookie = dialog.Advise(&events)?;
        let shown = dialog.Show(Some(owner));
        let _ = dialog.Unadvise(cookie);
        match shown {
            Ok(()) => println!("Show: OK"),
            Err(e) => {
                println!("Show: {:?} {}", e.code(), e.message());
                return Ok(());
            }
        }
        if let Ok(open) = dialog.cast::<IFileOpenDialog>() {
            let results = open.GetResults()?;
            for i in 0..results.GetCount()? {
                println!("result: {}", path(&results.GetItemAt(i)?));
            }
        } else {
            println!("result: {}", path(&dialog.GetResult()?));
        }
        println!("type index: {}", dialog.GetFileTypeIndex().unwrap_or(0));
        if let Ok(f) = dialog.GetFolder() {
            println!("folder: {}", path(&f));
        }
    }
    Ok(())
}

/// A dialog from the hook's own class factory: no registry involved.
fn hook_dialog(save: bool) -> Result<IFileDialog> {
    let clsid = if save { shellman_pickhook::real::CLSID_SAVE } else { shellman_pickhook::real::CLSID_OPEN };
    let mut raw = std::ptr::null_mut();
    unsafe { shellman_pickhook::class_object(&clsid, &IClassFactory::IID, &mut raw) }.ok()?;
    let factory = unsafe { IClassFactory::from_raw(raw) };
    if save {
        let d: IFileSaveDialog = unsafe { factory.CreateInstance(None)? };
        d.cast()
    } else {
        unsafe { factory.CreateInstance(None) }
    }
}

fn path(item: &IShellItem) -> String {
    unsafe {
        match item.GetDisplayName(SIGDN_FILESYSPATH) {
            Ok(p) => {
                let s = p.to_string().unwrap_or_default();
                CoTaskMemFree(Some(p.0 as *const _));
                s
            }
            Err(e) => format!("(no path: {e})"),
        }
    }
}

/// The app's event sink: prints what it hears, and in `veto` refuses the first choice.
#[implement(IFileDialogEvents)]
struct Events {
    veto: Cell<bool>,
}

impl IFileDialogEvents_Impl for Events_Impl {
    fn OnFileOk(&self, pfd: windows::core::Ref<'_, IFileDialog>) -> Result<()> {
        let name = pfd
            .ok()
            .and_then(|d| unsafe { d.GetResult() })
            .map(|i| path(&i))
            .unwrap_or_else(|e| format!("(GetResult failed: {e})"));
        if self.veto.replace(false) {
            println!("OnFileOk: {name}: refused");
            return Err(S_FALSE.into());
        }
        println!("OnFileOk: {name}: accepted");
        Ok(())
    }
    fn OnFolderChanging(&self, _: windows::core::Ref<'_, IFileDialog>, _: windows::core::Ref<'_, IShellItem>) -> Result<()> {
        Ok(())
    }
    fn OnFolderChange(&self, _: windows::core::Ref<'_, IFileDialog>) -> Result<()> {
        println!("OnFolderChange");
        Ok(())
    }
    fn OnSelectionChange(&self, _: windows::core::Ref<'_, IFileDialog>) -> Result<()> {
        Ok(())
    }
    fn OnShareViolation(&self, _: windows::core::Ref<'_, IFileDialog>, _: windows::core::Ref<'_, IShellItem>) -> Result<FDE_SHAREVIOLATION_RESPONSE> {
        Ok(FDE_SHAREVIOLATION_RESPONSE(0))
    }
    fn OnTypeChange(&self, _: windows::core::Ref<'_, IFileDialog>) -> Result<()> {
        println!("OnTypeChange");
        Ok(())
    }
    fn OnOverwrite(&self, _: windows::core::Ref<'_, IFileDialog>, _: windows::core::Ref<'_, IShellItem>) -> Result<FDE_OVERWRITE_RESPONSE> {
        Ok(FDE_OVERWRITE_RESPONSE(0))
    }
}

/// A plain window to own the dialog, at a known place, so the picker's centring can be seen.
fn owner_window() -> HWND {
    unsafe extern "system" fn proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        unsafe { DefWindowProcW(h, m, w, l) }
    }
    unsafe {
        let instance = GetModuleHandleW(None).unwrap_or_default();
        let class = WNDCLASSW {
            lpfnWndProc: Some(proc),
            hInstance: instance.into(),
            lpszClassName: w!("shellman-pickhook-host"),
            hbrBackground: HBRUSH(GetStockObject(WHITE_BRUSH).0),
            ..Default::default()
        };
        RegisterClassW(&class);
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("shellman-pickhook-host"),
            w!("Pick hook test host"),
            WS_OVERLAPPEDWINDOW,
            300,
            200,
            1400,
            1000,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .unwrap_or_default();
        let _ = ShowWindow(hwnd, SW_SHOW);
        hwnd
    }
}
