//! The platform's notification store, read with Windows' own API from an unpackaged process.
//!
//! `UserNotificationListener` lists the notifications (id, app, time, text) and removes them; it
//! needs no package identity, only the user's "let apps read notifications" privacy switch
//! (`GetAccessStatus` is `Allowed` here without any prompt). Its `NotificationChanged` event is
//! not available to a desktop process (it fails with 0x80070490, "Element not found"), so
//! changes are noticed by `watch` instead. The XML is read from each app's toast history, which
//! an unpackaged process may read for any app id.
//!
//! Every call blocks (the async operations are waited on), so this runs off the UI thread, on a
//! thread that has COM (`watch` and `activate` see to that).

use super::model::Toast;
use super::pair::{assemble, Listed};
use super::source::{Invoke, Outcome, Source};
use std::collections::HashMap;
use std::time::{Duration, SystemTime};
use windows::core::HSTRING;
use windows::UI::Notifications::Management::{UserNotificationListener, UserNotificationListenerAccessStatus};
use windows::UI::Notifications::{KnownNotificationBindings, NotificationKinds, ToastNotificationManager, UserNotification};

/// From 1601 (Windows' clock) to 1970, in 100 ns ticks.
const TICKS_TO_UNIX: i64 = 116_444_736_000_000_000;

#[derive(Default)]
pub struct Platform;

impl Platform {
    pub fn new() -> Self {
        Self
    }
}

fn err(e: windows::core::Error) -> String {
    format!("{:#010x} {}", e.code().0, e.message())
}

fn listener() -> Result<UserNotificationListener, String> {
    let listener = UserNotificationListener::Current().map_err(err)?;
    match listener.GetAccessStatus().map_err(err)? {
        UserNotificationListenerAccessStatus::Allowed => Ok(listener),
        status => Err(format!("notification access is {status:?} (Windows privacy settings)")),
    }
}

/// One listed notification; None if it vanished while being read.
fn listed(n: &UserNotification) -> Option<Listed> {
    let app = n.AppInfo().ok()?;
    let app_id = app.AppUserModelId().ok()?.to_string();
    let app_name = app.DisplayInfo().and_then(|d| d.DisplayName()).map(|s| s.to_string()).unwrap_or_default();
    let mut texts = Vec::new();
    let generic = KnownNotificationBindings::ToastGeneric().ok()?;
    if let Ok(binding) = n.Notification().and_then(|v| v.Visual()).and_then(|v| v.GetBinding(&generic)) {
        if let Ok(elements) = binding.GetTextElements() {
            texts.extend(elements.into_iter().filter_map(|t| t.Text().ok()).map(|t| t.to_string()));
        }
    }
    let ticks = n.CreationTime().map(|t| t.UniversalTime).unwrap_or(TICKS_TO_UNIX);
    let since_unix = Duration::from_nanos((ticks - TICKS_TO_UNIX).max(0) as u64 * 100);
    Some(Listed { id: n.Id().ok()?, app_id, app_name, texts, time: SystemTime::UNIX_EPOCH + since_unix })
}

/// The XML of every toast `app` has in the store.
fn history(app: &str) -> Vec<String> {
    let Ok(list) = ToastNotificationManager::History().and_then(|h| h.GetHistoryWithId(&HSTRING::from(app))) else {
        return Vec::new();
    };
    list.into_iter().filter_map(|t| t.Content().and_then(|c| c.GetXml()).ok()).map(|x| x.to_string()).collect()
}

impl Source for Platform {
    fn read(&self) -> Result<Vec<Toast>, String> {
        let listener = listener()?;
        let all = listener.GetNotificationsAsync(NotificationKinds::Toast).map_err(err)?.get().map_err(err)?;
        let listed: Vec<Listed> = all.into_iter().filter_map(|n| listed(&n)).collect();
        let mut xml = HashMap::new();
        for l in &listed {
            if !xml.contains_key(&l.app_id) {
                xml.insert(l.app_id.clone(), history(&l.app_id));
            }
        }
        Ok(assemble(listed, &xml))
    }

    fn dismiss(&self, id: u32) -> Result<(), String> {
        listener()?.RemoveNotification(id).map_err(err)
    }

    fn activate(&self, toast: &Toast, invoke: &Invoke) -> Result<Outcome, String> {
        super::activate::deliver(&toast.app_id, invoke)
    }
}
