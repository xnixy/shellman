//! Cheap, non-blocking system readings for the top bar: clock, volume, network, battery and
//! friendly app names. Every function is a handful of syscalls or COM calls meant to be called
//! from a UI thread about once a minute or on a user action; nothing here polls or sleeps.
//!
//! The control centre's live state comes from watchers (`audio_watch`, `radios`, `brightness`,
//! `media`): each owns a worker thread that hears Windows' own change events, and ends with it.

pub mod app_name;
pub mod audio;
pub mod audio_watch;
pub mod backlight;
pub mod brightness;
pub mod clock;
pub mod display;
pub mod display_names;
pub mod live;
pub mod media;
pub mod media_art;
pub mod netlist;
pub mod pointer;
pub mod power;
pub mod radios;
pub mod sound_choices;
pub mod sound_dev;
pub mod sound_enable;
pub mod sound_format;
pub mod sound_page;
pub mod sound_sessions;
pub mod sound_spatial;
mod sound_worker;
pub mod status;
pub mod wifi;
