//! The platform's own record of toasts coming and going, as a kernel event to wait on.
//!
//! The notification platform logs every toast it delivers (event 3153), clears (3055) or that its
//! app cancels (3054) to `Microsoft-Windows-PushNotification-Platform/Operational`, which the
//! user may read. A subscription to just those events signals a Win32 event the moment one is
//! written: a push, not a poll. The other events in that log (connection keep-alives, endpoint
//! clean-ups every few seconds) are left out by the query, so they never wake anyone.

use windows::core::w;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::EventLog::{EvtClose, EvtNext, EvtSubscribe, EvtSubscribeToFutureEvents, EVT_HANDLE};

const CHANNEL: windows::core::PCWSTR = w!("Microsoft-Windows-PushNotification-Platform/Operational");
const QUERY: windows::core::PCWSTR = w!("*[System[(EventID=3153 or EventID=3055 or EventID=3054)]]");

/// A live subscription; closed on drop.
pub struct Subscription(EVT_HANDLE);

// The handle is only used through wevtapi calls, which are thread-safe.
unsafe impl Send for Subscription {}

impl Subscription {
    /// Signal `event` whenever a toast is delivered, cleared or cancelled. None if the log cannot
    /// be read (turned off, or not permitted).
    pub fn start(event: HANDLE) -> Option<Self> {
        let h = unsafe {
            EvtSubscribe(None, Some(event), CHANNEL, QUERY, None, None, None, EvtSubscribeToFutureEvents.0)
        };
        let subscription = Subscription(h.ok()?);
        // The event is signalled only once the results have been read to the end, so once now.
        subscription.drain();
        Some(subscription)
    }

    /// Take the events that arrived, so the next one signals again. True if there were any.
    pub fn drain(&self) -> bool {
        let mut any = false;
        loop {
            let mut batch = [0isize; 16];
            let mut got = 0u32;
            if unsafe { EvtNext(self.0, &mut batch, 0, 0, &mut got) }.is_err() || got == 0 {
                return any;
            }
            any = true;
            for &e in &batch[..got as usize] {
                unsafe {
                    let _ = EvtClose(EVT_HANDLE(e));
                }
            }
        }
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        unsafe {
            let _ = EvtClose(self.0);
        }
    }
}
