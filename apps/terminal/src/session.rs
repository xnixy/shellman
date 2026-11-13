//! One running shell: its pseudo-console and the emulator's state. It knows nothing of how it is
//! drawn; a tab is one of these.
//!
//! `alacritty_terminal` runs the pseudo-console (ConPTY) and the VT parser on a thread of its
//! own; the grid sits behind a lock the view takes briefly to read it. The pty's output passes
//! through [`osc::Scanner`] on its way to the parser, for what the emulator does not keep (the
//! working directory a shell reports, X10 mouse mode).
//!
//! A tab that is not shown still reads and parses its shell's output, but does not wake the
//! window for it: only the first output after it was last looked at does (to show the activity
//! dot), and titles, bells and exits.

use crate::osc::{self, Found};
use crate::palette::{Palette, Rgb};
use alacritty_terminal::event::{Event, EventListener, OnResize, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::tty::{self, ChildEvent, EventedPty, EventedReadWrite};
use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

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

/// The colours a program may ask about (OSC 4, 10, 11, 12), as the window shows them.
#[derive(Clone, Copy)]
pub struct Colours {
    pub palette: Palette,
    pub cursor: Rgb,
}

/// What the emulator tells the window, set from the pty thread.
struct Shared {
    /// New output (or anything else) to draw.
    dirty: AtomicBool,
    /// Counts the emulator's wake-ups for new output (find recounts when it moves).
    output_seq: AtomicU64,
    exited: AtomicBool,
    /// The program's exit code, once it exited (when known).
    exit_code: Mutex<Option<i32>>,
    /// This tab is the one shown: its output wakes the window.
    shown: AtomicBool,
    /// Output came while the tab was not shown.
    activity: AtomicBool,
    bell: AtomicBool,
    /// DECSET 9, which the emulator does not track.
    x10: AtomicBool,
    /// DECSET 9001: the console wants whole key events (`keys::Win32Key`).
    win32_input: AtomicBool,
    /// The title a program set; None for the default.
    title: Mutex<Option<String>>,
    /// The working directory the shell last reported.
    cwd: Mutex<Option<String>>,
    /// Text a program asked to copy (OSC 52), not yet put on the clipboard.
    copy: Mutex<Option<String>>,
    colours: Mutex<Option<Colours>>,
    size: Mutex<WindowSize>,
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

    fn wake(&self) {
        self.shared.dirty.store(true, Ordering::Release);
        (self.wake)();
    }
}

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        let s = &self.shared;
        match event {
            Event::Wakeup => {
                s.dirty.store(true, Ordering::Release);
                s.output_seq.fetch_add(1, Ordering::AcqRel);
                // A tab not shown wakes the window once, for its dot.
                if s.shown.load(Ordering::Acquire) || !s.activity.swap(true, Ordering::AcqRel) {
                    (self.wake)();
                }
            }
            Event::MouseCursorDirty => {}
            Event::CursorBlinkingChange => self.wake(),
            Event::Title(title) => {
                *s.title.lock().unwrap() = Some(title);
                self.wake();
            }
            Event::ResetTitle => {
                *s.title.lock().unwrap() = None;
                self.wake();
            }
            Event::PtyWrite(text) => self.write(text),
            Event::ColorRequest(index, format) => {
                if let Some(c) = *s.colours.lock().unwrap() {
                    let rgb = match index {
                        0..=255 => c.palette.indexed(index as u8),
                        256 => c.palette.fg,
                        257 => c.palette.bg,
                        _ => c.cursor,
                    };
                    self.write(format(alacritty_terminal::vte::ansi::Rgb { r: rgb[0], g: rgb[1], b: rgb[2] }));
                }
            }
            Event::TextAreaSizeRequest(format) => {
                let size = *s.size.lock().unwrap();
                self.write(format(size));
            }
            Event::ChildExit(status) => {
                *s.exit_code.lock().unwrap() = status.code();
                s.exited.store(true, Ordering::Release);
                self.wake();
            }
            Event::Exit => {
                s.exited.store(true, Ordering::Release);
                self.wake();
            }
            Event::Bell => {
                s.bell.store(true, Ordering::Release);
                self.wake();
            }
            // OSC 52: copy only (the emulator is set to ignore requests to read the clipboard).
            Event::ClipboardStore(_, text) => {
                *s.copy.lock().unwrap() = Some(text);
                self.wake();
            }
            Event::ClipboardLoad(..) => {}
        }
    }
}

/// The pseudo-console, with its output read through the scanner.
struct ScanPty<P: EventedPty> {
    pty: P,
    reader: ScanReader<P>,
}

struct ScanReader<P: EventedPty> {
    /// The pty's own reader, pointed at again by every `reader()` call (it lives in `pty`,
    /// beside this, and is only used through the borrow that call hands out).
    pipe: *mut P::Reader,
    scanner: osc::Scanner,
    shared: Arc<Shared>,
    /// Bytes handed to the parser since the event loop last found the pipe empty.
    given: usize,
    /// Where to post "readable" again (see `READ_PASS`).
    poller: Option<Arc<polling::Poller>>,
}

/// Bytes handed over in one read pass of the event loop. `alacritty_terminal` stops a pass after
/// 64 KiB, and on Windows nothing tells it to read again while its pipe still holds data, so a
/// fast writer (`cat` of a big file) could leave it waiting for ever. Passes end here instead,
/// below that limit, with a "readable" event posted so the loop comes straight back.
const READ_PASS: usize = 48 * 1024;

// The pointer is only followed inside the `&mut ScanPty` borrow that set it.
unsafe impl<P: EventedPty> Send for ScanReader<P> {}

impl<P: EventedPty> std::io::Read for ScanReader<P> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.given >= READ_PASS {
            // End this pass (nothing read), and be woken for the next one.
            self.given = 0;
            if let Some(poller) = &self.poller {
                use polling::os::iocp::{CompletionPacket, PollerIocpExt};
                let _ = poller.post(CompletionPacket::new(polling::Event::readable(tty::PTY_READ_WRITE_TOKEN)));
            }
            return Ok(0);
        }
        let room = buf.len().min(READ_PASS - self.given);
        // SAFETY: set from `&mut self.pty` by `ScanPty::reader`, whose borrow this call is in.
        let n = unsafe { &mut *self.pipe }.read(&mut buf[..room])?;
        // Empty: the pipe wakes the loop itself when more comes.
        self.given = if n == 0 { 0 } else { self.given + n };
        let shared = &self.shared;
        self.scanner.feed(&buf[..n], |found| match found {
            Found::Cwd(dir) => *shared.cwd.lock().unwrap() = Some(dir),
            Found::X10(on) => shared.x10.store(on, Ordering::Release),
            Found::Win32Input(on) => shared.win32_input.store(on, Ordering::Release),
        });
        Ok(n)
    }
}

impl<P: EventedPty> EventedReadWrite for ScanPty<P> {
    type Reader = ScanReader<P>;
    type Writer = P::Writer;

    unsafe fn register(&mut self, poll: &Arc<polling::Poller>, event: polling::Event, mode: polling::PollMode) -> std::io::Result<()> {
        self.reader.poller = Some(poll.clone());
        self.pty.register(poll, event, mode)
    }
    fn reregister(&mut self, poll: &Arc<polling::Poller>, event: polling::Event, mode: polling::PollMode) -> std::io::Result<()> {
        self.pty.reregister(poll, event, mode)
    }
    fn deregister(&mut self, poll: &Arc<polling::Poller>) -> std::io::Result<()> {
        self.pty.deregister(poll)
    }
    fn reader(&mut self) -> &mut ScanReader<P> {
        self.reader.pipe = self.pty.reader();
        &mut self.reader
    }
    fn writer(&mut self) -> &mut Self::Writer {
        self.pty.writer()
    }
}

impl<P: EventedPty> EventedPty for ScanPty<P> {
    fn next_child_event(&mut self) -> Option<ChildEvent> {
        self.pty.next_child_event()
    }
}

impl<P: EventedPty + OnResize> OnResize for ScanPty<P> {
    fn on_resize(&mut self, size: WindowSize) {
        self.pty.on_resize(size)
    }
}

/// How a session starts.
pub struct Start<'a> {
    /// The whole command line, as CreateProcess takes it.
    pub command: &'a str,
    pub dir: Option<PathBuf>,
    pub size: Size,
    pub scrollback: usize,
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
    /// Start a shell. `wake` is called from the pty thread whenever there is something new to
    /// draw.
    pub fn start(start: Start, wake: Arc<dyn Fn() + Send + Sync>) -> std::io::Result<Self> {
        let Start { command, dir, size, scrollback } = start;
        // The command line goes to CreateProcess as it was typed.
        let pty = crate::conpty::spawn(command, dir.as_deref(), size.into())?;
        Self::run(pty, program_name(command), size, scrollback, wake)
    }

    /// Show a console Windows handed over (default terminal): `program` names it until it sets
    /// a title.
    pub fn attach(pty: crate::pty::Pty, program: String, size: Size, scrollback: usize, wake: Arc<dyn Fn() + Send + Sync>) -> std::io::Result<Self> {
        Self::run(pty, program, size, scrollback, wake)
    }

    fn run<P: EventedPty + OnResize + Send + 'static>(pty: P, program: String, size: Size, scrollback: usize, wake: Arc<dyn Fn() + Send + Sync>) -> std::io::Result<Self> {
        let shared = Arc::new(Shared {
            dirty: AtomicBool::new(true),
            output_seq: AtomicU64::new(0),
            exited: AtomicBool::new(false),
            exit_code: Mutex::new(None),
            shown: AtomicBool::new(true),
            activity: AtomicBool::new(false),
            bell: AtomicBool::new(false),
            x10: AtomicBool::new(false),
            win32_input: AtomicBool::new(false),
            title: Mutex::new(None),
            cwd: Mutex::new(None),
            copy: Mutex::new(None),
            colours: Mutex::new(None),
            size: Mutex::new(size.into()),
        });
        let pty_slot = Arc::new(OnceLock::new());
        let listener = Listener { shared: shared.clone(), pty: pty_slot.clone(), wake };
        let config = Config {
            scrolling_history: scrollback,
            semantic_escape_chars: crate::select::WORD_STOPS.to_owned(),
            ..Config::default()
        };
        let term = Arc::new(FairMutex::new(Term::new(config, &size, listener.clone())));
        let reader = ScanReader { pipe: std::ptr::null_mut(), scanner: osc::Scanner::default(), shared: shared.clone(), given: 0, poller: None };
        let event_loop = EventLoop::new(term.clone(), listener, ScanPty { pty, reader }, true, false)?;
        let sender = event_loop.channel();
        let _ = pty_slot.set(sender.clone());
        event_loop.spawn();
        Ok(Self { term, pty: sender, shared, size, program })
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

    /// Send bytes without moving the view (mouse and focus reports).
    pub fn report(&self, bytes: &[u8]) {
        if !bytes.is_empty() {
            let _ = self.pty.send(Msg::Input(Cow::Owned(bytes.to_vec())));
        }
    }

    /// Fit the pseudo-console and the grid to a new size. Free when it is the same.
    pub fn resize(&mut self, size: Size) {
        if size == self.size {
            return;
        }
        let cells_changed = (size.cols, size.rows) != (self.size.cols, self.size.rows);
        self.size = size;
        *self.shared.size.lock().unwrap() = size.into();
        if cells_changed {
            self.term.lock().resize(size);
            let _ = self.pty.send(Msg::Resize(size.into()));
            self.shared.dirty.store(true, Ordering::Release);
        }
    }

    /// Keep `lines` of history from now on (Settings changed it).
    pub fn set_scrollback(&self, lines: usize) {
        let config = Config { scrolling_history: lines, semantic_escape_chars: crate::select::WORD_STOPS.to_owned(), ..Config::default() };
        self.term.lock().set_options(config);
        self.shared.dirty.store(true, Ordering::Release);
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

    /// Something changed that the next frame must draw (a selection, a hover).
    pub fn mark_dirty(&self) {
        self.shared.dirty.store(true, Ordering::Release);
    }

    pub fn exited(&self) -> bool {
        self.shared.exited.load(Ordering::Acquire)
    }

    pub fn exit_code(&self) -> Option<i32> {
        *self.shared.exit_code.lock().unwrap()
    }

    /// Show `text` (with escape sequences) as if the program had written it: a note of the
    /// terminal's own, such as that the program exited.
    pub fn print(&self, text: &str) {
        let mut parser: alacritty_terminal::vte::ansi::Processor = alacritty_terminal::vte::ansi::Processor::new();
        parser.advance(&mut *self.term.lock(), text.as_bytes());
        self.shared.dirty.store(true, Ordering::Release);
    }

    /// Whether this tab is the one shown. Showing it clears its activity and bell.
    pub fn set_shown(&self, shown: bool) {
        self.shared.shown.store(shown, Ordering::Release);
        if shown {
            self.shared.activity.store(false, Ordering::Release);
            self.shared.bell.store(false, Ordering::Release);
            self.shared.dirty.store(true, Ordering::Release);
        }
    }

    /// New output while not shown, and whether a bell rang (for the tab's mark).
    pub fn marks(&self) -> (bool, bool) {
        (self.shared.activity.load(Ordering::Acquire), self.shared.bell.load(Ordering::Acquire))
    }

    /// A bell that rang in the shown tab, once.
    pub fn take_bell(&self) -> bool {
        self.shared.bell.swap(false, Ordering::AcqRel)
    }

    pub fn x10(&self) -> bool {
        self.shared.x10.load(Ordering::Acquire)
    }

    /// Whether the console asked for win32-input-mode.
    pub fn win32_input(&self) -> bool {
        self.shared.win32_input.load(Ordering::Acquire)
    }

    /// What a program asked to copy, once.
    pub fn take_copy(&self) -> Option<String> {
        self.shared.copy.lock().unwrap().take()
    }

    /// The directory the shell last reported, if it reports one.
    pub fn cwd(&self) -> Option<String> {
        self.shared.cwd.lock().unwrap().clone()
    }

    /// The colours to answer colour queries with.
    pub fn set_colours(&self, colours: Colours) {
        *self.shared.colours.lock().unwrap() = Some(colours);
    }

    /// The title: what a program set, else the program's name.
    pub fn title(&self) -> String {
        match self.shared.title.lock().unwrap().as_deref().map(str::trim).filter(|t| !t.is_empty()) {
            Some(t) => short_title(t),
            None => self.program.clone(),
        }
    }

    pub fn mode(&self) -> TermMode {
        *self.term.lock().mode()
    }

    /// Read the emulator's state under its lock.
    pub fn with_term<R>(&self, f: impl FnOnce(&Term<Listener>) -> R) -> R {
        f(&self.term.lock())
    }

    /// How many times output came so far.
    pub fn output_seq(&self) -> u64 {
        self.shared.output_seq.load(Ordering::Acquire)
    }

    /// Change the emulator's state under its lock without asking for a redraw (find's counting,
    /// which only reads; a scroll it makes is seen by the view anyway).
    pub fn with_term_quiet<R>(&self, f: impl FnOnce(&mut Term<Listener>) -> R) -> R {
        f(&mut self.term.lock())
    }

    /// Change the emulator's state (the selection, the view) under its lock.
    pub fn with_term_mut<R>(&self, f: impl FnOnce(&mut Term<Listener>) -> R) -> R {
        let r = f(&mut self.term.lock());
        self.shared.dirty.store(true, Ordering::Release);
        r
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.pty.send(Msg::Shutdown);
    }
}

/// "powershell" from `C:\...\powershell.exe -NoLogo`: what the title says before a program sets one.
pub fn program_name(command: &str) -> String {
    let first = if let Some(rest) = command.trim_start().strip_prefix('"') {
        rest.split('"').next().unwrap_or(rest)
    } else {
        command.split_whitespace().next().unwrap_or(command)
    };
    let name = first.rsplit(['\\', '/']).next().unwrap_or(first);
    let name = name.strip_suffix(".exe").or_else(|| name.strip_suffix(".EXE")).unwrap_or(name);
    name.to_owned()
}

/// A console program's title as ConPTY reports it is the exe's whole path
/// (`C:\WINDOWS\system32\cmd.exe`, `Administrator: C:\...\powershell.exe`): just its name.
pub fn short_title(title: &str) -> String {
    let lower = title.to_ascii_lowercase();
    if !lower.ends_with(".exe") {
        return title.to_owned();
    }
    let b = title.as_bytes();
    let start = (0..b.len().saturating_sub(2)).find(|&i| b[i].is_ascii_alphabetic() && b[i + 1] == b':' && b[i + 2] == b'\\');
    match start {
        Some(i) => format!("{}{}", &title[..i], program_name(&format!("\"{}\"", &title[i..]))),
        None => title.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::{program_name, short_title};

    #[test]
    fn exe_paths_as_titles_become_names() {
        assert_eq!(short_title(r"C:\WINDOWS\system32\cmd.exe"), "cmd");
        assert_eq!(short_title(r"Administrator: C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe"), "Administrator: powershell");
        assert_eq!(short_title(r"C:\Program Files\Git\usr\bin\bash.exe"), "bash");
        assert_eq!(short_title("vim README.md"), "vim README.md");
        assert_eq!(short_title("me@debian: ~/src"), "me@debian: ~/src");
    }

    #[test]
    fn program_names() {
        assert_eq!(program_name("powershell.exe"), "powershell");
        assert_eq!(program_name(r"C:\Windows\System32\cmd.exe /k echo hi"), "cmd");
        assert_eq!(program_name(r#""C:\Program Files\PowerShell\7\pwsh.exe" -NoLogo"#), "pwsh");
        assert_eq!(program_name("wsl"), "wsl");
    }
}
