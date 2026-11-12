//! The search field: does a process match what was typed.

use super::Proc;

/// True when every word of `query` is found (ignoring case) in the process's name, friendly
/// name, window title or command line, or a word is its PID. An empty query matches everything.
pub fn matches(proc: &Proc, query: &str) -> bool {
    let pid = proc.key.pid.to_string();
    query.split_whitespace().all(|word| {
        let word = word.to_lowercase();
        word == pid
            || contains(&proc.name, &word)
            || contains(&proc.description, &word)
            || proc.windows.iter().any(|w| contains(&w.title, &word))
            || contains(&proc.command_line, &word)
    })
}

fn contains(haystack: &str, lower_needle: &str) -> bool {
    !lower_needle.is_empty() && haystack.to_lowercase().contains(lower_needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::procs::{Key, Window};

    fn firefox() -> Proc {
        Proc {
            key: Key { pid: 4312, created: 1 },
            name: "firefox.exe".into(),
            description: "Firefox".into(),
            command_line: r#""C:\Program Files\Mozilla Firefox\firefox.exe" -contentproc"#.into(),
            windows: vec![Window { hwnd: 1, title: "Islands — Mozilla Firefox".into(), hung: false }],
            ..Default::default()
        }
    }

    #[test]
    fn by_name_title_pid_or_command_line() {
        let p = firefox();
        assert!(matches(&p, ""));
        assert!(matches(&p, "FIRE"));
        assert!(matches(&p, "islands"));
        assert!(matches(&p, "4312"));
        assert!(matches(&p, "contentproc"));
        assert!(matches(&p, "mozilla contentproc"), "every word, anywhere");
    }

    #[test]
    fn misses() {
        let p = firefox();
        assert!(!matches(&p, "chrome"));
        assert!(!matches(&p, "431"), "a PID matches whole, not as part of another number");
        assert!(!matches(&p, "firefox chrome"));
    }
}
