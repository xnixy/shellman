//! Start Menu shortcuts: `Programs\shellman\shellman <Name>.lnk` for the apps a user opens by
//! hand. The apps set no AppUserModelID of their own, so the shortcuts carry none either (a
//! shortcut's ID that no process shares would only split taskbar grouping).

use std::path::{Path, PathBuf};
use windows::core::{Interface, HSTRING};
use windows::Win32::Storage::FileSystem::WIN32_FIND_DATAW;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
    STGM_READ,
};
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

/// (shortcut name, exe in the install folder, description)
pub const SET: [(&str, &str, &str); 4] = [
    ("shellman Files", "shellman-files.exe", "Files"),
    ("shellman Settings", "shellman-settings.exe", "Settings"),
    ("shellman Task Manager", "shellman-tasks.exe", "Task Manager"),
    ("shellman Terminal", "shellman-terminal.exe", "Terminal"),
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shortcut {
    pub lnk: PathBuf,
    pub target: PathBuf,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Create(Shortcut),
    Retarget(Shortcut),
    UpToDate(Shortcut),
    /// The exe it would point at is not part of the install.
    Skip { name: String, why: String },
}

/// What to do for each shortcut. `have` says which exes exist in the install; `existing` reads a
/// shortcut file's current target (`None`: no such shortcut).
pub fn plan(
    folder: &Path,
    prefix: &Path,
    have: &dyn Fn(&str) -> bool,
    existing: &dyn Fn(&Path) -> Option<PathBuf>,
) -> Vec<Step> {
    SET.iter()
        .map(|(name, exe, desc)| {
            if !have(exe) {
                return Step::Skip { name: name.to_string(), why: format!("{exe} is not part of the install") };
            }
            let sc = Shortcut { lnk: folder.join(format!("{name}.lnk")), target: prefix.join(exe), description: desc.to_string() };
            match existing(&sc.lnk) {
                None => Step::Create(sc),
                Some(t) if t.to_string_lossy().eq_ignore_ascii_case(&sc.target.to_string_lossy()) => Step::UpToDate(sc),
                Some(_) => Step::Retarget(sc),
            }
        })
        .collect()
}

/// The shortcut names we own (file names), for uninstall.
pub fn our_files() -> Vec<String> {
    SET.iter().map(|(n, _, _)| format!("{n}.lnk")).collect()
}

struct Com;
impl Com {
    fn new() -> Com {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        Com
    }
}
impl Drop for Com {
    fn drop(&mut self) {
        unsafe { CoUninitialize() }
    }
}

/// The target of an existing shortcut file.
pub fn read_target(lnk: &Path) -> Option<PathBuf> {
    if !lnk.is_file() {
        return None;
    }
    let _com = Com::new();
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let file: IPersistFile = link.cast().ok()?;
        file.Load(&HSTRING::from(lnk.as_os_str()), STGM_READ).ok()?;
        let mut buf = [0u16; 1024];
        link.GetPath(&mut buf, std::ptr::null_mut::<WIN32_FIND_DATAW>(), 0).ok()?;
        let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(PathBuf::from(String::from_utf16_lossy(&buf[..n])))
    }
}

/// Write a shortcut file (the folder is created).
pub fn write(sc: &Shortcut) -> Result<(), String> {
    if let Some(dir) = sc.lnk.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    let _com = Com::new();
    unsafe {
        fn e(what: &'static str) -> impl Fn(windows::core::Error) -> String {
            move |err| format!("{what}: {err}")
        }
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).map_err(e("shell link"))?;
        link.SetPath(&HSTRING::from(sc.target.as_os_str())).map_err(e("target"))?;
        if let Some(dir) = sc.target.parent() {
            let _ = link.SetWorkingDirectory(&HSTRING::from(dir.as_os_str()));
        }
        let _ = link.SetDescription(&HSTRING::from(sc.description.as_str()));
        let _ = link.SetIconLocation(&HSTRING::from(sc.target.as_os_str()), 0);
        let file: IPersistFile = link.cast().map_err(e("persist"))?;
        file.Save(&HSTRING::from(sc.lnk.as_os_str()), true).map_err(e("save"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans_each_shortcut() {
        let folder = Path::new(r"C:\SM\shellman");
        let prefix = Path::new(r"C:\P\shellman");
        let have = |e: &str| e != "shellman-terminal.exe";
        let existing = |p: &Path| match p.file_name().unwrap().to_str().unwrap() {
            "shellman Settings.lnk" => Some(PathBuf::from(r"c:\p\SHELLMAN\shellman-settings.exe")),
            "shellman Task Manager.lnk" => Some(PathBuf::from(r"C:\old\shellman-tasks.exe")),
            _ => None,
        };
        let steps = plan(folder, prefix, &have, &existing);
        assert!(matches!(&steps[0], Step::Create(s) if s.lnk == folder.join("shellman Files.lnk") && s.target == prefix.join("shellman-files.exe")));
        assert!(matches!(&steps[1], Step::UpToDate(_)));
        assert!(matches!(&steps[2], Step::Retarget(s) if s.target == prefix.join("shellman-tasks.exe")));
        assert!(matches!(&steps[3], Step::Skip { name, .. } if name == "shellman Terminal"));
    }

    #[test]
    fn the_set_is_files_settings_tasks_terminal() {
        let names: Vec<_> = our_files();
        assert_eq!(names, ["shellman Files.lnk", "shellman Settings.lnk", "shellman Task Manager.lnk", "shellman Terminal.lnk"]);
    }

    /// The temp folder in its long form: `%TEMP%` can be a short 8.3 path (GitHub's runners),
    /// while a shortcut's target reads back long. (As `shellman_core::long_temp_dir`.)
    fn long_temp_dir() -> std::path::PathBuf {
        use windows::Win32::Storage::FileSystem::GetLongPathNameW;
        let short = std::env::temp_dir();
        let mut buf = vec![0u16; 1024];
        let n = unsafe { GetLongPathNameW(&HSTRING::from(short.as_os_str()), Some(&mut buf)) } as usize;
        if n == 0 || n > buf.len() {
            return short;
        }
        std::path::PathBuf::from(String::from_utf16_lossy(&buf[..n]))
    }

    #[test]
    fn writes_and_reads_back_a_shortcut_in_a_temp_folder() {
        let d = long_temp_dir().join(format!("shellman-setup-lnk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let target = d.join("shellman-files.exe");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(&target, b"x").unwrap();
        let sc = Shortcut { lnk: d.join("menu").join("shellman Files.lnk"), target: target.clone(), description: "Files".into() };
        write(&sc).unwrap();
        assert_eq!(read_target(&sc.lnk).map(|p| p.to_string_lossy().to_lowercase()), Some(target.to_string_lossy().to_lowercase()));
        let _ = std::fs::remove_dir_all(&d);
    }
}
