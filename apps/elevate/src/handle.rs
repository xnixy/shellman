//! One handler per operation. Each does its single thing and says whether it worked.

use shellman_core::elevate::ops::Op;
use shellman_core::registry::{Hive, Real};
use shellman_core::startup::{approved, win};
use shellman_core::system::sound_enable;

pub fn run(op: &Op) -> bool {
    match op {
        Op::SoundDevice { enable, id } => sound_enable::set_enabled(id, *enable).is_ok(),
        // The one registry write the helper does: that item's value, in HKLM's StartupApproved.
        Op::StartupApprove { enable, area, name } => {
            approved::write(&Real, Hive::Hklm, *area, name, *enable, win::now_filetime()).is_ok()
        }
    }
}
