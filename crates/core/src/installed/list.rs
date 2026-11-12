//! Putting the list together and showing part of it: merging the two sources without doubles,
//! and searching and sorting what the page shows. Pure.

use super::app::App;
use std::cmp::Ordering;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Sort {
    #[default]
    Name,
    /// Largest first; programs with no size last.
    Size,
    /// Newest first; programs with no date last.
    Date,
}

/// Merge the sources: the same name and version seen twice (the 32- and 64-bit views of the
/// registry list some programs in both) is one entry, the first one seen.
pub fn merge(sources: Vec<Vec<App>>) -> Vec<App> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for app in sources.into_iter().flatten() {
        if seen.insert((app.name.to_lowercase(), app.version.to_lowercase())) {
            out.push(app);
        }
    }
    out
}

fn by_name(a: &App, b: &App) -> Ordering {
    a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.id.cmp(&b.id))
}

/// The indexes into `apps` the page shows: those matching the search (every word of it must be in
/// the name or the publisher), in the chosen order.
pub fn view(apps: &[App], query: &str, sort: Sort) -> Vec<usize> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    let mut shown: Vec<usize> = (0..apps.len())
        .filter(|&i| {
            let hay = format!("{} {}", apps[i].name, apps[i].publisher).to_lowercase();
            words.iter().all(|w| hay.contains(w))
        })
        .collect();
    shown.sort_by(|&a, &b| {
        let (a, b) = (&apps[a], &apps[b]);
        match sort {
            Sort::Name => by_name(a, b),
            Sort::Size => b.size_kb.cmp(&a.size_kb).then_with(|| by_name(a, b)),
            Sort::Date => b.date.cmp(&a.date).then_with(|| by_name(a, b)),
        }
    });
    shown
}

#[cfg(test)]
mod tests {
    use super::super::app::tests::classic;
    use super::*;
    use crate::installed::app::Classic;

    fn app(name: &str, publisher: &str, version: &str, size: Option<u64>, date: Option<u32>) -> App {
        let mut a = classic(name, Classic::default());
        a.publisher = publisher.into();
        a.version = version.into();
        a.size_kb = size;
        a.date = date;
        a
    }

    fn names(apps: &[App], idx: &[usize]) -> Vec<String> {
        idx.iter().map(|&i| apps[i].name.clone()).collect()
    }

    #[test]
    fn doubles_collapse_to_the_first() {
        let mut dup = app("acme tool", "", "1.0", None, None);
        dup.id = "other".into();
        let merged = merge(vec![vec![app("Acme Tool", "", "1.0", None, None)], vec![dup, app("Acme Tool", "", "2.0", None, None)]]);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].id, "Acme Tool");
    }

    fn sample() -> Vec<App> {
        vec![
            app("beta", "Zed Corp", "1", Some(500), Some(20230101)),
            app("Alpha", "Acme", "1", None, Some(20240101)),
            app("gamma", "Acme", "1", Some(9000), None),
        ]
    }

    #[test]
    fn sorted_by_name_ignores_case() {
        let apps = sample();
        assert_eq!(names(&apps, &view(&apps, "", Sort::Name)), ["Alpha", "beta", "gamma"]);
    }

    #[test]
    fn size_and_date_put_the_unknown_last() {
        let apps = sample();
        assert_eq!(names(&apps, &view(&apps, "", Sort::Size)), ["gamma", "beta", "Alpha"]);
        assert_eq!(names(&apps, &view(&apps, "", Sort::Date)), ["Alpha", "beta", "gamma"]);
    }

    #[test]
    fn search_looks_at_name_and_publisher_and_wants_every_word() {
        let apps = sample();
        assert_eq!(names(&apps, &view(&apps, "ACME", Sort::Name)), ["Alpha", "gamma"]);
        assert_eq!(names(&apps, &view(&apps, "acme gam", Sort::Name)), ["gamma"]);
        assert_eq!(names(&apps, &view(&apps, "zed", Sort::Name)), ["beta"]);
        assert!(view(&apps, "nothing", Sort::Name).is_empty());
        assert_eq!(view(&apps, "   ", Sort::Name).len(), 3);
    }
}
