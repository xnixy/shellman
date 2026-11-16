//! shellman-setup's engine: install, update, inspect and remove shellman. The command line
//! (`main.rs`) and the installer window (`apps/installer`) both drive it through [`ops`]; it
//! reports through [`out`]. See `docs/PLAN.md`, "Install and update".

#[macro_use]
pub mod out;
pub mod cli;
pub mod fsops;
pub mod integrate;
pub mod manifest;
pub mod ops;
pub mod paths;
pub mod plan;
pub mod reg;
pub mod sess;
pub mod sha256;
pub mod shortcuts;
pub mod state;
pub mod stop;
pub mod sys;
