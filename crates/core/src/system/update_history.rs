//! What Windows Update installed, from the Windows Update Agent's history (read only; nothing is
//! installed or changed through WUA). It is the fuller list: the orchestrator's own
//! `GetMostRecentCompletedUpdates` keeps only a few entries. Twenty entries read in about half
//! a second (26300), so it is read on a worker, when the page asks.
//!
//! The Store's app updates are in the same history (service `update_logic::STORE_SERVICE`) and
//! are left out: they are not Windows updates.

use super::update_logic::{history_group, is_store, ole_date_to_unix, Group, Outcome};
use windows::core::Interface;
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::System::UpdateAgent::{IUpdateHistoryEntry2, IUpdateSession, UpdateSession};

/// WUA's `UpdateOperation` for an installation (2 is an uninstallation).
const INSTALLATION: i32 = 1;

#[derive(Clone, PartialEq, Debug)]
pub struct Entry {
    /// Seconds since 1970, UTC.
    pub when: i64,
    pub title: String,
    pub group: Group,
    pub outcome: Outcome,
}

/// The newest `limit` installations, newest first. COM must be initialised on the calling
/// thread. The error is a plain line for the page.
pub fn read(limit: i32) -> Result<Vec<Entry>, String> {
    // SAFETY: COM calls on a thread the caller initialised; every out-value is owned.
    unsafe {
        let session: IUpdateSession = CoCreateInstance(&UpdateSession, None, CLSCTX_INPROC_SERVER).map_err(|e| format!("Windows Update's history is not available ({e})."))?;
        let searcher = session.CreateUpdateSearcher().map_err(|e| format!("Windows Update's history is not available ({e})."))?;
        let total = searcher.GetTotalHistoryCount().map_err(|e| format!("Could not read the history ({e})."))?;
        if total <= 0 {
            return Ok(Vec::new());
        }
        let list = searcher.QueryHistory(0, total.min(limit)).map_err(|e| format!("Could not read the history ({e})."))?;
        let mut out = Vec::new();
        for i in 0..list.Count().unwrap_or(0) {
            let Ok(e) = list.get_Item(i) else { continue };
            if e.Operation().map(|o| o.0) != Ok(INSTALLATION) {
                continue;
            }
            if is_store(&e.ServiceID().unwrap_or_default().to_string()) {
                continue;
            }
            let title = e.Title().unwrap_or_default().to_string();
            let categories: Vec<String> = e
                .cast::<IUpdateHistoryEntry2>()
                .ok()
                .and_then(|e2| e2.Categories().ok())
                .map(|c| (0..c.Count().unwrap_or(0)).filter_map(|j| c.get_Item(j).ok()?.Name().ok()).map(|b| b.to_string()).collect())
                .unwrap_or_default();
            let client = e.ClientApplicationID().unwrap_or_default().to_string();
            out.push(Entry {
                when: ole_date_to_unix(e.Date().unwrap_or(0.0)),
                group: history_group(&title, &categories, &client),
                outcome: Outcome::from_wua(e.ResultCode().map(|r| r.0).unwrap_or(0), e.HResult().unwrap_or(0)),
                title,
            });
        }
        out.sort_by(|a, b| b.when.cmp(&a.when));
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    /// Prints this machine's history: `cargo test -p shellman-core update_history -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn read_this_machine() {
        // SAFETY: plain COM start-up for the test thread.
        let _ = unsafe { windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED) };
        let t = std::time::Instant::now();
        let h = super::read(50).unwrap();
        println!("{} entries in {:?}", h.len(), t.elapsed());
        for e in &h {
            println!("{} | {:?} | {} | {}", e.when, e.group, e.title, e.outcome.text());
        }
    }
}
