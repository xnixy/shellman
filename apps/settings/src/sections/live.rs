//! A page's worker for Windows settings it reads and writes: one thread that makes each change and
//! then reads everything again, in order, so a reading never runs ahead of a write. The page shows
//! what it expects at once and takes Windows' reading when it comes. Shared by Accessibility and
//! Privacy & security.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

type Read<S> = Box<dyn FnMut() -> S + Send>;
type Write<J> = Box<dyn FnMut(J) -> Result<(), String> + Send>;

/// The worker and what it last read.
pub struct Live<S, J> {
    jobs: Option<Sender<Option<J>>>,
    out: Option<Receiver<(S, Option<String>)>>,
    /// What the page shows: Windows' last reading, with the changes made since applied.
    pub shown: Option<S>,
    /// A change Windows refused, in a line for the page.
    pub notice: Option<String>,
    asked: Option<Instant>,
}

impl<S, J> Default for Live<S, J> {
    fn default() -> Self {
        Self { jobs: None, out: None, shown: None, notice: None, asked: None }
    }
}

impl<S: Send + 'static, J: Send + 'static> Live<S, J> {
    /// Start the worker (once) and ask for a first reading.
    pub fn start(&mut self, ctx: &egui::Context, mut read: Read<S>, mut write: Write<J>) {
        if self.jobs.is_some() {
            return;
        }
        let (jobs, rx) = channel::<Option<J>>();
        let (tx, out) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            // Some readings ask WinRT (package names).
            // SAFETY: plain COM set-up for this thread.
            unsafe {
                let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
            }
            while let Ok(job) = rx.recv() {
                let mut notice = None;
                let mut pending = Some(job);
                // Make every queued change, then read once.
                while let Some(job) = pending.take() {
                    if let Some(j) = job {
                        if let Err(e) = write(j) {
                            notice = Some(e);
                        }
                    }
                    pending = rx.try_recv().ok();
                }
                if tx.send((read(), notice)).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
        });
        self.jobs = Some(jobs);
        self.out = Some(out);
        self.ask();
    }

    fn ask(&mut self) {
        if let Some(j) = &self.jobs {
            let _ = j.send(None);
        }
        self.asked = Some(Instant::now());
    }

    /// Take what the worker read.
    pub fn poll(&mut self) {
        let Some(out) = &self.out else { return };
        while let Ok((s, notice)) = out.try_recv() {
            self.shown = Some(s);
            if notice.is_some() {
                self.notice = notice;
            }
        }
    }

    /// Read again every `every` while the page shows, so a change made elsewhere shows too.
    pub fn refresh_every(&mut self, ctx: &egui::Context, every: Duration) {
        if self.asked.is_some_and(|t| t.elapsed() >= every) {
            self.ask();
        }
        ctx.request_repaint_after(every);
    }

    /// Make a change on the worker; `expect` is what the page shows until Windows answers.
    pub fn change(&mut self, job: J, expect: impl FnOnce(S) -> S) {
        self.notice = None;
        if let Some(s) = self.shown.take() {
            self.shown = Some(expect(s));
        }
        if let Some(j) = &self.jobs {
            let _ = j.send(Some(job));
        }
        self.asked = Some(Instant::now());
    }
}
