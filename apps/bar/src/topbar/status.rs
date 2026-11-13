//! The status icons' readings, followed live off the UI thread: sound through the same watcher
//! the Control Center uses (`SoundWatch`: Windows' own volume, mute and device callbacks, and a
//! kept endpoint for the scroll wheel's changes), the network through `ConnectivityWatch` (the
//! network list's events), and the battery when Windows says the power status changed
//! (`WM_POWERBROADCAST`, heard by the host window). Each watcher sleeps on its thread until
//! something changes and then wakes the top bars; a frame only takes what arrived. Nothing polls,
//! and no frame creates a COM object.

use shellman_core::system::audio::Sound;
use shellman_core::system::audio_watch::SoundWatch;
use shellman_core::system::radios::{Connectivity, ConnectivityWatch};
use shellman_core::system::status::{self, Battery, Network, Volume};
use std::sync::Arc;

/// What the status icons show.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    pub volume: Option<Volume>,
    pub network: Option<Network>,
    pub battery: Option<Battery>,
}

impl Status {
    /// Take in a sound reading.
    pub fn sound(&mut self, s: &Sound) {
        self.volume = s.volume;
    }

    /// Take in a connectivity reading.
    pub fn connectivity(&mut self, c: &Connectivity) {
        self.network = c.network;
    }

    /// The network as the icon shows it: offline until the watcher has said otherwise.
    pub fn network_or_offline(&self) -> Network {
        self.network.unwrap_or(Network::Offline)
    }
}

/// The level after one wheel step of `scroll` (any amount; only its sign counts) from `level`.
pub fn scrolled(level: f32, scroll: f32) -> f32 {
    if scroll == 0.0 || !scroll.is_finite() {
        return level;
    }
    (level + scroll.signum() * 0.02).clamp(0.0, 1.0)
}

pub struct StatusFeed {
    sound: Option<SoundWatch>,
    net: Option<ConnectivityWatch>,
    pub now: Status,
}

impl StatusFeed {
    /// Start the watchers; `wake` (any thread) repaints the top bars.
    pub fn start(wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self {
            sound: Some(SoundWatch::start(wake.clone())),
            net: Some(ConnectivityWatch::start(wake)),
            now: Status { battery: status::battery(), ..Default::default() },
        }
    }

    /// No watchers (tests).
    #[cfg(test)]
    pub fn idle() -> Self {
        Self { sound: None, net: None, now: Status::default() }
    }

    /// Take what the watchers sent since the last frame.
    pub fn take(&mut self) {
        if let Some(s) = self.sound.as_ref().and_then(SoundWatch::take) {
            self.now.sound(&s);
        }
        if let Some(c) = self.net.as_ref().and_then(ConnectivityWatch::take) {
            self.now.connectivity(&c);
        }
    }

    /// The power status changed (or the machine woke): read the battery again. One cheap,
    /// local call, made only then.
    pub fn power_changed(&mut self) {
        self.now.battery = status::battery();
    }

    /// One wheel step over the status icons: the new level goes to the sound worker, and shows
    /// at once.
    pub fn scroll(&mut self, scroll: f32) {
        let (Some(sound), Some(v)) = (&self.sound, self.now.volume.as_mut()) else { return };
        let level = scrolled(v.level, scroll);
        if level != v.level {
            v.level = level;
            sound.set_volume(level);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readings_land_where_the_icons_read_them() {
        let mut s = Status::default();
        assert_eq!(s.network_or_offline(), Network::Offline);
        s.connectivity(&Connectivity { network: Some(Network::Internet), ..Default::default() });
        assert_eq!(s.network_or_offline(), Network::Internet);
        s.sound(&Sound { volume: Some(Volume { level: 0.5, muted: true }), ..Default::default() });
        assert_eq!(s.volume, Some(Volume { level: 0.5, muted: true }));
        // A reading without an output (unplugged) takes the icon away.
        s.sound(&Sound::default());
        assert_eq!(s.volume, None);
    }

    #[test]
    fn a_wheel_step_is_two_points_and_stays_in_range() {
        assert_eq!(scrolled(0.5, 120.0), 0.52);
        assert_eq!(scrolled(0.5, -3.0), 0.48);
        assert_eq!(scrolled(0.99, 1.0), 1.0);
        assert_eq!(scrolled(0.01, -1.0), 0.0);
        assert_eq!(scrolled(0.3, 0.0), 0.3);
        assert_eq!(scrolled(0.3, f32::NAN), 0.3);
    }

    #[test]
    fn scrolling_without_a_sound_device_does_nothing() {
        let mut f = StatusFeed::idle();
        f.scroll(1.0);
        assert_eq!(f.now.volume, None);
    }
}
