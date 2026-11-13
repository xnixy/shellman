//! Events beyond the regular reading's window, read on demand: the top bar's calendar shows a
//! whole month (and any month the user steps to), while the regular reading covers a day back to
//! [`super::AHEAD_DAYS`] ahead. A month that is not covered is read once, off the UI thread,
//! and kept for a while ([`FRESH`]); the last few months read are kept ([`KEEP`]). Pure: the
//! worker (`feed`) does the reading.

use super::expand::Instance;
use super::time::DAY;
use serde::{Deserialize, Serialize};

/// A month read on demand is read again after this long (the regular reading's period).
pub const FRESH: i64 = 15 * 60;
/// How many months read on demand are kept.
pub const KEEP: usize = 4;

/// The events of `[from, to)` (UTC seconds), read at `fetched`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub from: i64,
    pub to: i64,
    pub fetched: i64,
    pub instances: Vec<Instance>,
}

/// The window the regular reading at `fetched` covers.
pub fn regular(fetched: i64) -> (i64, i64) {
    (fetched - DAY, fetched + super::AHEAD_DAYS * DAY)
}

fn holds(have: (i64, i64), want: (i64, i64)) -> bool {
    have.0 <= want.0 && want.1 <= have.1
}

/// Whether `want` must be read: neither the regular reading (`regular`, None when there is none)
/// nor a span still fresh at `now` holds all of it.
pub fn needs_read(regular: Option<(i64, i64)>, spans: &[Span], want: (i64, i64), now: i64) -> bool {
    if want.1 <= want.0 {
        return false;
    }
    if regular.is_some_and(|r| holds(r, want)) {
        return false;
    }
    !spans.iter().any(|s| holds((s.from, s.to), want) && now - s.fetched < FRESH)
}

/// Keep `span`, in place of any it covers, and only the [`KEEP`] newest.
pub fn remember(spans: &mut Vec<Span>, span: Span) {
    spans.retain(|s| !holds((span.from, span.to), (s.from, s.to)));
    spans.push(span);
    spans.sort_by_key(|s| std::cmp::Reverse(s.fetched));
    spans.truncate(KEEP);
}

/// Whether an instance touches `[from, to)`. A point event (end = start) touches its instant.
fn touches(i: &Instance, from: i64, to: i64) -> bool {
    i.start < to && (i.end > from || (i.end == i.start && i.start >= from))
}

/// Every instance touching `[from, to)`: from the regular reading and the spans, each once (the
/// same event and start from two readings is one), sorted by start.
pub fn merged<'a>(regular: &'a [Instance], spans: &'a [Span], from: i64, to: i64) -> Vec<&'a Instance> {
    let mut out: Vec<&Instance> = Vec::new();
    // The freshest reading first, so its copy of an instance wins.
    let mut sources: Vec<&[Instance]> = vec![regular];
    let mut by_age: Vec<&Span> = spans.iter().collect();
    by_age.sort_by_key(|s| std::cmp::Reverse(s.fetched));
    sources.extend(by_age.iter().map(|s| s.instances.as_slice()));
    for list in sources {
        for i in list.iter().filter(|i| touches(i, from, to)) {
            if !out.iter().any(|o| o.uid == i.uid && o.start == i.start && o.calendar == i.calendar) {
                out.push(i);
            }
        }
    }
    out.sort_by(|a, b| a.start.cmp(&b.start).then(a.title.cmp(&b.title)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::zone::wall_at;

    fn ev(uid: &str, start: i64, mins: i64) -> Instance {
        Instance { uid: uid.into(), title: uid.into(), location: String::new(), calendar: "c".into(), start, end: start + mins * 60, days: None }
    }

    const NOW: i64 = 1_794_487_260; // 12 Nov 2026 12:41 UTC

    #[test]
    fn the_regular_window_needs_no_extra_read() {
        let r = Some(regular(NOW));
        assert!(!needs_read(r, &[], (NOW, NOW + 10 * DAY), NOW));
        // November from the 1st: before the regular window.
        let nov = (wall_at(2026, 11, 1, 0, 0), wall_at(2026, 12, 1, 0, 0));
        assert!(needs_read(r, &[], nov, NOW));
        // Nothing read yet at all: read.
        assert!(needs_read(None, &[], (NOW, NOW + DAY), NOW));
        // An empty range never is.
        assert!(!needs_read(None, &[], (NOW, NOW), NOW));
    }

    #[test]
    fn a_fresh_span_covers_until_it_is_stale() {
        let nov = (wall_at(2026, 11, 1, 0, 0), wall_at(2026, 12, 1, 0, 0));
        let spans = vec![Span { from: nov.0 - DAY, to: nov.1 + DAY, fetched: NOW, instances: vec![] }];
        assert!(!needs_read(None, &spans, nov, NOW + 60));
        assert!(needs_read(None, &spans, nov, NOW + FRESH));
        // A span that holds only part of it does not do.
        assert!(needs_read(None, &spans, (nov.0, nov.1 + 3 * DAY), NOW));
    }

    #[test]
    fn spans_are_replaced_and_capped() {
        let mut spans = Vec::new();
        for m in 0..6 {
            remember(&mut spans, Span { from: m * 40 * DAY, to: (m * 40 + 31) * DAY, fetched: NOW + m, instances: vec![] });
        }
        assert_eq!(spans.len(), KEEP);
        assert_eq!(spans[0].fetched, NOW + 5, "newest first");
        // The same month read again replaces the old reading.
        remember(&mut spans, Span { from: 200 * DAY, to: 231 * DAY, fetched: NOW + 9, instances: vec![] });
        assert_eq!(spans.iter().filter(|s| s.from == 200 * DAY).count(), 1);
    }

    #[test]
    fn readings_merge_without_doubles() {
        let a = ev("standup", wall_at(2026, 11, 13, 9, 0), 15);
        let b = ev("review", wall_at(2026, 11, 2, 14, 0), 60);
        let old_a = Instance { title: "old title".into(), ..a.clone() };
        let spans = vec![Span { from: 0, to: i64::MAX / 2, fetched: NOW - 100, instances: vec![b.clone(), old_a] }];
        let regular = vec![a.clone()];
        let from = wall_at(2026, 11, 1, 0, 0);
        let all = merged(&regular, &spans, from, from + 30 * DAY);
        assert_eq!(all.iter().map(|i| i.title.as_str()).collect::<Vec<_>>(), ["review", "standup"]);
        // Out of range: left out. A point event at the range's start is in.
        let point = [ev("deadline", from, 0)];
        assert_eq!(merged(&point, &[], from, from + DAY).len(), 1);
        assert!(merged(&regular, &[], from, from + DAY).is_empty());
    }
}
