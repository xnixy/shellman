//! Now playing: the media session Windows considers current (the one the media keys control),
//! its title, artist and art, and previous / play-pause / next.
//!
//! Followed through the session manager's and the session's own events; the worker sleeps in
//! between. WinRT's async calls are waited on here, never on the UI thread.

use super::live::{self, Mailbox, Wake};
use super::media_art::{self, Art};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use windows::Foundation::TypedEventHandler;
use windows::Media::Control::{
    CurrentSessionChangedEventArgs, GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status, MediaPropertiesChangedEventArgs,
    PlaybackInfoChangedEventArgs, TimelinePropertiesChangedEventArgs,
};

/// The art's size in pixels: twice the largest it is drawn, for sharp high-DPI screens.
const ART_SIDE: u32 = 144;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct NowPlaying {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub playing: bool,
    /// Where playback was, in seconds, at `at` (UTC seconds since 1970); 0 when unknown.
    pub position: f64,
    pub at: f64,
    /// The track's length in seconds; 0 when the player does not say.
    pub duration: f64,
    pub can_previous: bool,
    pub can_next: bool,
    pub can_toggle: bool,
    /// Shared, so a reading that only changes play state does not copy the pixels.
    pub art: Option<Arc<Art>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Previous,
    PlayPause,
    Next,
}

enum Cmd {
    Refresh,
    /// The current session changed: follow the new one.
    Reattach,
    Do(Action),
    Stop,
}

pub struct MediaWatch {
    tx: Sender<Cmd>,
    /// None inside: nothing is playing.
    mailbox: Arc<Mailbox<Option<NowPlaying>>>,
}

impl MediaWatch {
    pub fn start(wake: Wake) -> Self {
        let (tx, rx) = channel();
        let mailbox = Arc::new(Mailbox::default());
        let (to_self, out) = (tx.clone(), mailbox.clone());
        live::spawn("media-watch", move || run(rx, to_self, out, wake));
        Self { tx, mailbox }
    }

    /// The newest reading: `Some(None)` means nothing is playing any more.
    pub fn take(&self) -> Option<Option<NowPlaying>> {
        self.mailbox.take()
    }

    pub fn act(&self, action: Action) {
        let _ = self.tx.send(Cmd::Do(action));
    }
}

impl Drop for MediaWatch {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Stop);
    }
}

/// The session being followed, with its event registrations.
struct Followed {
    session: Session,
    tokens: (i64, i64, i64),
}

impl Followed {
    fn new(manager: &Manager, tx: &Sender<Cmd>) -> Option<Self> {
        let session = manager.GetCurrentSession().ok()?;
        let (a, b, c) = (tx.clone(), tx.clone(), tx.clone());
        let props = TypedEventHandler::<Session, MediaPropertiesChangedEventArgs>::new(move |_, _| {
            let _ = a.send(Cmd::Refresh);
            Ok(())
        });
        let playback = TypedEventHandler::<Session, PlaybackInfoChangedEventArgs>::new(move |_, _| {
            let _ = b.send(Cmd::Refresh);
            Ok(())
        });
        let timeline = TypedEventHandler::<Session, TimelinePropertiesChangedEventArgs>::new(move |_, _| {
            let _ = c.send(Cmd::Refresh);
            Ok(())
        });
        let tokens = (
            session.MediaPropertiesChanged(&props).unwrap_or_default(),
            session.PlaybackInfoChanged(&playback).unwrap_or_default(),
            session.TimelinePropertiesChanged(&timeline).unwrap_or_default(),
        );
        Some(Self { session, tokens })
    }

    fn close(self) {
        let _ = self.session.RemoveMediaPropertiesChanged(self.tokens.0);
        let _ = self.session.RemovePlaybackInfoChanged(self.tokens.1);
        let _ = self.session.RemoveTimelinePropertiesChanged(self.tokens.2);
    }

    fn act(&self, action: Action) {
        let op = match action {
            Action::Previous => self.session.TrySkipPreviousAsync(),
            Action::PlayPause => self.session.TryTogglePlayPauseAsync(),
            Action::Next => self.session.TrySkipNextAsync(),
        };
        let _ = op.and_then(|op| op.get());
    }
}

fn run(rx: Receiver<Cmd>, to_self: Sender<Cmd>, out: Arc<Mailbox<Option<NowPlaying>>>, wake: Wake) {
    live::init_mta();
    let Some(manager) = Manager::RequestAsync().and_then(|op| op.get()).ok() else {
        out.put(None);
        wake();
        return;
    };
    let tx = to_self.clone();
    let changed = TypedEventHandler::<Manager, CurrentSessionChangedEventArgs>::new(move |_, _| {
        let _ = tx.send(Cmd::Reattach);
        Ok(())
    });
    let token = manager.CurrentSessionChanged(&changed).ok();
    let mut followed = Followed::new(&manager, &to_self);
    // The art is read again only when the track changes.
    let mut last: Option<NowPlaying> = None;
    let _ = to_self.send(Cmd::Refresh);
    while let Ok(first) = rx.recv() {
        let mut batch = vec![first];
        batch.extend(rx.try_iter());
        if batch.iter().any(|c| matches!(c, Cmd::Stop)) {
            break;
        }
        if batch.iter().any(|c| matches!(c, Cmd::Reattach)) {
            if let Some(f) = followed.take() {
                f.close();
            }
            followed = Followed::new(&manager, &to_self);
        }
        for cmd in &batch {
            if let (Cmd::Do(action), Some(f)) = (cmd, &followed) {
                f.act(*action);
            }
        }
        let now = followed.as_ref().and_then(|f| read(&f.session, last.as_ref()));
        last.clone_from(&now);
        out.put(now);
        wake();
    }
    if let Some(f) = followed {
        f.close();
    }
    if let Some(token) = token {
        let _ = manager.RemoveCurrentSessionChanged(token);
    }
}

/// What the session shows now; `before` lends its art when the track is the same.
fn read(session: &Session, before: Option<&NowPlaying>) -> Option<NowPlaying> {
    let props = session.TryGetMediaPropertiesAsync().ok()?.get().ok()?;
    let mut title = props.Title().map(|s| s.to_string()).unwrap_or_default();
    let artist = props.Artist().map(|s| s.to_string()).unwrap_or_default();
    let album = props.AlbumTitle().map(|s| s.to_string()).unwrap_or_default();
    let info = session.GetPlaybackInfo().ok()?;
    let playing = info.PlaybackStatus().ok() == Some(Status::Playing);
    // A session with nothing to say and nothing playing is left out; one that plays but names
    // nothing (some players) goes by its app's name.
    if title.is_empty() && artist.is_empty() {
        if !playing {
            return None;
        }
        let app = session.SourceAppUserModelId().map(|s| s.to_string()).unwrap_or_default();
        title = super::app_name::display_name(&app, None);
    }
    let controls = info.Controls().ok()?;
    let same_track = before.is_some_and(|b| b.title == title && b.artist == artist);
    let art = if same_track {
        before.and_then(|b| b.art.clone())
    } else {
        props.Thumbnail().ok().and_then(|t| media_art::load(&t, ART_SIDE)).map(Arc::new)
    };
    let (position, at, duration) = timeline(session);
    Some(NowPlaying {
        album,
        position,
        at,
        duration,
        playing,
        can_previous: controls.IsPreviousEnabled().unwrap_or(false),
        can_next: controls.IsNextEnabled().unwrap_or(false),
        can_toggle: controls.IsPlayPauseToggleEnabled().unwrap_or(false),
        title,
        artist,
        art,
    })
}

/// 100 ns ticks between 1601 (Windows' epoch) and 1970.
const EPOCH_TICKS: i64 = 116_444_736_000_000_000;

/// Where playback is: (position, when that was, length), in seconds; (0, now, 0) when unknown.
fn timeline(session: &Session) -> (f64, f64, f64) {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64());
    let Ok(t) = session.GetTimelineProperties() else { return (0.0, now, 0.0) };
    let secs = |ticks: i64| ticks as f64 / 1e7;
    let start = t.StartTime().map_or(0, |d| d.Duration);
    let end = t.EndTime().map_or(0, |d| d.Duration);
    let pos = t.Position().map_or(0, |d| d.Duration);
    let updated = t.LastUpdatedTime().map_or(0, |d| d.UniversalTime);
    let at = if updated > EPOCH_TICKS { secs(updated - EPOCH_TICKS).min(now) } else { now };
    let duration = secs(end - start).max(0.0);
    (secs(pos - start).max(0.0), at, duration)
}

impl NowPlaying {
    /// Where playback is at `now` (UTC seconds): moving on from the last reading while playing,
    /// never past the end.
    pub fn position_at(&self, now: f64) -> f64 {
        let p = if self.playing { self.position + (now - self.at).max(0.0) } else { self.position };
        if self.duration > 0.0 { p.min(self.duration) } else { p }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What is playing on this machine; run with `--ignored --nocapture`. Reads only.
    #[test]
    #[ignore]
    fn probe() {
        live::init_mta();
        let manager = Manager::RequestAsync().and_then(|op| op.get());
        println!("manager: {:?}", manager.as_ref().map(|_| ()));
        if let Ok(m) = manager {
            println!("sessions: {:?}", m.GetSessions().map(|s| s.Size()));
            let current = m.GetCurrentSession();
            println!("current: {:?}", current.as_ref().map(|s| s.SourceAppUserModelId()));
            if let Ok(s) = current {
                let props = s.TryGetMediaPropertiesAsync().and_then(|op| op.get());
                println!("title: {:?}", props.as_ref().map(|p| (p.Title(), p.Artist())));
                println!("status: {:?}", s.GetPlaybackInfo().and_then(|i| i.PlaybackStatus()));
            }
        }
        let wake: Wake = Arc::new(|| {});
        let watch = MediaWatch::start(wake);
        std::thread::sleep(std::time::Duration::from_secs(2));
        match watch.take() {
            Some(Some(now)) => println!(
                "{:?} / {:?} playing={} art={:?}",
                now.title,
                now.artist,
                now.playing,
                now.art.as_ref().map(|a| a.side)
            ),
            other => println!("nothing: {:?}", other.map(|o| o.is_some())),
        }
    }
}
