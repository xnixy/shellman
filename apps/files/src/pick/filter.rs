//! File type patterns: `*.txt`, `*.*`, `report-??.pdf`. Matching ignores case, as Windows does.

/// A file type's patterns, ready to match names against.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Patterns {
    /// Lower-cased patterns, as characters. Empty when everything matches.
    pats: Vec<Vec<char>>,
}

impl Patterns {
    /// Patterns that match every name.
    pub fn all() -> Self {
        Self::default()
    }

    /// From a type's patterns. `*` or `*.*` among them, or none at all, matches everything
    /// (`*.*` also matches names without a dot, as on Windows).
    pub fn new<S: AsRef<str>>(patterns: &[S]) -> Self {
        let mut pats = Vec::new();
        for p in patterns {
            let p = p.as_ref().trim();
            if p.is_empty() {
                continue;
            }
            if p == "*" || p == "*.*" {
                return Self::all();
            }
            pats.push(lower(p));
        }
        Self { pats }
    }

    /// From what the user typed in the name field: `*.log` or `*.txt;*.md`.
    pub fn typed(text: &str) -> Self {
        let parts: Vec<&str> = text.split(';').collect();
        Self::new(&parts)
    }

    /// Whether `name` (a file name, not a path) matches any pattern.
    pub fn matches(&self, name: &str) -> bool {
        if self.pats.is_empty() {
            return true;
        }
        let name = lower(name);
        self.pats.iter().any(|p| glob(p, &name))
    }
}

fn lower(s: &str) -> Vec<char> {
    s.chars().flat_map(char::to_lowercase).collect()
}

/// `*` is any run of characters, `?` any one character; the rest must be equal.
fn glob(pat: &[char], name: &[char]) -> bool {
    let (mut p, mut n) = (0, 0);
    // Where the last `*` was, and how much of the name it had taken, to back up to.
    let mut star: Option<(usize, usize)> = None;
    while n < name.len() {
        if p < pat.len() && (pat[p] == '?' || pat[p] == name[n]) {
            p += 1;
            n += 1;
        } else if p < pat.len() && pat[p] == '*' {
            star = Some((p, n));
            p += 1;
        } else if let Some((sp, sn)) = star {
            p = sp + 1;
            n = sn + 1;
            star = Some((sp, sn + 1));
        } else {
            return false;
        }
    }
    pat[p..].iter().all(|&c| c == '*')
}

/// Whether text holds a wildcard, which makes typed text a filter rather than a name.
pub fn has_wildcard(text: &str) -> bool {
    text.contains('*') || text.contains('?')
}

/// The extension a type stands for, when its first pattern is a plain `*.ext` ("txt" for
/// `*.txt`); none for `*.*` or a pattern with wildcards in its extension.
pub fn type_extension<S: AsRef<str>>(patterns: &[S]) -> Option<String> {
    let first = patterns.first()?.as_ref().trim();
    let ext = first.strip_prefix("*.")?;
    if ext.is_empty() || has_wildcard(ext) || ext.contains('.') {
        return None;
    }
    Some(ext.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_match_without_regard_to_case() {
        let p = Patterns::new(&["*.txt", "*.md"]);
        assert!(p.matches("notes.txt"));
        assert!(p.matches("README.MD"));
        assert!(p.matches("Notes.Txt"));
        assert!(!p.matches("notes.txt.bak"));
        assert!(!p.matches("txt"));
        assert!(!p.matches("image.png"));
    }

    #[test]
    fn star_dot_star_and_star_match_everything() {
        for all in [Patterns::new(&["*.*"]), Patterns::new(&["*"]), Patterns::new::<&str>(&[])] {
            assert_eq!(all, Patterns::all());
            assert!(all.matches("Makefile"));
            assert!(all.matches("a.b.c"));
        }
        // `*.*` anywhere in the list wins.
        assert!(Patterns::new(&["*.txt", "*.*"]).matches("photo.jpg"));
    }

    #[test]
    fn question_marks_and_inner_stars() {
        let p = Patterns::new(&["report-??.pdf"]);
        assert!(p.matches("Report-07.PDF"));
        assert!(!p.matches("report-7.pdf"));
        let p = Patterns::new(&["a*b*c.log"]);
        assert!(p.matches("a--b--c.log"));
        assert!(p.matches("abc.log"));
        assert!(p.matches("ab_b_xc.log"));
        assert!(!p.matches("acb.log"));
        // Exact names work too.
        assert!(Patterns::new(&["Makefile"]).matches("makefile"));
    }

    #[test]
    fn typed_filters_split_on_semicolons() {
        let p = Patterns::typed("*.rs; *.toml");
        assert!(p.matches("main.rs") && p.matches("Cargo.toml") && !p.matches("a.md"));
        assert!(has_wildcard("*.rs") && has_wildcard("a?.txt") && !has_wildcard("a.txt"));
    }

    #[test]
    fn non_ascii_names_fold_case() {
        let p = Patterns::new(&["*.ÄBC"]);
        assert!(p.matches("straße.äbc"));
    }

    #[test]
    fn a_type_stands_for_its_first_plain_extension() {
        assert_eq!(type_extension(&["*.txt", "*.md"]).as_deref(), Some("txt"));
        assert_eq!(type_extension(&["*.*"]), None);
        assert_eq!(type_extension(&["*.tx?"]), None);
        assert_eq!(type_extension(&["notes.txt"]), None);
        assert_eq!(type_extension::<&str>(&[]), None);
        assert_eq!(type_extension(&["*.tar.gz"]), None);
    }
}
