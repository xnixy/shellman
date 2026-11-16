//! The look's prefs in a profile's `user.js`: one block between marker comments, appended to
//! whatever the file already holds and taken out again byte for byte. Everything works on bytes,
//! so a file in another encoding or with mixed line endings comes back exactly as it was.
//!
//! The begin marker is matched by its start only: `install.ps1` wrote a longer text after it, and
//! either tool removes the other's block.

/// What the begin marker line says.
pub const BEGIN: &str = "// >>> shellman-islands begin (managed by shellman, do not edit) >>>";
/// The part of the begin marker both tools match.
const BEGIN_PREFIX: &[u8] = b"// >>> shellman-islands begin";
pub const END: &str = "// <<< shellman-islands end <<<";

fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    hay.get(from..)?.windows(needle.len()).position(|w| w == needle).map(|i| i + from)
}

/// `text` without any of our blocks. A block runs from the begin marker to the first end marker
/// after it, plus the spaces, tabs and one line break that follow (as install.ps1's pattern). A
/// begin marker with no end marker after it is left alone.
pub fn strip(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut at = 0;
    while let Some(b) = find(text, BEGIN_PREFIX, at) {
        let Some(e) = find(text, END.as_bytes(), b) else { break };
        out.extend_from_slice(&text[at..b]);
        let mut j = e + END.len();
        while matches!(text.get(j), Some(b' ' | b'\t')) {
            j += 1;
        }
        if text.get(j) == Some(&b'\r') {
            j += 1;
        }
        if text.get(j) == Some(&b'\n') {
            j += 1;
        }
        at = j;
    }
    out.extend_from_slice(&text[at..]);
    out
}

/// The block for `prefs` (the shipped `user.js`), its trailing white space trimmed.
pub fn block(prefs: &[u8]) -> Vec<u8> {
    let end = prefs.iter().rposition(|b| !b.is_ascii_whitespace()).map_or(0, |i| i + 1);
    let mut b = Vec::with_capacity(prefs.len() + 120);
    b.extend_from_slice(BEGIN.as_bytes());
    b.extend_from_slice(b"\r\n");
    b.extend_from_slice(&prefs[..end]);
    b.extend_from_slice(b"\r\n");
    b.extend_from_slice(END.as_bytes());
    b.extend_from_slice(b"\r\n");
    b
}

/// `existing` (None: no file) with our block, replacing any block already there. Also says
/// whether a line break had to be added after the existing text (it did not end in one), which
/// removal takes away again.
pub fn with_block(existing: Option<&[u8]>, prefs: &[u8]) -> (Vec<u8>, bool) {
    let mut rest = existing.map(strip).unwrap_or_default();
    let added = !rest.is_empty() && rest.last() != Some(&b'\n');
    if added {
        rest.extend_from_slice(b"\r\n");
    }
    rest.extend_from_slice(&block(prefs));
    (rest, added)
}

/// `text` without our block, and without the line break install added (`added_break`).
pub fn without_block(text: &[u8], added_break: bool) -> Vec<u8> {
    let mut rest = strip(text);
    if added_break && rest.ends_with(b"\r\n") {
        rest.truncate(rest.len() - 2);
    }
    rest
}

/// Whether `text` holds exactly the block for `prefs`.
pub fn has_block(text: &[u8], prefs: &[u8]) -> bool {
    let b = block(prefs);
    text.windows(b.len()).any(|w| w == b.as_slice())
}

/// Whether nothing but white space is left.
pub fn is_blank(text: &[u8]) -> bool {
    text.iter().all(u8::is_ascii_whitespace)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PREFS: &[u8] = b"// theme\nuser_pref(\"a\", true);\n\n";

    #[test]
    fn a_block_goes_after_what_is_there_and_comes_out_exactly() {
        for before in [&b"user_pref(\"mine\", 1);\r\n"[..], b"user_pref(\"mine\", 1);", b"// lf only\nx\n", b"\xEF\xBB\xBFbom\n"] {
            let (with, added) = with_block(Some(before), PREFS);
            assert_eq!(added, !before.ends_with(b"\n"));
            assert!(has_block(&with, PREFS));
            assert_eq!(without_block(&with, added), before, "{:?}", String::from_utf8_lossy(before));
        }
    }

    #[test]
    fn a_new_file_is_just_the_block() {
        let (with, added) = with_block(None, PREFS);
        assert!(!added);
        assert_eq!(with, block(PREFS));
        assert!(is_blank(&without_block(&with, false)));
    }

    #[test]
    fn installing_again_replaces_the_block_and_keeps_later_lines() {
        let (once, added) = with_block(Some(b"a\n"), b"old\n");
        let mut edited = once.clone();
        edited.extend_from_slice(b"user_pref(\"added later\", 2);\n");
        let (twice, again) = with_block(Some(&edited), PREFS);
        assert!(!added && !again);
        assert!(has_block(&twice, PREFS) && !has_block(&twice, b"old\n"));
        assert_eq!(without_block(&twice, false), b"a\nuser_pref(\"added later\", 2);\n");
    }

    #[test]
    fn the_powershell_scripts_block_is_recognised() {
        let ps = b"x\r\n// >>> shellman-islands begin (managed by integrations/firefox/install.ps1, do not edit) >>>\r\nuser_pref(\"a\", true);\r\n// <<< shellman-islands end <<<  \r\ny\r\n";
        assert_eq!(strip(ps), b"x\r\ny\r\n");
    }

    #[test]
    fn a_begin_without_an_end_is_left_alone() {
        let t = b"a\n// >>> shellman-islands begin\nb\n";
        assert_eq!(strip(t), t);
    }

    #[test]
    fn the_block_trims_the_prefs_trailing_space() {
        let b = block(b"p\n\n  \n");
        assert_eq!(b, format!("{BEGIN}\r\np\r\n{END}\r\n").into_bytes());
    }
}
