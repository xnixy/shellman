//! Shellman's Win32 shell plumbing: file operations, thumbnails, the shell menu, folder and
//! window watching, the app-window list, live window previews, and config.

pub mod app_host;
pub mod apps_index;
pub mod config;
pub mod elevate;
pub mod foreground;
pub mod frames;
pub mod icon_pack;
pub mod fsops;
pub mod installed;
pub mod live_thumb;
pub mod notify;
pub mod places;
pub mod places_reg;
pub mod procs;
pub mod recycle_bin;
pub mod registry;
pub mod services;
pub mod settings;
pub mod shell_menu;
pub mod sibling;
pub mod startup;
pub mod system;
pub mod thumbs;
pub mod transfer;
pub mod watch;
pub mod winlist;
