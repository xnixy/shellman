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

    /// What a screen reader reads for the status group, a button that opens quick controls:
    /// every icon in words, in the order they are drawn.
    pub fn spoken(&self) -> String {
        let mut parts = vec!["Quick controls".to_owned()];
        parts.push(
            match self.network_or_offline() {
                Network::Internet => "connected to the internet",
                Network::Local => "connected, no internet",
                Network::Offline => "offline",
            }
            .to_owned(),
        );
        if let Some(v) = self.volume {
            parts.push(if v.muted { "sound muted".to_owned() } else { format!("volume {:.0}%", v.level * 100.0) });
        }
        if let Some(b) = self.battery {
            parts.push(format!("battery {}%{}", b.percent, if b.charging { ", charging" } else { "" }));
        }
        parts.join(", ")
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
    fn the_status_group_reads_as_words() {
        let mut s = Status::default();
        assert_eq!(s.spoken(), "Quick controls, offline");
        s.network = Some(Network::Internet);
        s.volume = Some(Volume { level: 0.4, muted: false });
        s.battery = Some(Battery { percent: 81, charging: true });
        assert_eq!(s.spoken(), "Quick controls, connected to the internet, volume 40%, battery 81%, charging");
        s.network = Some(Network::Local);
        s.volume = Some(Volume { level: 0.4, muted: true });
        s.battery = None;
        assert_eq!(s.spoken(), "Quick controls, connected, no internet, sound muted");
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
