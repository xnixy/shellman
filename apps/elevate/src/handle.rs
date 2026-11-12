//! One handler per operation. Each does its single thing and says whether it worked.

use shellman_core::elevate::ops::Op;
use shellman_core::system::sound_enable;

pub fn run(op: &Op) -> bool {
    match op {
        Op::SoundDevice { enable, id } => sound_enable::set_enabled(id, *enable).is_ok(),
    }
}
