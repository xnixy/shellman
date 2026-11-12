//! Matching a typed query against app names, and ordering the matches.
//!
//! Every whitespace-separated word of the query must match the name. A word scores by how it
//! matches, best first: the whole name, the start of the name, the start of a word in it, any
//! substring, then letters in order with gaps (a simple fuzzy match). A name's score is its
//! weakest word's, so "vis code" finds "Visual Studio Code" but ranks it below "Vis Code".

/// A match: the index in the list searched, and its score (higher is better).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    pub index: usize,
    pub score: i32,
}

const EXACT: i32 = 1000;
const PREFIX: i32 = 800;
const WORD_START: i32 = 600;
const SUBSTRING: i32 = 400;
const FUZZY: i32 = 100;

/// Rank `names` against `query`, best first, at most `limit`. Ties go to the more used app
/// (`uses`, one count per name, may be shorter), then to the shorter name, then list order.
pub fn rank(names: &[&str], uses: &dyn Fn(usize) -> u32, query: &str, limit: usize) -> Vec<Hit> {
    let words: Vec<Vec<char>> = query
        .split_whitespace()
        .map(|w| w.chars().flat_map(char::to_lowercase).collect())
        .collect();
    if words.is_empty() {
        return Vec::new();
    }
    let mut hits: Vec<Hit> = names
        .iter()
        .enumerate()
        .filter_map(|(index, name)| {
            score(name, &words).map(|score| Hit { index, score })
        })
        .collect();
    hits.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| uses(b.index).cmp(&uses(a.index)))
            .then_with(|| names[a.index].chars().count().cmp(&names[b.index].chars().count()))
            .then_with(|| a.index.cmp(&b.index))
    });
    hits.truncate(limit);
    hits
}

/// The name's score for all the words, or None when one of them does not match.
fn score(name: &str, words: &[Vec<char>]) -> Option<i32> {
    let chars: Vec<char> = name.chars().collect();
    let lower: Vec<char> = name.chars().flat_map(char::to_lowercase).collect();
    // Lowercasing can change the length for a few letters; then positions would not line up, so
    // judge word starts from the lowercase text alone.
    let starts = if lower.len() == chars.len() {
        word_starts(&chars)
    } else {
        word_starts(&lower)
    };
    let mut total = i32::MAX;
    for word in words {
        total = total.min(score_word(&lower, &starts, word)?);
    }
    // A single word is the whole query: let an exact or prefix match on the whole name show.
    if words.len() == 1 && lower == words[0] {
        return Some(EXACT);
    }
    Some(total)
}

/// Which positions begin a word: the first letter, anything after a space or punctuation, and
/// an upper-case letter after a lower-case one ("PowerToys" has two).
fn word_starts(chars: &[char]) -> Vec<bool> {
    chars
        .iter()
        .enumerate()
        .map(|(i, c)| {
            i == 0
                || !chars[i - 1].is_alphanumeric()
                || (c.is_uppercase() && chars[i - 1].is_lowercase())
                || (c.is_numeric() && chars[i - 1].is_alphabetic())
        })
        .collect()
}

fn score_word(name: &[char], starts: &[bool], word: &[char]) -> Option<i32> {
    if word.len() > name.len() {
        return None;
    }
    if name.starts_with(word) {
        // Closer in length is closer to exact.
        return Some(PREFIX - (name.len() - word.len()).min(100) as i32 / 4);
    }
    let mut best: Option<i32> = None;
    for at in 0..=(name.len() - word.len()) {
        if name[at..at + word.len()] == *word {
            let score = if starts[at] {
                WORD_START - (at as i32).min(100) / 4
            } else {
                SUBSTRING - (at as i32).min(100) / 4
            };
            best = best.max(Some(score));
        }
    }
    best.or_else(|| fuzzy(name, starts, word))
}

/// The word's letters in order, with gaps. Short words match too much by chance, so they need
/// at least three letters; a match is better when letters begin words or sit together.
fn fuzzy(name: &[char], starts: &[bool], word: &[char]) -> Option<i32> {
    if word.len() < 3 {
        return None;
    }
    let mut at = 0;
    let mut bonus = 0;
    let mut prev: Option<usize> = None;
    for &c in word {
        let found = (at..name.len()).find(|&i| name[i] == c)?;
        if prev.is_some_and(|p| found == p + 1) {
            bonus += 3;
        }
        if starts[found] {
            bonus += 4;
        }
        // Gaps cost a little.
        prev = Some(found);
        at = found + 1;
    }
    let spread = prev.map_or(0, |p| p + 1) as i32 - word.len() as i32;
    Some(FUZZY + bonus.min(60) - spread.min(40))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ranked(names: &[&str], query: &str) -> Vec<String> {
        rank(names, &|_| 0, query, 50)
            .into_iter()
            .map(|h| names[h.index].to_string())
            .collect()
    }

    #[test]
    fn empty_query_matches_nothing() {
        assert!(ranked(&["Calculator"], "").is_empty());
        assert!(ranked(&["Calculator"], "   ").is_empty());
    }

    #[test]
    fn is_case_insensitive() {
        assert_eq!(ranked(&["Calculator"], "CALC"), ["Calculator"]);
        assert_eq!(ranked(&["calculator"], "Calc"), ["calculator"]);
    }

    #[test]
    fn prefix_beats_word_start_beats_substring() {
        let names = ["Microsoft Edge", "Edge Tools", "Knowledge Base"];
        assert_eq!(
            ranked(&names, "edge"),
            ["Edge Tools", "Microsoft Edge", "Knowledge Base"]
        );
    }

    #[test]
    fn exact_name_wins() {
        let names = ["Paint 3D", "Paint", "Paintbrush"];
        assert_eq!(ranked(&names, "paint")[0], "Paint");
    }

    #[test]
    fn every_word_must_match() {
        let names = ["Visual Studio Code", "Visual Studio", "Code Editor"];
        assert_eq!(ranked(&names, "vis code"), ["Visual Studio Code"]);
    }

    #[test]
    fn camel_case_words_start_a_word() {
        let names = ["Xtoys", "PowerToys"];
        assert_eq!(ranked(&names, "toys"), ["PowerToys", "Xtoys"]);
    }

    #[test]
    fn fuzzy_finds_letters_in_order() {
        let names = ["Calculator", "Camera"];
        assert_eq!(ranked(&names, "clcr"), ["Calculator"]);
        // Two letters are too few to match by scattering.
        assert_eq!(ranked(&names, "cl"), Vec::<String>::new());
        assert_eq!(ranked(&names, "xyz"), Vec::<String>::new());
    }

    #[test]
    fn fuzzy_ranks_below_substrings() {
        let names = ["Windows Terminal", "Network Utility Control"];
        // "term" is a word start in the first; only scattered letters in the second.
        assert_eq!(ranked(&names, "term")[0], "Windows Terminal");
    }

    #[test]
    fn ties_go_to_the_more_used_then_shorter() {
        let names = ["Notes B", "Notes A", "Notes"];
        let hits = rank(&names, &|i| if i == 1 { 5 } else { 0 }, "notes", 10);
        let order: Vec<usize> = hits.iter().map(|h| h.index).collect();
        // "Notes" is exact; then the used one; then list order.
        assert_eq!(order, [2, 1, 0]);
    }

    #[test]
    fn limit_keeps_the_best() {
        let names = ["a1", "a2", "a3", "ab"];
        assert_eq!(rank(&names, &|_| 0, "ab", 1).len(), 1);
    }

    #[test]
    fn non_ascii_names() {
        assert_eq!(ranked(&["Éditeur", "Zoo"], "édi"), ["Éditeur"]);
    }
}
