//! One running shell: its pseudo-console and the emulator's state. It knows nothing of how it is
//! drawn, so tabs later are several of these and one view.
//!
//! `alacritty_terminal` runs the pseudo-console (ConPTY) and the VT parser on a thread of its
//! own; the grid sits behind a lock the view takes briefly to read it.

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::tty;
use std::borrow::Cow;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

/// Lines of history kept above the screen.
const HISTORY: usize = 10_000;

/// The grid's size in cells, and one cell's size in pixels (ConPTY is told both).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Size {
    pub cols: u16,
    pub rows: u16,
    pub cell_w: u16,
    pub cell_h: u16,
}

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.rows as usize
    }
    fn screen_lines(&self) -> usize {
        self.rows as usize
    }
    fn columns(&self) -> usize {
        self.cols as usize
    }
}

impl From<Size> for WindowSize {
    fn from(s: Size) -> Self {
        WindowSize { num_lines: s.rows, num_cols: s.cols, cell_width: s.cell_w, cell_height: s.cell_h }
    }
}

/// What the emulator tells the window, set from the pty thread.
#[derive(Default)]
struct Shared {
    /// New output (or anything else) to draw.
    dirty: AtomicBool,
    exited: AtomicBool,
    /// The title a program set; None for the default.
    title: Mutex<Option<String>>,
}

/// Hears the emulator: answers it straight back into the pty (cursor reports, colour queries),
/// and wakes the window for anything to draw.
#[derive(Clone)]
pub struct Listener {
    shared: Arc<Shared>,
    pty: Arc<OnceLock<EventLoopSender>>,
    /// Wakes the window: a repaint request, from any thread.
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl Listener {
    fn write(&self, text: String) {
        if let Some(pty) = self.pty.get() {
            let _ = pty.send(Msg::Input(Cow::Owned(text.into_bytes())));
        }
    }
}

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        match event {
            Event::Wakeup | Event::MouseCursorDirty | Event::CursorBlinkingChange => {}
            Event::Title(title) => *self.shared.title.lock().unwrap() = Some(title),
            Event::ResetTitle => *self.shared.title.lock().unwrap() = None,
            Event::PtyWrite(text) => {
                self.write(text);
                return;
            }
            // Not answered yet: programs that ask (vim's background check) carry on without.
            Event::ColorRequest(_, _) | Event::TextAreaSizeRequest(_) => return,
            Event::ChildExit(_) | Event::Exit => self.shared.exited.store(true, Ordering::Release),
            Event::Bell | Event::ClipboardStore(..) | Event::ClipboardLoad(..) => return,
        }
        self.shared.dirty.store(true, Ordering::Release);
        (self.wake)();
    }
}

pub struct Session {
    term: Arc<FairMutex<Term<Listener>>>,
    pty: EventLoopSender,
    shared: Arc<Shared>,
    size: Size,
    /// The program, for the title until it sets one.
    pub program: String,
}

impl Session {
    /// Start `command` (a whole command line) in a pseudo-console of `size`. `wake` is called
    /// from the pty thread whenever there is something new to draw.
    pub fn start(command: &str, size: Size, wake: Arc<dyn Fn() + Send + Sync>) -> std::io::Result<Self> {
        let shared = Arc::new(Shared::default());
        let pty_slot = Arc::new(OnceLock::new());
        let listener = Listener { shared: shared.clone(), pty: pty_slot.clone(), wake };
        let config = Config { scrolling_history: HISTORY, ..Config::default() };
        let term = Arc::new(FairMutex::new(Term::new(config, &size, listener.clone())));
        let options = tty::Options {
            // The command line goes to CreateProcess as it was typed.
            shell: Some(tty::Shell::new(command.to_owned(), Vec::new())),
            working_directory: None,
            drain_on_exit: true,
            env: Default::default(),
            escape_args: false,
        };
        let pty = tty::new(&options, size.into(), 0)?;
        let event_loop = EventLoop::new(term.clone(), listener, pty, true, false)?;
        let sender = event_loop.channel();
        let _ = pty_slot.set(sender.clone());
        event_loop.spawn();
        Ok(Self { term, pty: sender, shared, size, program: program_name(command) })
    }

    /// Send bytes to the shell, as if typed. The view follows to the bottom.
    pub fn write(&self, bytes: Vec<u8>) {
        if bytes.is_empty() {
            return;
        }
        {
            let mut term = self.term.lock();
            if term.grid().display_offset() != 0 {
                term.scroll_display(Scroll::Bottom);
                self.shared.dirty.store(true, Ordering::Release);
            }
        }
        let _ = self.pty.send(Msg::Input(Cow::Owned(bytes)));
    }

    /// Fit the pseudo-console and the grid to a new size. Free when it is the same.
    pub fn resize(&mut self, size: Size) {
        if size == self.size {
            return;
        }
        let cells_changed = (size.cols, size.rows) != (self.size.cols, self.size.rows);
        self.size = size;
        if cells_changed {
            self.term.lock().resize(size);
            let _ = self.pty.send(Msg::Resize(size.into()));
            self.shared.dirty.store(true, Ordering::Release);
        }
    }

    pub fn size(&self) -> Size {
        self.size
    }

    /// Move through the history: positive lines up, negative down.
    pub fn scroll(&self, lines: i32) {
        if lines != 0 {
            self.term.lock().scroll_display(Scroll::Delta(lines));
            self.shared.dirty.store(true, Ordering::Release);
        }
    }

    /// Whether there is anything new to draw; clears it.
    pub fn take_dirty(&self) -> bool {
        self.shared.dirty.swap(false, Ordering::AcqRel)
    }


    pub fn exited(&self) -> bool {
        self.shared.exited.load(Ordering::Acquire)
    }

    pub fn title(&self) -> String {
        self.shared.title.lock().unwrap().clone().filter(|t| !t.trim().is_empty()).unwrap_or_else(|| self.program.clone())
    }

    pub fn mode(&self) -> TermMode {
        *self.term.lock().mode()
    }

    /// Read the emulator's state under its lock.
    pub fn with_term<R>(&self, f: impl FnOnce(&Term<Listener>) -> R) -> R {
        f(&self.term.lock())
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.pty.send(Msg::Shutdown);
    }
}

/// "powershell" from `C:\...\powershell.exe -NoLogo`: what the title says before a program sets one.
fn program_name(command: &str) -> String {
    let first = if let Some(rest) = command.trim_start().strip_prefix('"') {
        rest.split('"').next().unwrap_or(rest)
    } else {
        command.split_whitespace().next().unwrap_or(command)
    };
    let name = first.rsplit(['\\', '/']).next().unwrap_or(first);
    let name = name.strip_suffix(".exe").or_else(|| name.strip_suffix(".EXE")).unwrap_or(name);
    name.to_owned()
}

#[cfg(test)]
mod tests {
    use super::program_name;

    #[test]
    fn program_names() {
        assert_eq!(program_name("powershell.exe"), "powershell");
        assert_eq!(program_name(r"C:\Windows\System32\cmd.exe /k echo hi"), "cmd");
        assert_eq!(program_name(r#""C:\Program Files\PowerShell\7\pwsh.exe" -NoLogo"#), "pwsh");
        assert_eq!(program_name("wsl"), "wsl");
    }
}
