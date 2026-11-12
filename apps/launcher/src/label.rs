//! Fitting an app's name under its icon: up to two lines, wrapped at word boundaries, with an
//! ellipsis only after the last line. Pure text logic (the caller says what fits), so it is
//! unit-tested.

/// Wrap `text` into at most `max_lines` lines that each satisfy `fits`. Words stay whole unless
/// one is wider than a line, which is then broken. What does not fit on the last line is cut and
/// ends with "…".
pub fn wrap(text: &str, max_lines: usize, fits: &dyn Fn(&str) -> bool) -> Vec<String> {
    let mut words: std::collections::VecDeque<String> =
        text.split_whitespace().map(str::to_owned).collect();
    let mut lines = Vec::new();
    while !words.is_empty() && lines.len() < max_lines {
        if lines.len() + 1 == max_lines {
            let rest = words.drain(..).collect::<Vec<_>>().join(" ");
            lines.push(if fits(&rest) { rest } else { ellipsize(&rest, fits) });
            break;
        }
        let mut line = String::new();
        while let Some(word) = words.pop_front() {
            let joined = if line.is_empty() { word.clone() } else { format!("{line} {word}") };
            if fits(&joined) {
                line = joined;
            } else if line.is_empty() {
                let (head, tail) = split_fitting(&word, fits);
                line = head;
                if !tail.is_empty() {
                    words.push_front(tail);
                }
                break;
            } else {
                words.push_front(word);
                break;
            }
        }
        lines.push(line);
    }
    lines
}

/// The longest start of `word` that fits (at least one letter), and the rest.
fn split_fitting(word: &str, fits: &dyn Fn(&str) -> bool) -> (String, String) {
    let chars: Vec<char> = word.chars().collect();
    let mut n = chars.len();
    while n > 1 && !fits(&chars[..n].iter().collect::<String>()) {
        n -= 1;
    }
    (chars[..n].iter().collect(), chars[n..].iter().collect())
}

/// The longest start of `text` that fits together with a trailing "…".
fn ellipsize(text: &str, fits: &dyn Fn(&str) -> bool) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut n = chars.len();
    while n > 0 {
        let head: String = chars[..n].iter().collect();
        let candidate = format!("{}…", head.trim_end());
        if fits(&candidate) {
            return candidate;
        }
        n -= 1;
    }
    "…".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ten letters to a line.
    fn ten(s: &str) -> bool {
        s.chars().count() <= 10
    }

    #[test]
    fn short_names_stay_on_one_line() {
        assert_eq!(wrap("Notepad", 2, &ten), ["Notepad"]);
    }

    #[test]
    fn wraps_at_word_boundaries() {
        assert_eq!(wrap("Visual Studio", 2, &ten), ["Visual", "Studio"]);
        assert_eq!(wrap("Windows Security", 2, &ten), ["Windows", "Security"]);
    }

    #[test]
    fn ellipsis_only_after_the_second_line() {
        assert_eq!(
            wrap("Developer Command Prompt for VS", 2, &ten),
            ["Developer", "Command P…"]
        );
        assert_eq!(wrap("One Two Three Four Five", 2, &ten), ["One Two", "Three Fou…"]);
    }

    #[test]
    fn a_word_wider_than_a_line_is_broken() {
        assert_eq!(wrap("Supercalifragilistic", 2, &ten), ["Supercalif", "ragilistic"]);
        assert_eq!(wrap("Supercalifragilisticexpi", 2, &ten), ["Supercalif", "ragilisti…"]);
    }

    #[test]
    fn empty_text_has_no_lines() {
        assert!(wrap("   ", 2, &ten).is_empty());
    }
}
