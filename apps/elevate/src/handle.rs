//! One handler per operation. Each does its single thing and says whether it worked.

use shellman_core::elevate::ops::Op;
use shellman_core::procs::{end, Key};
use shellman_core::registry::{Hive, Real};
use shellman_core::startup::{approved, win};
use shellman_core::system::sound_enable;
use shellman_sysfont::elevated::{self, Step};

pub fn run(op: &Op) -> bool {
    match op {
        Op::SoundDevice { enable, id } => sound_enable::set_enabled(id, *enable).is_ok(),
        // The one registry write the helper does: that item's value, in HKLM's StartupApproved.
        Op::StartupApprove { enable, area, name } => {
            approved::write(&Real, Hive::Hklm, *area, name, *enable, win::now_filetime()).is_ok()
        }
        // The system font: install the checked replacement faces and point the Segoe UI entries at
        // them, or point them back. The step checks its plan itself and leaves its message for
        // Settings in a fixed file (see shellman_sysfont::elevated).
        Op::SysfontApply { generation } => elevated::run_and_report(&Step::Apply { generation: generation.clone() }),
        Op::SysfontRestore => elevated::run_and_report(&Step::Restore),
        Op::SysfontReuse { generation } => elevated::run_and_report(&Step::Reuse { generation: generation.clone() }),
        // Each process by PID and start time, in the caller's order (children first). One that
        // fails does not stop the rest; the exit code says whether any failed.
        Op::EndProcesses { targets } => {
            let mut all = true;
            for &(pid, created) in targets {
                all &= end::terminate_as_helper(Key { pid, created }).is_ok();
            }
            all
        }
    }
}
