//! `SHELLMAN_TERMINAL_SCRIPT`: a preview of the window's states without driving the keyboard.
//! A comma-separated list of steps, run one a frame once the window is laid out: shortcut names
//! (`split-across`, `split-down`, `split-auto`, `focus-left|right|up|down`,
//! `resize-left|right|up|down`, `close-pane`, `new-tab`, `next-tab`, `find`, `focus-chrome`), `type:<text>`
//! (written to the focused pane, `\n` for Enter) and `find:<text>` (opens find with that query).
//! Synthetic keys would go to whichever window has the foreground; this goes only to this one.

use super::input::Shortcut;
use crate::splits::Dir;

#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    Key(Shortcut),
    Type(String),
    Find(String),
}

/// The steps in `spec`; unknown ones are skipped (and said on stderr).
pub fn parse(spec: &str) -> Vec<Step> {
    spec.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .filter_map(|s| {
            let step = step(s);
            if step.is_none() {
                eprintln!("terminal: SHELLMAN_TERMINAL_SCRIPT: unknown step {s:?}");
            }
            step
        })
        .collect()
}

fn step(s: &str) -> Option<Step> {
    if let Some(t) = s.strip_prefix("type:") {
        return Some(Step::Type(t.replace("\\n", "\r")));
    }
    if let Some(t) = s.strip_prefix("find:") {
        return Some(Step::Find(t.to_owned()));
    }
    let dir = |d: &str| match d {
        "left" => Some(Dir::Left),
        "right" => Some(Dir::Right),
        "up" => Some(Dir::Up),
        "down" => Some(Dir::Down),
        _ => None,
    };
    Some(Step::Key(match s {
        "split-across" => Shortcut::SplitAcross,
        "split-down" => Shortcut::SplitDown,
        "split-auto" => Shortcut::SplitAuto,
        "close-pane" => Shortcut::CloseTab,
        "new-tab" => Shortcut::NewTab,
        "next-tab" => Shortcut::NextTab,
        "find" => Shortcut::Find,
        "focus-chrome" => Shortcut::FocusChrome,
        _ => {
            if let Some(d) = s.strip_prefix("focus-") {
                Shortcut::FocusPane(dir(d)?)
            } else if let Some(d) = s.strip_prefix("resize-") {
                Shortcut::ResizePane(dir(d)?)
            } else {
                return None;
            }
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_parse() {
        assert_eq!(
            parse("split-across, focus-left,type:dir\\n,find:hello,bogus,resize-up"),
            vec![
                Step::Key(Shortcut::SplitAcross),
                Step::Key(Shortcut::FocusPane(Dir::Left)),
                Step::Type("dir\r".into()),
                Step::Find("hello".into()),
                Step::Key(Shortcut::ResizePane(Dir::Up)),
            ]
        );
        assert!(parse("").is_empty());
    }
}
