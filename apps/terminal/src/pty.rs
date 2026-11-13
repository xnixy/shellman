//! A pseudo-console driven as `alacritty_terminal`'s event loop wants a pty: one we created
//! (`conpty::spawn`, through the bundled `conpty.dll` or Windows' own), or one Windows handed over
//! (default terminal, `defterm`).
//!
//! Either way it comes down to the same handles: the console's input and output pipes, the
//! program's process, and a way to resize and close the console. This type does over them what
//! `alacritty_terminal`'s own `Pty` does:
//!
//! - output is read on a thread into a buffer, and the event loop is told it is readable (a posted
//!   completion packet, as `alacritty_terminal` does on Windows);
//! - input is queued to a writer thread, so a console that does not read never blocks the loop;
//!   at most [`MAX_QUEUED`] bytes wait there: past that the writer says "would block" and the
//!   event loop keeps the rest (a huge paste into a program that reads slowly is held once, in
//!   the loop's own queue, not copied again here), and is told it may write again once the
//!   console took some. Nothing is dropped;
//! - a resize is `ResizePseudoConsole`, or for a handed-over console ConPTY's signal packet
//!   (`PTY_SIGNAL_RESIZE_WINDOW`, see [`resize_packet`]);
//! - the program's end is its process ending (or the console's). The end is told only once the
//!   console's output has all been read and handed to the parser: a program that prints and
//!   exits at once loses nothing (the console host is released at the start, so it exits after
//!   its last program and its output pipe then breaks; see [`drain`]);
//! - dropping it closes the console (`ClosePseudoConsole`, or for a handed-over one the signal
//!   pipe, the reference and the console's process in that order), so the console sends
//!   CTRL_CLOSE to its programs and exits.

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
use std::time::{Duration, Instant};

/// ConPTY's signal codes (`winconpty.h`).
pub const PTY_SIGNAL_RESIZE_WINDOW: u16 = 8;

/// Output held for the parser before the reader thread waits (the console then waits too).
const BUFFER: usize = 1 << 20;
/// Input queued for the console before the writer pushes back.
pub const MAX_QUEUED: usize = 1 << 20;
/// The most taken in one write.
const CHUNK: usize = 64 * 1024;

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

/// The console itself, for resizing and closing it.
pub enum Console {
    /// Handed over by Windows: its signal pipe, the reference that keeps it alive, its process.
    Handoff { signal: File, reference: Option<OwnedHandle>, server: Option<OwnedHandle> },
    /// Made by `conpty::spawn`.
    Created { api: &'static crate::conpty::Api, hpc: windows::Win32::System::Console::HPCON },
    Closed,
}

impl Console {
    fn resize(&mut self, cols: u16, rows: u16) {
        match self {
            Console::Handoff { signal, .. } => {
                let _ = signal.write_all(&resize_packet(cols, rows));
            }
            Console::Created { api, hpc } => api.resize(*hpc, cols, rows),
            Console::Closed => {}
        }
    }

    fn close(&mut self) {
        match std::mem::replace(self, Console::Closed) {
            // As ClosePseudoConsole: the signal pipe, the reference, then the console's process.
            Console::Handoff { signal, reference, server } => {
                drop(signal);
                drop(reference);
                drop(server);
            }
            Console::Created { api, hpc } => api.close(hpc),
            Console::Closed => {}
        }
    }
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

/// The console's output on its way from the reader thread to the parser.
#[derive(Default)]
struct Inbuf {
    data: VecDeque<u8>,
    /// The pipe broke: the console exited and everything it wrote is in `data`.
    closed: bool,
    /// Bytes ever received (to see output go quiet).
    total: u64,
    /// The pty is being dropped: output is thrown away rather than waited on.
    discard: bool,
}

type Shared = Arc<(Mutex<Inbuf>, Condvar)>;

pub struct Reader {
    buf: Shared,
}

impl Read for Reader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let (lock, cv) = &*self.buf;
        let mut b = lock.lock().unwrap();
        let n = out.len().min(b.data.len());
        for (dst, src) in out.iter_mut().zip(b.data.drain(..n)) {
            *dst = src;
        }
        if n > 0 || b.data.is_empty() {
            cv.notify_all();
        }
        Ok(n)
    }
}

/// Input on its way to the console: bytes queued, and whether the console stopped taking any.
#[derive(Default)]
struct Outq {
    queued: std::sync::atomic::AtomicUsize,
    gone: std::sync::atomic::AtomicBool,
}

impl Outq {
    fn has_room(&self) -> bool {
        self.queued.load(std::sync::atomic::Ordering::SeqCst) < MAX_QUEUED
    }
}

pub struct Writer {
    tx: Option<mpsc::Sender<Vec<u8>>>,
    out: Arc<Outq>,
}

impl Write for Writer {
    /// Takes up to [`CHUNK`] bytes while fewer than [`MAX_QUEUED`] wait; "would block" when that
    /// many do (the event loop keeps the rest and writes it when told there is room).
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        use std::sync::atomic::Ordering::SeqCst;
        let Some(tx) = &self.tx else { return Ok(buf.len()) };
        if buf.is_empty() || self.out.gone.load(SeqCst) {
            // The console is gone: what is typed has nowhere to go.
            return Ok(buf.len());
        }
        let queued = self.out.queued.load(SeqCst);
        if queued >= MAX_QUEUED {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        let n = buf.len().min(MAX_QUEUED - queued).min(CHUNK);
        self.out.queued.fetch_add(n, SeqCst);
        if tx.send(buf[..n].to_vec()).is_err() {
            self.out.gone.store(true, SeqCst);
            return Ok(buf.len());
        }
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub struct Pty {
    console: Console,
    stop: OwnedHandle,
    reader: Reader,
    writer: Writer,
    read_slot: Arc<Slot>,
    write_slot: Arc<Slot>,
    child_slot: Arc<Slot>,
    child_rx: mpsc::Receiver<ChildEvent>,
    first_register: bool,
}

// SAFETY: every handle here is owned and used from one thread at a time (the event loop's); the
// pseudo-console handle may be used from any thread.
unsafe impl Send for Pty {}

/// How the end of the program is told apart from output still on its way.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ending {
    /// The console host was released: it exits after its last program and its pipe breaks, so
    /// the end waits for that (up to a bound, for a program that left a child on the console).
    PipeBreaks,
    /// Nothing tells when the console has written everything: the end waits for the output to
    /// go quiet.
    Quiet,
}

impl Pty {
    /// Take over a console Windows handed over, sized `size`.
    pub fn handoff(h: Handles, size: WindowSize) -> io::Result<Self> {
        let watch = h.server.try_clone()?;
        let console = Console::Handoff { signal: File::from(h.signal), reference: Some(h.reference), server: Some(h.server) };
        let mut pty = Self::new(console, h.output, h.input, h.client, Some(watch), Ending::Quiet)?;
        // The console starts at whatever size it had: make it ours.
        pty.on_resize(size);
        Ok(pty)
    }

    /// Drive `console` over its pipes; `client` is the program, `server` the console's process
    /// when it is known (its end ends the session too).
    pub fn new(console: Console, output: OwnedHandle, input: OwnedHandle, client: OwnedHandle, server: Option<OwnedHandle>, ending: Ending) -> io::Result<Self> {
        let read_slot = Arc::new(Slot::default());
        let write_slot = Arc::new(Slot::default());
        let child_slot = Arc::new(Slot::default());

        // Output.
        let buf: Shared = Arc::new((Mutex::new(Inbuf::default()), Condvar::new()));
        {
            let buf = buf.clone();
            let slot = read_slot.clone();
            let mut pipe = File::from(output);
            std::thread::Builder::new().name("pty-read".into()).spawn(move || {
                let mut chunk = vec![0u8; 64 * 1024];
                loop {
                    let n = pipe.read(&mut chunk).unwrap_or(0);
                    let (lock, cv) = &*buf;
                    let mut b = lock.lock().unwrap();
                    if n == 0 {
                        b.closed = true;
                        cv.notify_all();
                        drop(b);
                        slot.post(|e| e.readable);
                        return;
                    }
                    while b.data.len() >= BUFFER && !b.discard {
                        b = cv.wait(b).unwrap();
                    }
                    b.total += n as u64;
                    if !b.discard {
                        b.data.extend(&chunk[..n]);
                    }
                    cv.notify_all();
                    drop(b);
                    slot.post(|e| e.readable);
                }
            })?;
        }

        // Input.
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        let out = Arc::new(Outq::default());
        {
            let mut pipe = File::from(input);
            let out = out.clone();
            let slot = write_slot.clone();
            std::thread::Builder::new().name("pty-write".into()).spawn(move || {
                use std::sync::atomic::Ordering::SeqCst;
                while let Ok(bytes) = rx.recv() {
                    if pipe.write_all(&bytes).is_err() {
                        out.gone.store(true, SeqCst);
                        // The loop may be waiting for room: there is (it all goes nowhere now).
                        slot.post(|e| e.writable);
                        return;
                    }
                    let before = out.queued.fetch_sub(bytes.len(), SeqCst);
                    // The writer said "would block" at the cap: tell the loop there is room.
                    if before >= MAX_QUEUED && before - bytes.len() < MAX_QUEUED {
                        slot.post(|e| e.writable);
                    }
                }
            })?;
        }

        // The program's end (or the console's), told once its output is all in.
        let stop = event()?;
        let (child_tx, child_rx) = mpsc::channel();
        {
            let slot = child_slot.clone();
            let stop = stop.try_clone()?;
            let buf = buf.clone();
            std::thread::Builder::new().name("pty-child".into()).spawn(move || {
                if let Some(code) = wait_exit(&client, server.as_ref(), &stop) {
                    drain(&buf, ending, Duration::from_secs(3));
                    let _ = child_tx.send(ChildEvent::Exited(code.map(std::process::ExitStatus::from_raw)));
                    slot.post(|_| true);
                }
            })?;
        }

        Ok(Self {
            console,
            stop,
            reader: Reader { buf },
            writer: Writer { tx: Some(tx), out },
            read_slot,
            write_slot,
            child_slot,
            child_rx,
            first_register: true,
        })
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
        // Start the loop off: output waiting, or room to write (else the writer thread says when
        // there is: it checks after this interest is in place, so no wake-up is lost).
        let first = std::mem::take(&mut self.first_register);
        let room = self.writer.out.has_room() || self.writer.out.gone.load(std::sync::atomic::Ordering::SeqCst);
        if (rw.readable && (first || self.has_output())) || (rw.writable && room) {
            let _ = poll.post(CompletionPacket::new(rw));
        }
    }
}

/// After the program ended: wait until the console has written all it will (its pipe broke, or
/// for `Ending::Quiet` no output came for a moment; at most `bound`), then until the parser has
/// taken it all, so the event loop, which reads once more and stops when it hears of the end,
/// misses nothing.
fn drain(buf: &Shared, ending: Ending, bound: Duration) {
    const QUIET: Duration = Duration::from_millis(120);
    let (lock, cv) = &**buf;
    let start = Instant::now();
    let mut b = lock.lock().unwrap();
    let bound = if ending == Ending::Quiet { bound.min(Duration::from_secs(1)) } else { bound };
    while !b.closed && !b.discard && start.elapsed() < bound {
        let before = b.total;
        b = cv.wait_timeout(b, QUIET).unwrap().0;
        if ending == Ending::Quiet && b.total == before && b.data.is_empty() {
            break;
        }
    }
    let start = Instant::now();
    while !b.data.is_empty() && !b.discard && start.elapsed() < Duration::from_secs(5) {
        b = cv.wait_timeout(b, Duration::from_millis(50)).unwrap().0;
    }
}

/// Wait for the client (or the console) to end, or `stop`. The client's exit code, None when
/// `stop` came first; Some(None) when the code is unknown.
fn wait_exit(client: &OwnedHandle, server: Option<&OwnedHandle>, stop: &OwnedHandle) -> Option<Option<u32>> {
    use windows::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForMultipleObjects, INFINITE};
    let mut hs = vec![HANDLE(stop.as_raw_handle()), HANDLE(client.as_raw_handle())];
    if let Some(s) = server {
        hs.push(HANDLE(s.as_raw_handle()));
    }
    let r = unsafe { WaitForMultipleObjects(&hs, false, INFINITE) };
    if r.0 == WAIT_OBJECT_0.0 {
        return None;
    }
    let mut code = 0u32;
    let known = unsafe { GetExitCodeProcess(hs[1], &mut code) }.is_ok() && code != 259; // STILL_ACTIVE
    Some(known.then_some(code))
}

fn event() -> io::Result<OwnedHandle> {
    use std::os::windows::io::FromRawHandle;
    let h = unsafe { windows::Win32::System::Threading::CreateEventW(None, true, false, None) }.map_err(io::Error::other)?;
    Ok(unsafe { OwnedHandle::from_raw_handle(h.0) })
}

impl EventedReadWrite for Pty {
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

impl EventedPty for Pty {
    fn next_child_event(&mut self) -> Option<ChildEvent> {
        self.child_rx.try_recv().ok()
    }
}

impl OnResize for Pty {
    fn on_resize(&mut self, size: WindowSize) {
        self.console.resize(size.num_cols, size.num_lines);
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::System::Threading::SetEvent(windows::Win32::Foundation::HANDLE(self.stop.as_raw_handle()));
        }
        // Nobody reads any more: the reader thread throws output away, so a console that flushes
        // on its way out never waits on us.
        {
            let (lock, cv) = &*self.reader.buf;
            lock.lock().unwrap().discard = true;
            cv.notify_all();
        }
        // The input thread ends when its sender goes.
        self.writer.tx = None;
        self.console.close();
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

    fn pipe() -> (File, OwnedHandle) {
        use std::os::windows::io::FromRawHandle;
        let (mut r, mut w) = (windows::Win32::Foundation::HANDLE::default(), windows::Win32::Foundation::HANDLE::default());
        unsafe { windows::Win32::System::Pipes::CreatePipe(&mut r, &mut w, None, 0) }.unwrap();
        unsafe { (File::from_raw_handle(r.0), OwnedHandle::from_raw_handle(w.0)) }
    }

    fn this_process() -> OwnedHandle {
        use std::os::windows::io::FromRawHandle;
        unsafe {
            let mut h = windows::Win32::Foundation::HANDLE::default();
            let cur = windows::Win32::System::Threading::GetCurrentProcess();
            windows::Win32::Foundation::DuplicateHandle(cur, cur, cur, &mut h, 0, false, windows::Win32::Foundation::DUPLICATE_SAME_ACCESS).unwrap();
            OwnedHandle::from_raw_handle(h.0)
        }
    }

    /// Two pipes and this process standing in for the console and its program: output read
    /// arrives through the reader, input reaches the console's end, a resize is a signal packet,
    /// and nothing blocks.
    #[test]
    fn a_handed_over_console_is_driven_through_its_handles() {
        let (mut console_in, input) = pipe(); // we write input; the "console" reads it
        let (output, console_out) = pipe(); // the "console" writes output; we read it
        let (mut signal_read, signal) = pipe();
        let mut console_out = File::from(console_out);
        // Stand-ins that stay alive: this process for the console, a long-lived child for the client.
        let this = this_process();
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
        let mut pty = Pty::handoff(handles, size).unwrap();
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
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(got, b"hello");

        pty.writer().write_all(b"dir\r").unwrap();
        let mut typed = [0u8; 4];
        console_in.read_exact(&mut typed).unwrap();
        assert_eq!(&typed, b"dir\r");

        // The program ends: an exit with its code.
        client.kill().unwrap();
        let mut event = None;
        for _ in 0..400 {
            event = pty.next_child_event();
            if event.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(matches!(event, Some(ChildEvent::Exited(Some(_)))));
        drop(pty);
    }

    /// A huge paste into a console that does not read: at most `MAX_QUEUED` (and a chunk) is
    /// taken, the rest is refused as "would block" for the event loop to keep; once the console
    /// reads, every byte arrives, in order.
    #[test]
    fn input_waits_for_the_console_instead_of_piling_up() {
        let (output, _console_out) = pipe();
        let (mut console_in, input) = pipe();
        let client = std::process::Command::new("ping.exe").args(["-n", "30", "127.0.0.1"]).stdout(std::process::Stdio::null()).spawn().unwrap();
        let client_handle = std::os::windows::io::AsHandle::as_handle(&client).try_clone_to_owned().unwrap();
        let mut pty = Pty::new(Console::Closed, OwnedHandle::from(output), input, client_handle, None, Ending::PipeBreaks).unwrap();
        let paste: Vec<u8> = (0..16 * 1024 * 1024u32).map(|i| (i % 251) as u8).collect();
        // Nobody reads: the writer takes what it may, then pushes back.
        let mut sent = 0;
        let mut refused = false;
        for _ in 0..10_000 {
            match pty.writer().write(&paste[sent..]) {
                Ok(n) => sent += n,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    refused = true;
                    break;
                }
                Err(e) => panic!("{e}"),
            }
        }
        assert!(refused, "the queue never pushed back ({sent} bytes taken)");
        // What was taken: the queue and what the pipe itself holds, nowhere near the paste.
        assert!(sent <= MAX_QUEUED + 2 * CHUNK + 256 * 1024, "{sent} bytes taken");
        // The console reads it all; the rest is written as room comes (as the loop does).
        let reader = std::thread::spawn(move || {
            let mut got = Vec::with_capacity(16 * 1024 * 1024);
            let mut buf = vec![0u8; 256 * 1024];
            while got.len() < 16 * 1024 * 1024 {
                let n = console_in.read(&mut buf).unwrap();
                got.extend_from_slice(&buf[..n]);
            }
            got
        });
        let start = Instant::now();
        while sent < paste.len() && start.elapsed() < Duration::from_secs(30) {
            match pty.writer().write(&paste[sent..]) {
                Ok(n) => sent += n,
                Err(_) => std::thread::sleep(Duration::from_millis(1)),
            }
        }
        let got = reader.join().unwrap();
        assert!(got == paste, "the paste arrived changed or incomplete");
        drop(pty);
        let mut client = client;
        let _ = client.kill();
    }

    /// A program that writes a lot and exits at once: its end is told only after the last byte
    /// was read, so nothing it wrote is lost (stage 2 lost the last line).
    #[test]
    fn the_end_waits_for_the_last_output() {
        let (output, console_out) = pipe();
        let (_console_in, input) = pipe();
        let client = std::process::Command::new("cmd.exe").args(["/c", "exit 3"]).spawn().unwrap();
        let client_handle = std::os::windows::io::AsHandle::as_handle(&client).try_clone_to_owned().unwrap();
        let mut pty = Pty::new(Console::Closed, OwnedHandle::from(output), input, client_handle, None, Ending::PipeBreaks).unwrap();
        // The "console" writes 3 MB (more than the buffer holds) after the program is gone, then
        // exits (its pipe breaks).
        let writer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            let mut console_out = File::from(console_out);
            let block = vec![b'x'; 64 * 1024];
            for _ in 0..48 {
                console_out.write_all(&block).unwrap();
            }
            console_out.write_all(b"LAST").unwrap();
        });
        let mut got = 0usize;
        let mut tail = Vec::new();
        let mut ended = false;
        let mut buf = vec![0u8; 64 * 1024];
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(10) {
            let n = pty.reader().read(&mut buf).unwrap();
            got += n;
            tail.extend_from_slice(&buf[..n]);
            if tail.len() > 8 {
                tail.drain(..tail.len() - 8);
            }
            if pty.next_child_event().is_some() {
                // Once the end is told, what remains is at most what one more read takes.
                let n = pty.reader().read(&mut buf).unwrap();
                got += n;
                tail.extend_from_slice(&buf[..n]);
                ended = true;
                break;
            }
            if n == 0 {
                std::thread::sleep(Duration::from_millis(2));
            }
        }
        writer.join().unwrap();
        assert!(ended);
        assert_eq!(got, 48 * 64 * 1024 + 4);
        assert!(tail.ends_with(b"LAST"));
    }
}
