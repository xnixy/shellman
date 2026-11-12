//! What a notification is, as shellman shows it.

use std::path::PathBuf;
use std::time::SystemTime;

/// How the app wants a click (on the toast or one of its buttons) delivered.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Activation {
    /// To the app, which comes forward (the default).
    #[default]
    Foreground,
    /// To the app, which stays in the background (a reply sent from the toast).
    Background,
    /// The arguments are a URI to open.
    Protocol,
    /// One of Windows' own buttons (snooze, dismiss); shellman leaves these out.
    System,
}

impl Activation {
    /// From a toast's `activationType` attribute; missing means foreground.
    pub fn parse(value: Option<&str>) -> Self {
        match value.map(str::to_ascii_lowercase).as_deref() {
            Some("background") => Activation::Background,
            Some("protocol") => Activation::Protocol,
            Some("system") => Activation::System,
            _ => Activation::Foreground,
        }
    }
}

/// A button on a toast.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Action {
    pub label: String,
    /// Handed back to the app when the button is pressed.
    pub args: String,
    pub kind: Activation,
    /// The text box whose text goes with it (`hint-inputId`): this is the reply's "Send".
    pub input: Option<String>,
}

/// A text box on a toast, with the button that sends it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Reply {
    /// The box's id: the app receives the text under this key.
    pub id: String,
    pub placeholder: String,
    pub send: Action,
}

/// One notification in the platform's store.
#[derive(Clone, PartialEq, Debug)]
pub struct Toast {
    /// The platform's id for it (what removing it takes).
    pub id: u32,
    /// The sending app's AppUserModelID.
    pub app_id: String,
    pub app_name: String,
    /// The app's logo as the toast gave it (`appLogoOverride`), when it is a file on disk.
    pub app_icon: Option<PathBuf>,
    pub title: String,
    pub body: Vec<String>,
    /// Small text under the body, such as "via Teams".
    pub attribution: Option<String>,
    pub time: SystemTime,
    /// What a click on the toast itself hands the app, and how.
    pub launch: String,
    pub launch_kind: Activation,
    /// Buttons, without the reply's own "Send" (that one is on `reply`).
    pub actions: Vec<Action>,
    pub reply: Option<Reply>,
}

impl Toast {
    pub fn has_reply(&self) -> bool {
        self.reply.is_some()
    }
}
