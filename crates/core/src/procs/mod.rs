//! Task Manager's data: the processes, what they use, and the machine's totals.
//!
//! A [`Source`] gives one [`Sample`] per tick; the real one ([`native::NativeSource`]) reads it
//! all from native calls (one `NtQuerySystemInformation` for every process, PDH counters for the
//! GPU, disks and network), and the [`sampler`] runs it on a thread of its own. What the window
//! does to a process goes through [`Control`]. Both are traits, so the window's logic is tested
//! with fakes. The arithmetic (CPU from two samples, rates, grouping, sorting, search, the
//! "may this be ended" policy, formatting) is pure and lives in modules of its own.

pub mod cpu;
pub mod end;
pub mod format;
pub mod gpu;
pub mod group;
pub mod history;
pub mod info;
pub mod native;
mod nt;
mod pdh;
pub mod policy;
pub mod sampler;
pub mod search;
pub mod sort;
mod system;
mod wins;

pub use system::cpu_name;

use std::path::Path;
use std::sync::Arc;

/// A process, told apart from a later one that reuses its PID by when it started.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default, PartialOrd, Ord)]
pub struct Key {
    pub pid: u32,
    /// When it started, as a FILETIME (100 ns steps since 1601).
    pub created: i64,
}

/// What a process is doing, as the Status column says it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Status {
    #[default]
    Running,
    /// Every thread is suspended (a Store app in the background, or a debugger's stop).
    Suspended,
    /// One of its windows has not answered for a while (`IsHungAppWindow`).
    NotResponding,
}

/// How far this (unelevated) program can reach into a process.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Access {
    /// Its path can be read and it can be ended.
    #[default]
    Full,
    /// Its path can be read, but it cannot be ended (it runs elevated, or as another user).
    ReadOnly,
    /// Nothing beyond what the system list says (a protected or system process).
    None,
}

/// One top-level window of a process.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Window {
    pub hwnd: isize,
    pub title: String,
    pub hung: bool,
}

/// One process as of the latest sample.
#[derive(Clone, Debug, Default)]
pub struct Proc {
    pub key: Key,
    /// The PID of the process that started it; it may have ended, or its PID may be reused
    /// (see [`group`], which checks the start times).
    pub parent: u32,
    /// The image name, `firefox.exe`.
    pub name: Arc<str>,
    pub exe: Option<Arc<Path>>,
    /// A friendly name: the exe's description, or the name without `.exe`.
    pub description: Arc<str>,
    pub command_line: Arc<str>,
    /// Share of the whole machine, 0 to 100 (all cores together are 100, as Task Manager has it).
    pub cpu: f32,
    /// Private working set, in bytes (what Task Manager's Memory column shows).
    pub memory: u64,
    /// Bytes read and written per second, all file and device I/O.
    pub disk: f64,
    /// Busiest GPU engine it uses, 0 to 100; None when the counters are not there.
    pub gpu: Option<f32>,
    pub threads: u32,
    pub handles: u32,
    pub session: u32,
    pub suspended: bool,
    pub windows: Vec<Window>,
    pub access: Access,
}

impl Proc {
    pub fn status(&self) -> Status {
        if self.windows.iter().any(|w| w.hung) {
            Status::NotResponding
        } else if self.suspended {
            Status::Suspended
        } else {
            Status::Running
        }
    }

    /// The first window's title, if it has a window.
    pub fn title(&self) -> Option<&str> {
        self.windows.first().map(|w| w.title.as_str())
    }
}

/// Memory of the whole machine, in bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Memory {
    pub total: u64,
    pub available: u64,
    pub committed: u64,
    pub commit_limit: u64,
    pub cached: u64,
}

impl Memory {
    pub fn used(&self) -> u64 {
        self.total.saturating_sub(self.available)
    }
}

/// One physical disk.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Disk {
    /// The counter's instance name: `0 C: D:`.
    pub name: String,
    /// Share of the time it was busy, 0 to 100.
    pub active: f32,
    pub read: f64,
    pub write: f64,
}

/// One network adapter that is up.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Net {
    /// What Windows calls it (`Ethernet`, `Wi-Fi`).
    pub name: String,
    /// The hardware's description.
    pub description: String,
    pub address: String,
    /// Bytes per second.
    pub send: f64,
    pub receive: f64,
}

/// One GPU.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Gpu {
    pub name: String,
    /// Busiest engine, 0 to 100.
    pub utilisation: f32,
    pub dedicated_used: u64,
    pub dedicated_total: u64,
    pub shared_used: u64,
    pub shared_total: u64,
}

/// The machine as of the latest sample.
#[derive(Clone, Debug, Default)]
pub struct System {
    /// All cores together, 0 to 100.
    pub cpu: f32,
    pub cores: Vec<f32>,
    /// The current speed in MHz, when the counters give it.
    pub mhz: Option<f32>,
    pub uptime_secs: u64,
    pub processes: u32,
    pub threads: u32,
    pub handles: u32,
    pub memory: Memory,
    pub disks: Vec<Disk>,
    pub nets: Vec<Net>,
    pub gpus: Vec<Gpu>,
}

impl System {
    /// The busiest GPU's utilisation, if there is a GPU.
    pub fn gpu(&self) -> Option<f32> {
        self.gpus.iter().map(|g| g.utilisation).reduce(f32::max)
    }

    /// Busiest disk's active time.
    pub fn disk_active(&self) -> f32 {
        self.disks.iter().map(|d| d.active).fold(0.0, f32::max)
    }

    /// Bytes sent and received per second over every adapter.
    pub fn net_rate(&self) -> f64 {
        self.nets.iter().map(|n| n.send + n.receive).sum()
    }
}

/// One tick's reading.
#[derive(Clone, Debug, Default)]
pub struct Sample {
    pub procs: Vec<Proc>,
    pub system: System,
    /// How long the reading took, for measuring the sampler's own cost.
    pub took: std::time::Duration,
}

/// Something that reads the processes and the machine, once per call. It lives on the sampling
/// thread (the counters' handles stay where they were opened), so it need not be `Send`.
pub trait Source {
    fn sample(&mut self) -> Sample;
}

/// What can be read about one process on asking (the details row).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Details {
    pub user: Option<String>,
    pub elevated: Option<bool>,
    /// `x64`, `x86`, `ARM64`...
    pub architecture: Option<&'static str>,
    /// When it started, in local time, `2026-11-12 09:41:07`.
    pub started: String,
    /// Why some of it could not be read, if it could not.
    pub denied: Option<String>,
}

/// What the window does to processes. Every call names the process by [`Key`], so a PID that was
/// reused by a newer process is never acted on.
pub trait Control {
    /// Ask these windows to close (WM_CLOSE), as their own close button would. How many were asked.
    fn close_windows(&self, windows: &[isize]) -> usize;
    /// End the process at once (TerminateProcess).
    fn terminate(&self, key: Key) -> Result<(), String>;
    fn details(&self, key: Key) -> Details;
}
