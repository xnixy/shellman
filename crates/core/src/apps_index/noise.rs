//! Which entries of the Apps folder are not apps anybody launches from a grid: help files,
//! release notes, website links, uninstallers. They stay searchable; the launcher only leaves
//! them out of its empty-query grid.

use super::App;

impl App {
    /// Whether the app belongs in the browsable grid (as opposed to search results only).
    pub fn is_listed(&self) -> bool {
        !is_noise(&self.name, &self.id)
    }
}

/// The whole rule list: words in the name, and what the shortcut points at.
fn is_noise(name: &str, id: &str) -> bool {
    let name = name.to_lowercase();
    const WORDS: [&str; 12] = [
        "help",
        "documentation",
        "readme",
        "read me",
        "release notes",
        "faq",
        "license",
        "website",
        "get started",
        "uninstall",
        "manual",
        "changelog",
    ];
    let word = WORDS.iter().any(|w| contains_word(&name, w));
    let web = name.starts_with("visit ") || name.ends_with(" on the web") || name.ends_with(" online");
    let id = id.trim().to_lowercase();
    const DOCS: [&str; 8] = [".url", ".chm", ".html", ".htm", ".txt", ".pdf", ".rtf", ".md"];
    let document = id.starts_with("http") || DOCS.iter().any(|e| id.ends_with(e));
    word || web || document
}

/// `word` appears in `text` with non-letters (or the ends) around it, so "Helpful Tool" and
/// "Faqir" are not caught by "help" and "faq". A trailing "s" is allowed ("FAQs").
fn contains_word(text: &str, word: &str) -> bool {
    text.match_indices(word).any(|(at, _)| {
        let edge = |c: Option<char>| c.is_none_or(|c| !c.is_alphanumeric());
        let before = edge(text[..at].chars().next_back());
        let rest = &text[at + word.len()..];
        let after = edge(rest.chars().next())
            || (rest.starts_with('s') && edge(rest[1..].chars().next()));
        before && after
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listed(name: &str, id: &str) -> bool {
        App { name: name.into(), id: id.into() }.is_listed()
    }

    #[test]
    fn real_apps_are_listed() {
        assert!(listed("7-Zip File Manager", r"C:\Program Files\7-Zip\7zFM.exe"));
        assert!(listed("Calculator", "Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"));
        assert!(listed("Helpful Tool", "x"));
        assert!(listed("Git Bash", "x"));
        assert!(listed("Faqir", "x"));
    }

    #[test]
    fn help_and_docs_are_hidden() {
        for n in [
            "7-Zip Help",
            "Documentation",
            "Git FAQs",
            "Git Release Notes",
            "Get Started",
            "Uninstall Foo",
            "Readme",
            "Visit our site",
            "Foo on the Web",
            "License Agreement",
        ] {
            assert!(!listed(n, "x"), "{n}");
        }
    }

    #[test]
    fn shortcuts_to_documents_and_links_are_hidden() {
        assert!(!listed("Foo", r"C:\x\Foo.chm"));
        assert!(!listed("Foo", r"C:\x\Foo.URL"));
        assert!(!listed("Foo", "https://example.com"));
        assert!(!listed("Foo", r"C:\x\notes.txt"));
        assert!(listed("Foo", r"C:\x\foo.exe"));
    }
}
