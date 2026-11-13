//! A pseudo-console handed over by Windows (default terminal), driven like `alacritty_terminal`'s
//! own ConPTY.
//!
//! When a console program starts without a terminal, Windows' console host hands the session to
//! the delegation console (Windows Terminal's OpenConsole), which runs it as a pseudo-console and
//! calls the delegation terminal (`defterm`) with: pipes for its input and output (we make them;
//! `ITerminalHandoff3` returns them), its signal pipe, a reference handle that keeps it alive, its
//! own process and the client process. That is everything `CreatePseudoConsole` would have given
//! us, except that the console already runs, so this type does what `alacritty_terminal`'s `Pty`
//! does over those handles:
//!
//! - output is read on a thread into a buffer, and the event loop is told it is readable (a posted
//!   completion packet, as `alacritty_terminal` does on Windows);
//! - input is queued to a writer thread, so a console that does not read never blocks the loop;
//! - a resize is ConPTY's signal packet (`PTY_SIGNAL_RESIZE_WINDOW`, see [`resize_packet`]);
//! - the program's end is the client process ending (or the console's), on a waiting thread;
//! - dropping it closes the signal pipe, the reference and the console's process handle, in
//!   `ClosePseudoConsole`'s order, so the console sends CTRL_CLOSE to its programs and exits.

use alacritty_terminal::event::{OnResize, WindowSize};
use alacritty_terminal::tty::{ChildEvent, EventedPty, EventedReadWrite, PTY_CHILD_EVENT_TOKEN};
use polling::os::iocp::{CompletionPacket, PollerIocpExt};
use polling::{Event, PollMode, Poller};
use std::collections::VecDeque;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::windows::io::{AsRawHandle, OwnedHandle};
use std::os::windows::process::ExitStatusExt;
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};

/// ConPTY's signal codes (`winconpty.h`).
pub const PTY_SIGNAL_RESIZE_WINDOW: u16 = 8;

/// Output held for the parser before the reader thread waits (the console then waits too).
const BUFFER: usize = 1 << 20;

/// The 6-byte resize packet ConPTY's `ResizePseudoConsole` writes to the signal pipe: three
/// little-endian u16s, the code, columns, rows.
pub fn resize_packet(cols: u16, rows: u16) -> [u8; 6] {
    let mut p = [0u8; 6];
    p[0..2].copy_from_slice(&PTY_SIGNAL_RESIZE_WINDOW.to_le_bytes());
    p[2..4].copy_from_slice(&cols.max(1).min(i16::MAX as u16).to_le_bytes());
    p[4..6].copy_from_slice(&rows.max(1).min(i16::MAX as u16).to_le_bytes());
    p
}

/// What the handoff gave us, already duplicated into handles we own.
pub struct Handles {
    /// We write the program's input here.
    pub input: OwnedHandle,
    /// We read its output here.
    pub output: OwnedHandle,
    pub signal: OwnedHandle,
    pub reference: OwnedHandle,
    /// The console host (OpenConsole).
    pub server: OwnedHandle,
    /// The console program.
    pub client: OwnedHandle,
}

struct Interest {
    poller: Arc<Poller>,
    event: Event,
    mode: PollMode,
}

#[derive(Default)]
struct Slot(Mutex<Option<Interest>>);

impl Slot {
    /// Tell the event loop, if it is listening for this kind of event.
    fn post(&self, want: impl Fn(&Event) -> bool) {
        let mut slot = self.0.lock().unwrap();
        if let Some(i) = slot.as_ref() {
            if want(&i.event) {
                let _ = i.poller.post(CompletionPacket::new(i.event));
                if matches!(i.mode, PollMode::Oneshot | PollMode::EdgeOneshot) {
                    *slot = None;
                }
            }
        }
    }
}

struct Inbuf {
    data: VecDeque<u8>,
    closed: bool,
}

pub struct Reader {
    buf: Arc<(Mutex<Inbuf>, Condvar)>,
}

impl Read for Reader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let (lock, room) = &*self.buf;
        let mut b = lock.lock().unwrap();
        let n = out.len().min(b.data.len());
        for (dst, src) in out.iter_mut().zip(b.data.drain(..n)) {
            *dst = src;
        }
        if n > 0 {
            room.notify_all();
        }
        Ok(n)
    }
}

pub struct Writer {
    tx: Option<mpsc::Sender<Vec<u8>>>,
}

impl Write for Writer {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if let Some(tx) = &self.tx {
            let _ = tx.send(buf.to_vec());
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub struct HandoffPty {
    // Field order is drop order: the console is let go before the pipes' threads are.
    signal: File,
    reference: Option<OwnedHandle>,
    server: Option<OwnedHandle>,
    stop: OwnedHandle,
    reader: Reader,
    writer: Writer,
    read_slot: Arc<Slot>,
    write_slot: Arc<Slot>,
    child_slot: Arc<Slot>,
    child_rx: mpsc::Receiver<ChildEvent>,
    first_register: bool,
}

// SAFETY: every handle here is owned and used from one thread at a time (the event loop's).
unsafe impl Send for HandoffPty {}

impl HandoffPty {
    /// Take over the handed-over console, sized `size`.
    pub fn new(h: Handles, size: WindowSize) -> io::Result<Self> {
        let read_slot = Arc::new(Slot::default());
        let write_slot = Arc::new(Slot::default());
        let child_slot = Arc::new(Slot::default());

        // Output.
        let buf = Arc::new((Mutex::new(Inbuf { data: VecDeque::new(), closed: false }), Condvar::new()));
        {
            let buf = buf.clone();
            let slot = read_slot.clone();
            let mut pipe = File::from(h.output);
            std::thread::Builder::new().name("handoff-read".into()).spawn(move || {
                let mut chunk = vec![0u8; 64 * 1024];
                loop {
                    let n = pipe.read(&mut chunk).unwrap_or(0);
                    let (lock, room) = &*buf;
                    let mut b = lock.lock().unwrap();
                    if n == 0 {
                        b.closed = true;
                        drop(b);
                        slot.post(|e| e.readable);
                        return;
                    }
                    while b.data.len() >= BUFFER {
                        b = room.wait(b).unwrap();
                    }
                    b.data.extend(&chunk[..n]);
                    drop(b);
                    slot.post(|e| e.readable);
                }
            })?;
        }

        // Input.
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        {
            let mut pipe = File::from(h.input);
            std::thread::Builder::new().name("handoff-write".into()).spawn(move || {
                while let Ok(bytes) = rx.recv() {
                    if pipe.write_all(&bytes).is_err() {
                        return;
                    }
                }
            })?;
        }

        // The program's end (or the console's).
        let stop = event()?;
        let (child_tx, child_rx) = mpsc::channel();
        {
            let slot = child_slot.clone();
            let client = h.client;
            let server = h.server.try_clone()?;
            let stop = stop.try_clone()?;
            std::thread::Builder::new().name("handoff-child".into()).spawn(move || {
                if let Some(code) = wait_exit(&client, &server, &stop) {
                    let _ = child_tx.send(ChildEvent::Exited(code.map(std::process::ExitStatus::from_raw)));
                    slot.post(|_| true);
                }
            })?;
        }

        let mut pty = Self {
            signal: File::from(h.signal),
            reference: Some(h.reference),
            server: Some(h.server),
            stop,
            reader: Reader { buf },
            writer: Writer { tx: Some(tx) },
            read_slot,
            write_slot,
            child_slot,
            child_rx,
            first_register: true,
        };
        // The console starts at whatever size it had: make it ours.
        pty.on_resize(size);
        Ok(pty)
    }

    fn has_output(&self) -> bool {
        let b = self.reader.buf.0.lock().unwrap();
        !b.data.is_empty() || b.closed
    }

    fn set_interest(&mut self, poll: &Arc<Poller>, event: Event, mode: PollMode) {
        let mut rw = event;
        rw.key = alacritty_terminal::tty::PTY_READ_WRITE_TOKEN;
        *self.read_slot.0.lock().unwrap() = Some(Interest { poller: poll.clone(), event: rw, mode });
        *self.write_slot.0.lock().unwrap() = Some(Interest { poller: poll.clone(), event: rw, mode });
        let child = Event::readable(PTY_CHILD_EVENT_TOKEN);
        *self.child_slot.0.lock().unwrap() = Some(Interest { poller: poll.clone(), event: child, mode: PollMode::Level });
        // Start the loop off: output waiting, room to write (always), or an exit already seen.
        let first = std::mem::take(&mut self.first_register);
        if (rw.readable && (first || self.has_output())) || rw.writable {
            let _ = poll.post(CompletionPacket::new(rw));
        }
    }
}

/// Wait for the client (or the console) to end, or `stop`. The client's exit code, None when
/// `stop` came first; Some(None) when the code is unknown.
fn wait_exit(client: &OwnedHandle, server: &OwnedHandle, stop: &OwnedHandle) -> Option<Option<u32>> {
    use windows::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForMultipleObjects, INFINITE};
    let hs = [HANDLE(client.as_raw_handle()), HANDLE(server.as_raw_handle()), HANDLE(stop.as_raw_handle())];
    let r = unsafe { WaitForMultipleObjects(&hs, false, INFINITE) };
    if r.0 == WAIT_OBJECT_0.0 + 2 {
        return None;
    }
    let mut code = 0u32;
    let known = unsafe { GetExitCodeProcess(hs[0], &mut code) }.is_ok() && code != 259; // STILL_ACTIVE
    Some(known.then_some(code))
}

fn event() -> io::Result<OwnedHandle> {
    use std::os::windows::io::FromRawHandle;
    let h = unsafe { windows::Win32::System::Threading::CreateEventW(None, true, false, None) }.map_err(io::Error::other)?;
    Ok(unsafe { OwnedHandle::from_raw_handle(h.0) })
}

impl EventedReadWrite for HandoffPty {
    type Reader = Reader;
    type Writer = Writer;

    unsafe fn register(&mut self, poll: &Arc<Poller>, event: Event, mode: PollMode) -> io::Result<()> {
        self.set_interest(poll, event, mode);
        Ok(())
    }
    fn reregister(&mut self, poll: &Arc<Poller>, event: Event, mode: PollMode) -> io::Result<()> {
        self.set_interest(poll, event, mode);
        Ok(())
    }
    fn deregister(&mut self, _poll: &Arc<Poller>) -> io::Result<()> {
        *self.read_slot.0.lock().unwrap() = None;
        *self.write_slot.0.lock().unwrap() = None;
        *self.child_slot.0.lock().unwrap() = None;
        Ok(())
    }
    fn reader(&mut self) -> &mut Reader {
        &mut self.reader
    }
    fn writer(&mut self) -> &mut Writer {
        &mut self.writer
    }
}

impl EventedPty for HandoffPty {
    fn next_child_event(&mut self) -> Option<ChildEvent> {
        self.child_rx.try_recv().ok()
    }
}

impl OnResize for HandoffPty {
    fn on_resize(&mut self, size: WindowSize) {
        let _ = self.signal.write_all(&resize_packet(size.num_cols, size.num_lines));
    }
}

impl Drop for HandoffPty {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::System::Threading::SetEvent(windows::Win32::Foundation::HANDLE(self.stop.as_raw_handle()));
        }
        // As ClosePseudoConsole: the signal pipe (dropped with `self`), the reference, then the
        // console's process. The input thread ends when its sender goes.
        self.reference = None;
        self.server = None;
        self.writer.tx = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resize_packets_are_conpty_s() {
        assert_eq!(resize_packet(120, 30), [8, 0, 120, 0, 30, 0]);
        assert_eq!(resize_packet(300, 1000), [8, 0, 0x2C, 0x01, 0xE8, 0x03]);
        // Never zero, never past SHORT_MAX.
        assert_eq!(resize_packet(0, 0), [8, 0, 1, 0, 1, 0]);
        assert_eq!(resize_packet(u16::MAX, 2), [8, 0, 0xFF, 0x7F, 2, 0]);
    }

    /// Two pipes and this process standing in for the console and its program: output read
    /// arrives through the reader, input reaches the console's end, a resize is a signal packet,
    /// and nothing blocks.
    #[test]
    fn a_handed_over_console_is_driven_through_its_handles() {
        use std::os::windows::io::FromRawHandle;
        fn pipe() -> (File, OwnedHandle) {
            let (mut r, mut w) = (windows::Win32::Foundation::HANDLE::default(), windows::Win32::Foundation::HANDLE::default());
            unsafe { windows::Win32::System::Pipes::CreatePipe(&mut r, &mut w, None, 0) }.unwrap();
            unsafe { (File::from_raw_handle(r.0), OwnedHandle::from_raw_handle(w.0)) }
        }
        let (mut console_in, input) = pipe(); // we write input; the "console" reads it
        let (output, console_out) = pipe(); // the "console" writes output; we read it
        let (mut signal_read, signal) = pipe();
        let mut console_out = File::from(console_out);
        // Stand-ins that stay alive: this process for the console, a long-lived child for the client.
        let this = unsafe {
            let mut h = windows::Win32::Foundation::HANDLE::default();
            let cur = windows::Win32::System::Threading::GetCurrentProcess();
            windows::Win32::Foundation::DuplicateHandle(cur, cur, cur, &mut h, 0, false, windows::Win32::Foundation::DUPLICATE_SAME_ACCESS).unwrap();
            OwnedHandle::from_raw_handle(h.0)
        };
        let mut client = std::process::Command::new("ping.exe")
            .args(["-n", "30", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let client_handle = std::os::windows::io::AsHandle::as_handle(&client).try_clone_to_owned().unwrap();
        let handles = Handles {
            input,
            output: OwnedHandle::from(output),
            signal,
            reference: this.try_clone().unwrap(),
            server: this,
            client: client_handle,
        };
        let size = WindowSize { num_lines: 30, num_cols: 120, cell_width: 8, cell_height: 16 };
        let mut pty = HandoffPty::new(handles, size).unwrap();
        let mut sig = [0u8; 6];
        signal_read.read_exact(&mut sig).unwrap();
        assert_eq!(sig, resize_packet(120, 30));

        console_out.write_all(b"hello").unwrap();
        let mut got: Vec<u8> = Vec::new();
        for _ in 0..200 {
            let mut b = [0u8; 16];
            let n = pty.reader().read(&mut b).unwrap();
            got.extend(&b[..n]);
            if got.len() >= 5 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(got, b"hello");

        pty.writer().write_all(b"dir\r").unwrap();
        let mut typed = [0u8; 4];
        console_in.read_exact(&mut typed).unwrap();
        assert_eq!(&typed, b"dir\r");

        // The program ends: an exit with its code.
        client.kill().unwrap();
        let mut event = None;
        for _ in 0..200 {
            event = pty.next_child_event();
            if event.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(matches!(event, Some(ChildEvent::Exited(Some(_)))));
        drop(pty);
    }
}
