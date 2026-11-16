//! Where setup's report goes: standard output for the command line, or whatever the installer
//! window hands in (`set_sink`), one line at a time. The engine says everything through `say!`.

use std::sync::Mutex;

type Sink = Box<dyn Fn(&str) + Send>;

static SINK: Mutex<Option<Sink>> = Mutex::new(None);

/// Send every later line to `f` instead of standard output (process-wide).
pub fn set_sink(f: impl Fn(&str) + Send + 'static) {
    if let Ok(mut s) = SINK.lock() {
        *s = Some(Box::new(f));
    }
}

/// One line of the report (may hold several, split on `\n`).
pub fn line(text: &str) {
    let sink = SINK.lock();
    match sink.as_deref() {
        Ok(Some(f)) => {
            for l in text.split('\n') {
                f(l);
            }
        }
        _ => println!("{text}"),
    }
}

/// `println!` for the report.
#[macro_export]
macro_rules! say {
    () => { $crate::out::line("") };
    ($($t:tt)*) => { $crate::out::line(&format!($($t)*)) };
}
