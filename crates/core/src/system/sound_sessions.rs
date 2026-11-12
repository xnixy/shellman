//! The volume mixer: the audio sessions on one output, one per app, each with its own volume and
//! mute (IAudioSessionManager2, ISimpleAudioVolume; all documented). A session appears when an
//! app first plays and goes when it closes, and both are heard through Windows' notifications:
//! one on the manager for new sessions, one on each session for its volume, its state and its end.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use windows::core::{implement, Interface, Result, BOOL, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, MAX_PATH};
use windows::Win32::Media::Audio::{
    AudioSessionDisconnectReason, AudioSessionState, AudioSessionStateActive, IAudioSessionControl,
    IAudioSessionControl2, IAudioSessionEvents, IAudioSessionEvents_Impl, IAudioSessionManager2,
    IAudioSessionNotification, IAudioSessionNotification_Impl, IMMDevice, ISimpleAudioVolume,
};
use windows::Win32::System::Com::{CoTaskMemFree, CLSCTX_ALL};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};

/// Called, from a Windows thread, when the sessions changed and should be read again.
pub type Poke = Arc<dyn Fn() + Send + Sync>;

/// One app's session.
#[derive(Debug, Clone, PartialEq)]
pub struct AppSession {
    /// Identifies the session for as long as it lives.
    pub key: String,
    pub name: String,
    /// The app's exe, whose icon the page shows.
    pub exe: Option<PathBuf>,
    pub system: bool,
    pub volume: f32,
    pub muted: bool,
    /// Whether it is playing right now.
    pub active: bool,
}

/// Apps by name, the system sounds last, as the classic mixer has them.
pub fn order(sessions: &mut [AppSession]) {
    sessions.sort_by_key(|s| (s.system, s.name.to_lowercase()));
}

struct Held {
    key: String,
    control: IAudioSessionControl,
    volume: ISimpleAudioVolume,
}

/// The sessions of one output, and what is registered on them.
pub struct Sessions {
    manager: Option<IAudioSessionManager2>,
    new_sessions: IAudioSessionNotification,
    events: IAudioSessionEvents,
    held: Vec<Held>,
    /// Set when a session came or went, so the next reading enumerates again. Registering on a
    /// session makes Windows call back at once, so enumerating on every reading would never end.
    dirty: Arc<AtomicBool>,
    poke: Poke,
}

impl Sessions {
    pub fn new(poke: Poke) -> Self {
        let dirty = Arc::new(AtomicBool::new(false));
        Self {
            manager: None,
            new_sessions: Created { poke: poke.clone(), dirty: dirty.clone() }.into(),
            events: Changed { poke: poke.clone(), dirty: dirty.clone() }.into(),
            held: Vec::new(),
            dirty,
            poke,
        }
    }

    /// Follow the sessions of `dev` instead of what was followed before; None follows nothing.
    pub fn attach(&mut self, dev: Option<&IMMDevice>) {
        self.detach();
        // SAFETY: valid interfaces; unregistered in `detach`.
        unsafe {
            let Some(dev) = dev else { return };
            let Ok(manager) = dev.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else { return };
            let _ = manager.RegisterSessionNotification(&self.new_sessions);
            self.manager = Some(manager);
        }
        self.reread();
    }

    fn detach(&mut self) {
        // SAFETY: the same objects that were registered.
        unsafe {
            for h in self.held.drain(..) {
                let _ = h.control.UnregisterAudioSessionNotification(&self.events);
            }
            if let Some(m) = self.manager.take() {
                let _ = m.UnregisterSessionNotification(&self.new_sessions);
            }
        }
    }

    /// Take the sessions as they are now, and listen to each.
    fn reread(&mut self) {
        // SAFETY: COM calls with valid arguments; a session that does not answer is skipped.
        unsafe {
            for h in self.held.drain(..) {
                let _ = h.control.UnregisterAudioSessionNotification(&self.events);
            }
            let Some(manager) = &self.manager else { return };
            let Ok(list) = manager.GetSessionEnumerator() else { return };
            for i in 0..list.GetCount().unwrap_or(0) {
                let Ok(control) = list.GetSession(i) else { continue };
                let Ok(two) = control.cast::<IAudioSessionControl2>() else { continue };
                let Ok(volume) = control.cast::<ISimpleAudioVolume>() else { continue };
                let Some(key) = pwstr(two.GetSessionInstanceIdentifier()) else { continue };
                let _ = control.RegisterAudioSessionNotification(&self.events);
                self.held.push(Held { key, control, volume });
            }
        }
    }

    /// The sessions as they are, after a notification.
    pub fn read(&mut self) -> Vec<AppSession> {
        if self.dirty.swap(false, Ordering::SeqCst) {
            self.reread();
        }
        let mut out: Vec<AppSession> = self.held.iter().filter_map(describe).collect();
        order(&mut out);
        out
    }

    pub fn set_volume(&self, key: &str, level: f32) {
        if let Some(h) = self.held.iter().find(|h| h.key == key) {
            // SAFETY: valid interface; a null event context is allowed.
            unsafe {
                let _ = h.volume.SetMasterVolume(level.clamp(0.0, 1.0), std::ptr::null());
            }
        }
    }

    pub fn set_muted(&self, key: &str, muted: bool) {
        if let Some(h) = self.held.iter().find(|h| h.key == key) {
            // SAFETY: as above.
            unsafe {
                let _ = h.volume.SetMute(muted, std::ptr::null());
            }
        }
    }
}

impl Drop for Sessions {
    fn drop(&mut self) {
        self.detach();
        let _ = &self.poke;
    }
}

unsafe fn pwstr(r: Result<windows::core::PWSTR>) -> Option<String> {
    let p = r.ok()?;
    let s = p.to_string().ok();
    CoTaskMemFree(Some(p.0 as *const _));
    s
}

fn describe(h: &Held) -> Option<AppSession> {
    // SAFETY: valid interfaces.
    unsafe {
        let two = h.control.cast::<IAudioSessionControl2>().ok()?;
        let system = two.IsSystemSoundsSession().0 == 0;
        let pid = two.GetProcessId().unwrap_or(0);
        let exe = if system { None } else { exe_of(pid) };
        let shown = pwstr(h.control.GetDisplayName()).filter(|n| !n.is_empty() && !n.starts_with('@'));
        let name = if system {
            "System sounds".to_owned()
        } else {
            match (&exe, shown) {
                (Some(path), _) => super::app_name::display_name(&path.to_string_lossy(), Some(path)),
                (None, Some(n)) => n,
                (None, None) => return None,
            }
        };
        let volume = h.volume.GetMasterVolume().ok()?;
        let muted = h.volume.GetMute().ok()?.as_bool();
        let active = h.control.GetState().ok()? == AudioSessionStateActive;
        Some(AppSession { key: h.key.clone(), name, exe, system, volume, muted, active })
    }
}

/// The exe a process runs, for its name and icon; None for a process that is gone or closed.
fn exe_of(pid: u32) -> Option<PathBuf> {
    if pid == 0 {
        return None;
    }
    // SAFETY: the handle is closed again; the buffer holds `len` UTF-16 units.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; MAX_PATH as usize * 2];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
        .is_ok();
        let _ = CloseHandle(process);
        ok.then(|| PathBuf::from(String::from_utf16_lossy(&buf[..len as usize])))
    }
}

/// Hears a new session open on the manager.
#[implement(IAudioSessionNotification)]
struct Created {
    poke: Poke,
    dirty: Arc<AtomicBool>,
}

impl IAudioSessionNotification_Impl for Created_Impl {
    fn OnSessionCreated(&self, _new: windows::core::Ref<'_, IAudioSessionControl>) -> Result<()> {
        self.dirty.store(true, Ordering::SeqCst);
        (self.poke)();
        Ok(())
    }
}

/// Hears a session's volume, state and end.
#[implement(IAudioSessionEvents)]
struct Changed {
    poke: Poke,
    dirty: Arc<AtomicBool>,
}

impl IAudioSessionEvents_Impl for Changed_Impl {
    fn OnDisplayNameChanged(&self, _name: &PCWSTR, _ctx: *const windows::core::GUID) -> Result<()> {
        (self.poke)();
        Ok(())
    }
    fn OnIconPathChanged(&self, _path: &PCWSTR, _ctx: *const windows::core::GUID) -> Result<()> {
        Ok(())
    }
    fn OnSimpleVolumeChanged(&self, _v: f32, _mute: BOOL, _ctx: *const windows::core::GUID) -> Result<()> {
        (self.poke)();
        Ok(())
    }
    fn OnChannelVolumeChanged(&self, _n: u32, _v: *const f32, _ch: u32, _ctx: *const windows::core::GUID) -> Result<()> {
        Ok(())
    }
    fn OnGroupingParamChanged(&self, _g: *const windows::core::GUID, _ctx: *const windows::core::GUID) -> Result<()> {
        Ok(())
    }
    fn OnStateChanged(&self, _state: AudioSessionState) -> Result<()> {
        (self.poke)();
        Ok(())
    }
    fn OnSessionDisconnected(&self, _why: AudioSessionDisconnectReason) -> Result<()> {
        self.dirty.store(true, Ordering::SeqCst);
        (self.poke)();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(name: &str, system: bool) -> AppSession {
        AppSession { key: name.into(), name: name.into(), exe: None, system, volume: 1.0, muted: false, active: false }
    }

    #[test]
    fn apps_by_name_then_system_sounds() {
        let mut v = vec![s("System sounds", true), s("spotify", false), s("Chrome", false)];
        order(&mut v);
        let names: Vec<_> = v.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Chrome", "spotify", "System sounds"]);
    }
}
