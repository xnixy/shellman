//! The Display page's machinery: the pure rules (`geom`, `modes`, `scale`, `revert`), what the
//! page shows (`page`), reading and changing the configuration (`os_read`, `os_apply`), the
//! worker that owns them (`worker`) and the "which screen is which" overlay (`identify`).

pub mod geom;
pub mod identify;
pub mod modes;
mod os_apply;
mod os_read;
pub mod page;
pub mod revert;
pub mod scale;
mod worker;

pub use worker::DisplayPage;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    fn first_page(p: &DisplayPage) -> Option<page::Page> {
        let until = Instant::now() + Duration::from_secs(8);
        while Instant::now() < until {
            if let Some(page) = p.take() {
                return Some(page);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        None
    }

    /// Reads the real configuration and prints it; changes nothing. By hand: `--ignored --nocapture`.
    #[test]
    #[ignore]
    fn dump_the_real_configuration() {
        let p = DisplayPage::start(Arc::new(|| {}));
        let page = first_page(&p).expect("a reading");
        println!("topology {:?}", page.topology);
        for m in &page.monitors {
            println!(
                "{} #{} {:?} rect {:?} {:?} {}Hz primary {} scale {:?} (now {:?}, recommended {:?}) hdr {:?}",
                m.id, m.number, m.name, m.rect, m.orientation, m.hz, m.primary, m.scale,
                m.scale.and_then(|s| s.current()), m.scale.and_then(|s| s.recommended()), m.hdr
            );
            for g in &m.groups {
                println!("   {} {:?}{}", g.label(), g.rates, if g.recommended { " recommended" } else { "" });
            }
        }
    }

    /// Validation only: asks Windows whether a plan would be accepted and applies nothing.
    #[test]
    #[ignore]
    fn validate_without_applying() {
        let (monitors, _, snap) = os_read::monitors().expect("a configuration");
        let plan: Vec<_> = monitors.iter().map(page::Change::of).collect();
        // An identical plan has nothing to apply.
        assert!(os_apply::validate(&monitors, &plan, &snap).is_err());
        // The same mode at another rate of the last monitor, tested with CDS_TEST only.
        let mut changed = plan.clone();
        let last = monitors.len() - 1;
        let rates = &monitors[last].groups.iter().find(|g| g.w as i32 == monitors[last].rect.w).unwrap().rates;
        println!("rates for the last monitor: {rates:?}");
        if let Some(other) = rates.iter().find(|r| **r != monitors[last].hz) {
            changed[last].hz = *other;
            println!("CDS_TEST {} Hz: {:?}", other, os_apply::validate(&monitors, &changed, &snap));
        }
        // A mode no monitor has is refused.
        changed[last].w = 123;
        println!("CDS_TEST 123 wide: {:?}", os_apply::validate(&monitors, &changed, &snap));
        assert!(os_apply::validate(&monitors, &changed, &snap).is_err());
        println!("topology extend validates: {:?}", os_apply::set_topology(page::Topology::Extend, false));
    }

    /// Writes the raw QueryDisplayConfig arrays (paths, modes, topology) to the file named by
    /// SHELLMAN_DISPLAY_DUMP, to compare a configuration before and after a session.
    #[test]
    #[ignore]
    fn dump_raw_config() {
        let Some(file) = std::env::var_os("SHELLMAN_DISPLAY_DUMP") else { return };
        let (snap, topo) = os_read::query().expect("a configuration");
        let mut text = format!("topology {topo:?}
");
        for p in &snap.paths {
            let (s, t) = (&p.sourceInfo, &p.targetInfo);
            // SAFETY: plain integers of the legacy arm.
            let idx = unsafe { s.Anonymous.modeInfoIdx };
            text += &format!(
                "path src {}:{} idx {idx} st {} | tgt {} rot {} scaling {} rate {}/{} order {} st {} | flags {}
",
                s.adapterId.LowPart, s.id, s.statusFlags, t.id, t.rotation.0, t.scaling.0,
                t.refreshRate.Numerator, t.refreshRate.Denominator, t.scanLineOrdering.0, t.statusFlags, p.flags
            );
        }
        for m in &snap.modes {
            // SAFETY: the type says which arm is live; targets are read as raw bytes below.
            let body = unsafe {
                if m.infoType == windows::Win32::Devices::Display::DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE {
                    let s = m.Anonymous.sourceMode;
                    format!("source {}x{} fmt {} at {},{}", s.width, s.height, s.pixelFormat.0, s.position.x, s.position.y)
                } else {
                    let bytes = std::slice::from_raw_parts(&m.Anonymous as *const _ as *const u8, 48);
                    format!("other {bytes:02x?}")
                }
            };
            text += &format!("mode type {} id {} adapter {} {body}
", m.infoType.0, m.id, m.adapterId.LowPart);
        }
        std::fs::write(file, text).unwrap();
    }

    /// Shows the identify overlay once and waits for it to go. By hand only.
    #[test]
    #[ignore]
    fn identify_for_a_moment() {
        let (monitors, _, _) = os_read::monitors().expect("a configuration");
        identify::show(monitors.iter().map(|m| (m.rect, m.number)).collect());
        std::thread::sleep(Duration::from_secs(6));
    }

    /// The one real apply, by hand: the LAST monitor's refresh rate to another rate at the same
    /// resolution, then back through the page's own revert, checking both readings. Set
    /// SHELLMAN_DISPLAY_REAL_APPLY=1 to allow it.
    #[test]
    #[ignore]
    fn real_apply_and_revert_of_a_refresh_rate() {
        if std::env::var_os("SHELLMAN_DISPLAY_REAL_APPLY").is_none() {
            return;
        }
        let p = DisplayPage::start(Arc::new(|| {}));
        let first = first_page(&p).expect("a reading");
        let m = first.monitors.last().unwrap().clone();
        let g = m.groups.iter().find(|g| g.w as i32 == m.rect.w && g.h as i32 == m.rect.h).unwrap();
        let Some(other) = g.rates.iter().copied().find(|r| *r != m.hz) else { return };
        let mut staged: Vec<_> = first.monitors.iter().map(page::Change::of).collect();
        staged.last_mut().unwrap().hz = other;
        let primary = first.monitors.iter().find(|x| x.primary).unwrap().id.clone();
        p.apply(staged, &primary);
        let until = Instant::now() + Duration::from_secs(10);
        let mut pending = None;
        while Instant::now() < until && pending.is_none() {
            pending = first_page(&p).filter(|pg| pg.pending.is_some() || pg.notice.is_some());
            if let Some(pg) = &pending {
                println!("after apply: {} Hz (was {}), pending {}, notice {:?}", pg.monitors.last().unwrap().hz, m.hz, pg.pending.is_some(), pg.notice);
            }
        }
        std::thread::sleep(Duration::from_secs(1));
        p.revert();
        std::thread::sleep(Duration::from_secs(3));
        p.refresh();
        let back = first_page(&p).expect("a reading");
        println!("after revert: {} Hz, others {:?}", back.monitors.last().unwrap().hz, back.monitors.iter().map(|x| (x.hz, x.rect)).collect::<Vec<_>>());
        assert_eq!(back.monitors, first.monitors);
    }
}
