//! Finding running processes by exe path in this logon session. Used to adopt apps a previous
//! session left running, and to see whether Explorer runs.

use windows::core::PWSTR;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};

/// Lower-case, with the `\\?\` prefix gone, so two spellings of one path compare equal.
pub fn normal(path: &str) -> String {
    path.trim_start_matches("\\\\?\\").replace('/', "\\").to_lowercase()
}

/// Is `image` (a running process's exe) this app `want`? Same path, or the same file renamed
/// aside by an update: Windows reports the renamed path for a running image, so a bar replaced
/// while it ran shows up as `shellman-bar.old123456.exe` in the same folder.
pub fn same_app(want: &str, image: &str) -> bool {
    let (want, image) = (normal(want), normal(image));
    if want == image {
        return true;
    }
    let split = |p: &str| match p.rfind('\\') {
        Some(i) => (p[..i].to_string(), p[i + 1..].to_string()),
        None => (String::new(), p.to_string()),
    };
    let ((wd, wn), (id, inn)) = (split(&want), split(&image));
    let Some(stem) = wn.strip_suffix(".exe") else { return false };
    wd == id && is_old_name(&inn, stem)
}

/// `<stem>.old<something without dots>.exe`, lower-case input.
fn is_old_name(name: &str, stem: &str) -> bool {
    name.strip_prefix(stem)
        .and_then(|r| r.strip_prefix(".old"))
        .and_then(|r| r.strip_suffix(".exe"))
        .is_some_and(|mid| !mid.is_empty() && !mid.contains(['.', '\\']))
}

/// A process's full exe path.
pub fn image_of(pid: u32) -> Option<String> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let got = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = CloseHandle(h);
        got.ok()?;
        Some(String::from_utf16_lossy(&buf[..len as usize]))
    }
}

fn my_session() -> u32 {
    let mut s = 0;
    unsafe {
        let _ = ProcessIdToSessionId(GetCurrentProcessId(), &mut s);
    }
    s
}

/// Pids of processes in this logon session whose exe is `exe`, other than this one.
pub fn by_exe(exe: &std::path::Path) -> Vec<u32> {
    let want = exe.to_string_lossy().into_owned();
    let (me, session) = (unsafe { GetCurrentProcessId() }, my_session());
    let mut found = Vec::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return found };
        let mut entry = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut ok = Process32FirstW(snap, &mut entry).is_ok();
        while ok {
            let pid = entry.th32ProcessID;
            if pid != me {
                let mut s = u32::MAX;
                let _ = ProcessIdToSessionId(pid, &mut s);
                if s == session && image_of(pid).is_some_and(|i| same_app(&want, &i)) {
                    found.push(pid);
                }
            }
            ok = Process32NextW(snap, &mut entry).is_ok();
        }
        let _ = CloseHandle(snap);
    }
    found
}

#[cfg(test)]
mod tests {
    #[test]
    fn paths_compare_loosely() {
        assert_eq!(super::normal("\\\\?\\C:\\A/b.EXE"), "c:\\a\\b.exe");
    }

    #[test]
    fn a_renamed_image_is_still_the_app() {
        let want = "C:\\P\\shellman-bar.exe";
        assert!(super::same_app(want, "c:\\p\\SHELLMAN-BAR.exe"));
        assert!(super::same_app(want, "C:\\P\\shellman-bar.old123456.exe"));
        assert!(super::same_app(want, "\\\\?\\C:\\P\\shellman-bar.old1.exe"));
    }

    #[test]
    fn nothing_else_matches() {
        let want = "C:\\P\\shellman-bar.exe";
        assert!(!super::same_app(want, "C:\\P\\shellman-barx.exe"));
        assert!(!super::same_app(want, "C:\\P\\shellman-bar.old1.txt"));
        assert!(!super::same_app(want, "C:\\P\\shellman-bar.old.exe.exe"));
        assert!(!super::same_app(want, "C:\\P\\shellman-bar.old1.2.exe"));
        assert!(!super::same_app(want, "C:\\Q\\shellman-bar.old1.exe"));
        assert!(!super::same_app(want, "C:\\P\\sub\\shellman-bar.old1.exe"));
        assert!(!super::same_app(want, "C:\\P\\shellman-shelf.old1.exe"));
    }

    #[test]
    fn finds_nothing_for_an_unused_path() {
        assert!(super::by_exe(std::path::Path::new("C:\\no\\such\\shellman-x.exe")).is_empty());
    }
}
