//! Cheap, non-blocking system readings for the top bar: clock, volume, network, battery and
//! friendly app names. Every function is a handful of syscalls or COM calls meant to be called
//! from a UI thread about once a minute or on a user action; nothing here polls or sleeps.

pub mod app_name;
pub mod audio;
pub mod audio_watch;
pub mod clock;
pub mod live;
pub mod netlist;
pub mod radios;
pub mod status;
pub mod wifi;
