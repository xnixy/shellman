//! Starting the items, one after another with pauses (Explorer waits a few seconds after logon
//! and spaces them out, so the machine is not hit by everything at once). The launching and the
//! waiting come in as arguments, so the order, the RunOnce deletion and the stopping are tested.

use super::items::{Item, Origin};
use crate::registry::Registry;
use std::time::Duration;

/// Before the first item, and between items.
pub const FIRST_DELAY: Duration = Duration::from_secs(6);
pub const GAP: Duration = Duration::from_millis(1500);

pub trait Launcher {
    fn launch(&self, item: &Item) -> Result<(), String>;
}

fn delete_once(reg: &dyn Registry, item: &Item, log: &dyn Fn(&str)) -> bool {
    let Origin::Run { hive, key } = &item.origin else { return true };
    match reg.delete(*hive, key, &item.name) {
        Ok(()) => true,
        Err(e) => {
            log(&format!("startup: could not remove RunOnce value {}: {e}", item.name));
            false
        }
    }
}

/// `wait` sleeps and returns false when the session is ending: then nothing more is started.
/// Returns how many items were started.
pub fn run(
    items: &[Item],
    reg: &dyn Registry,
    launcher: &dyn Launcher,
    wait: &dyn Fn(Duration) -> bool,
    log: &dyn Fn(&str),
) -> usize {
    let mut started = 0;
    if !wait(FIRST_DELAY) {
        return started;
    }
    for item in items.iter().filter(|i| i.enabled) {
        // A RunOnce value goes before its command runs, unless it asked for after (`!`): a
        // command that crashes the session must not run again at every start.
        if item.once && !item.delete_after && !delete_once(reg, item, log) {
            continue;
        }
        match launcher.launch(item) {
            Ok(()) => {
                started += 1;
                log(&format!("startup: started {} ({})", item.name, item.place));
            }
            Err(e) => log(&format!("startup: could not start {} ({}): {e}", item.name, item.place)),
        }
        if item.once && item.delete_after {
            delete_once(reg, item, log);
        }
        if !wait(GAP) {
            break;
        }
    }
    started
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{fake::Fake, Hive, Value};
    use std::cell::{Cell, RefCell};

    struct Rec<'a>(&'a RefCell<Vec<String>>, &'a Fake);

    impl Launcher for Rec<'_> {
        fn launch(&self, item: &Item) -> Result<(), String> {
            // What the registry holds at the moment of launch.
            let still_there = self.1.get(Hive::Hkcu, "K", &item.name).is_some();
            self.0.borrow_mut().push(format!("{}:{}", item.name, still_there));
            if item.name == "bad" { Err("no".into()) } else { Ok(()) }
        }
    }

    fn item(name: &str, enabled: bool, once: bool, after: bool) -> Item {
        Item {
            origin: Origin::Run { hive: Hive::Hkcu, key: "K".into() },
            place: "p".into(),
            name: name.into(),
            command: "c".into(),
            enabled,
            why: String::new(),
            once,
            delete_after: after,
        }
    }

    #[test]
    fn runs_enabled_items_in_order_and_skips_disabled() {
        let (reg, seen) = (Fake::default(), RefCell::new(Vec::new()));
        let items = [item("a", true, false, false), item("b", false, false, false), item("bad", true, false, false), item("c", true, false, false)];
        let n = run(&items, &reg, &Rec(&seen, &reg), &|_| true, &|_| {});
        assert_eq!(n, 2, "the one that failed to start is not counted");
        let names: Vec<_> = seen.borrow().iter().map(|s| s.split(':').next().unwrap().to_string()).collect();
        assert_eq!(names, ["a", "bad", "c"]);
    }

    #[test]
    fn runonce_is_deleted_before_launch_or_after_with_bang() {
        let (reg, seen) = (Fake::default(), RefCell::new(Vec::new()));
        reg.put(Hive::Hkcu, "K", "before", Value::Text("x".into()));
        reg.put(Hive::Hkcu, "K", "after", Value::Text("x".into()));
        let items = [item("before", true, true, false), item("after", true, true, true)];
        run(&items, &reg, &Rec(&seen, &reg), &|_| true, &|_| {});
        assert_eq!(*seen.borrow(), ["before:false", "after:true"]);
        assert_eq!(*reg.log.borrow(), ["delete before", "delete after"]);
    }

    #[test]
    fn runonce_that_cannot_be_deleted_is_not_run() {
        let (reg, seen) = (Fake::default(), RefCell::new(Vec::new()));
        reg.put(Hive::Hkcu, "K", "stuck", Value::Text("x".into()));
        reg.refuse.borrow_mut().push("stuck".into());
        let n = run(&[item("stuck", true, true, false)], &reg, &Rec(&seen, &reg), &|_| true, &|_| {});
        assert_eq!(n, 0);
        assert!(seen.borrow().is_empty());
    }

    #[test]
    fn stops_when_the_session_ends() {
        let (reg, seen) = (Fake::default(), RefCell::new(Vec::new()));
        let waits = Cell::new(0);
        // The first wait (the initial delay) passes, the one after the first item says stop.
        let wait = |_d: Duration| {
            waits.set(waits.get() + 1);
            waits.get() < 2
        };
        let items = [item("a", true, false, false), item("b", true, false, false)];
        assert_eq!(run(&items, &reg, &Rec(&seen, &reg), &wait, &|_| {}), 1);
        // Ending before the first delay is over starts nothing.
        assert_eq!(run(&items, &reg, &Rec(&seen, &reg), &|_| false, &|_| {}), 0);
    }
}
