//! Windows services, read only: what the Service Control Manager lists, for Task Manager's
//! Services page. Nothing here starts, stops or changes a service, and nothing needs
//! administrator rights: the SCM is opened to enumerate, and each service to read its config.
//!
//! [`native`] is the Win32 reading, [`reader`] runs it on a thread of its own, one reading at a
//! time, and [`logic`] is the pure part (labels, sort, search).

pub mod logic;
pub mod native;
pub mod reader;

/// Where a service is in its life.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum State {
    Running,
    #[default]
    Stopped,
    Starting,
    Stopping,
    Paused,
}

/// When Windows starts a service.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Startup {
    Boot,
    System,
    Automatic,
    AutomaticDelayed,
    Manual,
    Disabled,
}

/// One service as of the latest reading.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Service {
    /// The name it is registered under (`Spooler`).
    pub name: String,
    /// What Services shows (`Print Spooler`).
    pub display: String,
    pub state: State,
    /// None when the config could not be read without administrator rights.
    pub startup: Option<Startup>,
    /// Zero when it is not running.
    pub pid: u32,
    /// None when it has none or it could not be read.
    pub description: Option<String>,
}
