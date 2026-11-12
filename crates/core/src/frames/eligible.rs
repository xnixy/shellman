//! Which windows get restyled. Pure, over facts the service gathers.

use windows::Win32::UI::WindowsAndMessaging::{WS_CAPTION, WS_EX_TOOLWINDOW, WS_THICKFRAME, WS_VISIBLE};

/// Windows that are never an app's frame: the desktop, the taskbars, menus and tooltips.
const SKIP_CLASSES: [&str; 9] = [
    "Progman",
    "WorkerW",
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
    "ShellmanDesktop",
    "#32768",
    "tooltips_class32",
    "Windows.UI.Core.CoreWindow",
    "XamlExplorerHostIslandWindow",
];

pub struct WindowFacts<'a> {
    pub style: u32,
    pub ex_style: u32,
    pub cloaked: bool,
    /// Belongs to a shellman app (they draw their own frames).
    pub own_process: bool,
    /// Covers its whole monitor without being a maximized framed window.
    pub fullscreen: bool,
    pub class: &'a str,
}

/// Is this a visible, framed, ordinary application window of someone else?
pub fn eligible(w: &WindowFacts) -> bool {
    w.style & WS_VISIBLE.0 != 0
        && w.style & (WS_CAPTION.0 | WS_THICKFRAME.0) != 0
        && w.ex_style & WS_EX_TOOLWINDOW.0 == 0
        && !w.cloaked
        && !w.own_process
        && !w.fullscreen
        && !SKIP_CLASSES.contains(&w.class)
}

/// Is the program at `exe` one of shellman's own? Its name starts with `shellman-`, or it sits in
/// one of `dirs` (the install folder, the folder this app runs from).
pub fn is_own_exe(exe: &std::path::Path, dirs: &[std::path::PathBuf]) -> bool {
    let named = exe
        .file_name()
        .is_some_and(|n| n.to_string_lossy().to_lowercase().starts_with("shellman-"));
    let lower = |p: &std::path::Path| p.to_string_lossy().to_lowercase();
    named || exe.parent().is_some_and(|d| dirs.iter().any(|o| lower(o) == lower(d)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_programs_by_name_or_folder() {
        use std::path::{Path, PathBuf};
        let dirs = [PathBuf::from(r"C:\Users\x\AppData\Local\Programs\shellman")];
        assert!(is_own_exe(Path::new(r"D:\target\debug\Shellman-Files.exe"), &dirs));
        assert!(is_own_exe(Path::new(r"c:\users\x\appdata\local\programs\SHELLMAN\other.exe"), &dirs));
        assert!(!is_own_exe(Path::new(r"C:\Windows\notepad.exe"), &dirs));
        assert!(!is_own_exe(Path::new(r"C:\Users\x\AppData\Local\Programs\shellman\sub\a.exe"), &dirs));
    }

    fn ok() -> WindowFacts<'static> {
        WindowFacts {
            style: WS_VISIBLE.0 | WS_CAPTION.0,
            ex_style: 0,
            cloaked: false,
            own_process: false,
            fullscreen: false,
            class: "Notepad",
        }
    }

    #[test]
    fn an_ordinary_framed_window_qualifies() {
        assert!(eligible(&ok()));
        // A frame without a caption (resizable border only) still has a DWM frame.
        assert!(eligible(&WindowFacts { style: WS_VISIBLE.0 | WS_THICKFRAME.0, ..ok() }));
    }

    #[test]
    fn each_disqualifier_counts() {
        assert!(!eligible(&WindowFacts { style: WS_CAPTION.0, ..ok() }), "hidden");
        assert!(!eligible(&WindowFacts { style: WS_VISIBLE.0, ..ok() }), "no frame");
        assert!(!eligible(&WindowFacts { ex_style: WS_EX_TOOLWINDOW.0, ..ok() }));
        assert!(!eligible(&WindowFacts { cloaked: true, ..ok() }));
        assert!(!eligible(&WindowFacts { own_process: true, ..ok() }));
        assert!(!eligible(&WindowFacts { fullscreen: true, ..ok() }));
        assert!(!eligible(&WindowFacts { class: "Progman", ..ok() }));
    }
}
