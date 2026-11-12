//! Where notifications come from and what can be done to them, behind a trait: the platform
//! (`platform::Platform`) in the shell, a `Fake` in tests.

use super::model::{Action, Activation, Reply, Toast};
use std::sync::Mutex;

/// What pressing a toast (or one of its buttons) hands its app.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Invoke {
    pub args: String,
    pub kind: Activation,
    /// Text boxes' contents, by box id.
    pub inputs: Vec<(String, String)>,
}

impl Invoke {
    /// A click on the toast itself.
    pub fn click(toast: &Toast) -> Self {
        Self { args: toast.launch.clone(), kind: toast.launch_kind, inputs: Vec::new() }
    }

    pub fn button(action: &Action) -> Self {
        Self { args: action.args.clone(), kind: action.kind, inputs: Vec::new() }
    }

    /// The reply box's "Send", with what was typed.
    pub fn reply(reply: &Reply, text: &str) -> Self {
        Self {
            args: reply.send.args.clone(),
            kind: reply.send.kind,
            inputs: vec![(reply.id.clone(), text.to_owned())],
        }
    }
}

/// How an activation went.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    /// The app's activation handler received it.
    Delivered,
    /// A protocol URI was opened.
    Opened,
    /// The app has no handler shellman can reach (a packaged app, or a desktop app that did not
    /// register one): the caller brings the app forward instead.
    NoHandler,
}

/// The notification store.
pub trait Source: Send + Sync {
    /// Every notification there is now, oldest first.
    fn read(&self) -> Result<Vec<Toast>, String>;
    /// Remove one from the store (it is gone for every viewer).
    fn dismiss(&self, id: u32) -> Result<(), String>;
    /// Deliver a click to the toast's app. Blocking: it may start the app.
    fn activate(&self, toast: &Toast, invoke: &Invoke) -> Result<Outcome, String>;
}

/// A store in memory that records what was done to it.
#[derive(Default)]
pub struct Fake {
    pub toasts: Mutex<Vec<Toast>>,
    pub log: Mutex<Vec<String>>,
}

impl Source for Fake {
    fn read(&self) -> Result<Vec<Toast>, String> {
        Ok(self.toasts.lock().unwrap().clone())
    }

    fn dismiss(&self, id: u32) -> Result<(), String> {
        self.toasts.lock().unwrap().retain(|t| t.id != id);
        self.log.lock().unwrap().push(format!("dismiss {id}"));
        Ok(())
    }

    fn activate(&self, toast: &Toast, invoke: &Invoke) -> Result<Outcome, String> {
        self.log.lock().unwrap().push(format!("activate {} {} {:?}", toast.id, invoke.args, invoke.inputs));
        Ok(Outcome::Delivered)
    }
}

/// A toast for tests: `id` from `app`, sent `secs` after the epoch.
pub fn sample(id: u32, app: &str, title: &str, secs: u64) -> Toast {
    Toast {
        id,
        app_id: app.into(),
        app_name: app.into(),
        app_icon: None,
        title: title.into(),
        body: vec!["body".into()],
        attribution: None,
        time: std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(secs),
        launch: format!("launch {id}"),
        launch_kind: Activation::Foreground,
        actions: Vec::new(),
        reply: None,
    }
}
