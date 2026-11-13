//! The transparency state DWM reads (`system::transparency`).
//!
//! `transparency` prints it, the user's setting and whether Windows' own shell runs.
//! `transparency republish` publishes the value the state already holds again (a write that
//! changes nothing but the change stamp), to prove the call and the access check. It refuses when
//! the state has no data. It never publishes anything else.

use shellman_core::system::transparency as t;

fn main() {
    let show = |label: &str| match t::read() {
        Ok(r) => println!("{label}: stamp={} allowed={:?}", r.stamp, r.allowed),
        Err(s) => println!("{label}: query failed {:#010x}", s as u32),
    };
    show("state");
    println!("setting={} real_shell={:?}", t::setting(), t::real_shell());
    if std::env::args().nth(1).as_deref() == Some("republish") {
        let Some((_, value)) = t::query() else {
            println!("no data: not publishing");
            return;
        };
        match t::publish(value) {
            Ok(()) => println!("publish({value}): ok"),
            Err(s) => println!("publish({value}): failed {:#010x}", s as u32),
        }
        show("after");
    }
}
