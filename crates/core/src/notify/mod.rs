//! Notifications: the toasts apps send to Windows, read so shellman can show them (with Explorer
//! gone, nothing else does; the platform still stores every toast).
//!
//! What works from an unpackaged process with no immersive shell (measured on build 26300):
//! - reading: `UserNotificationListener` (`platform`) lists toasts and their text, with the
//!   user's privacy switch on; each app's toast history gives the XML (`pair` joins the two);
//! - noticing changes: the platform's event log, subscribed to (`events`, `watch`); the listener's
//!   own change event is refused to desktop processes;
//! - dismissing: `UserNotificationListener::RemoveNotification`;
//! - clicking, buttons and replies: the app's toast activator, a COM local server any process
//!   may call (`activate`); packaged apps excepted.
//!
//! `Source` keeps all of that behind a trait, so what shows them is tested with `source::Fake`.

pub mod activate;
pub mod events;
pub mod model;
pub mod pair;
pub mod platform;
pub mod source;
pub mod watch;
pub mod xml;

pub use model::{Action, Activation, Reply, Toast};
pub use source::{Invoke, Outcome, Source};
