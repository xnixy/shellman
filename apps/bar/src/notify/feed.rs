//! What the notifications show, and when: the store's toasts, which are unseen (the badge's
//! count), which pop up and for how long, and whether the list is open. Pure: time comes in as
//! arguments, so it is tested without windows or clocks.

use shellman_core::notify::Toast;
use std::collections::BTreeSet;
use std::time::{Duration, Instant};

/// How long a popup stays up (paused while the pointer is over it).
pub const SHOW_FOR: Duration = Duration::from_secs(6);
/// How long it takes to slide in, and to slide away.
pub const SLIDE_IN: Duration = Duration::from_millis(180);
pub const SLIDE_OUT: Duration = Duration::from_millis(220);
/// At most this many popups at once; more push the oldest away.
pub const MAX_POPUPS: usize = 3;

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Popup {
    pub id: u32,
    pub appeared: Instant,
    /// When it starts to slide away, unless paused.
    pub until: Instant,
    pub leaving: Option<Instant>,
}

#[derive(Default)]
pub struct Feed {
    /// Oldest first.
    toasts: Vec<Toast>,
    unseen: BTreeSet<u32>,
    popups: Vec<Popup>,
    /// The first read has come (toasts already there then do not pop up).
    loaded: bool,
    pub dnd: bool,
    list_open: bool,
    /// Since when the popups have been held (pointer over them, or a reply being typed).
    paused: Option<Instant>,
}

/// One app's notifications, newest first.
pub struct Group<'a> {
    pub app_id: &'a str,
    pub toasts: Vec<&'a Toast>,
}

impl Feed {
    pub fn with_dnd(dnd: bool) -> Self {
        Self { dnd, ..Default::default() }
    }

    pub fn toasts(&self) -> &[Toast] {
        &self.toasts
    }

    pub fn toast(&self, id: u32) -> Option<&Toast> {
        self.toasts.iter().find(|t| t.id == id)
    }

    pub fn unseen(&self) -> usize {
        self.unseen.len()
    }

    pub fn list_open(&self) -> bool {
        self.list_open
    }

    /// Whether the window has anything to show.
    pub fn visible(&self) -> bool {
        self.list_open || !self.popups.is_empty()
    }

    pub fn popups(&self) -> &[Popup] {
        &self.popups
    }

    /// The store as read now. New toasts are unseen; they pop up unless do-not-disturb is on, the
    /// list is open, or `quiet` (a fullscreen app is in front). Toasts gone from the store go.
    /// True if anything changed.
    pub fn update(&mut self, toasts: Vec<Toast>, now: Instant, quiet: bool) -> bool {
        let same = toasts == self.toasts;
        let first = !self.loaded;
        self.loaded = true;
        for t in &toasts {
            if self.toasts.iter().any(|o| o.id == t.id) {
                continue;
            }
            self.unseen.insert(t.id);
            if !first && !self.dnd && !self.list_open && !quiet {
                self.popups.push(Popup { id: t.id, appeared: now, until: now + SHOW_FOR, leaving: None });
            }
        }
        let alive = |id: &u32| toasts.iter().any(|t| t.id == *id);
        self.unseen.retain(alive);
        self.popups.retain(|p| alive(&p.id));
        self.toasts = toasts;
        self.crowd(now);
        self.unhold_if_empty();
        !same
    }

    /// Too many up: the oldest slide away.
    fn crowd(&mut self, now: Instant) {
        let staying = self.popups.iter().filter(|p| p.leaving.is_none()).count();
        let extra = staying.saturating_sub(MAX_POPUPS);
        for p in self.popups.iter_mut().filter(|p| p.leaving.is_none()).take(extra) {
            p.leaving = Some(now);
        }
    }

    /// Let the timers run. True if anything changed.
    pub fn tick(&mut self, now: Instant) -> bool {
        let before = self.popups.clone();
        if self.paused.is_none() {
            for p in self.popups.iter_mut().filter(|p| p.leaving.is_none() && now >= p.until) {
                p.leaving = Some(now);
            }
        }
        self.popups.retain(|p| p.leaving.is_none_or(|l| now < l + SLIDE_OUT));
        self.unhold_if_empty();
        self.popups != before
    }

    /// Hold the popups (true) or let them run again; held time is added to their stay. A popup
    /// that came while they were held is only held from when it came.
    pub fn hold(&mut self, held: bool, now: Instant) {
        match (held, self.paused) {
            (true, None) if !self.popups.is_empty() => self.paused = Some(now),
            (false, Some(since)) => {
                self.paused = None;
                for p in &mut self.popups {
                    p.until += now.saturating_duration_since(since.max(p.appeared));
                }
            }
            _ => {}
        }
    }

    /// With no popups there is nothing to hold: a hold left over (the pointer was on a toast
    /// that was clicked or dismissed, and no frame came to let go) must not stall the next one.
    fn unhold_if_empty(&mut self) {
        if self.popups.is_empty() {
            self.paused = None;
        }
    }

    /// When the next timer is due, if any: a popup's stay ending, or its slide finishing.
    pub fn due(&self, now: Instant) -> Option<Duration> {
        self.popups
            .iter()
            .filter_map(|p| match p.leaving {
                Some(l) => Some(l + SLIDE_OUT),
                None if self.paused.is_none() => Some(p.until),
                None => None,
            })
            .min()
            .map(|at| at.saturating_duration_since(now))
    }

    /// Whether something is sliding, so frames are needed until it is still.
    pub fn moving(&self, now: Instant) -> bool {
        self.popups.iter().any(|p| p.leaving.is_some() || now < p.appeared + SLIDE_IN)
    }

    /// How far a popup is out of place, 0 (in place) to 1 (off to the side).
    pub fn offset(p: &Popup, now: Instant) -> f32 {
        let t = |since: Instant, over: Duration| (now - since).as_secs_f32() / over.as_secs_f32();
        let f = match p.leaving {
            Some(l) => t(l, SLIDE_OUT).min(1.0),
            None => 1.0 - t(p.appeared, SLIDE_IN).min(1.0),
        };
        // Ease out: fast at first, settling gently.
        let f = f.clamp(0.0, 1.0);
        if p.leaving.is_some() { f * f } else { f * f * f }
    }

    /// Gone from view at once (dismissed or clicked); the store follows.
    pub fn remove(&mut self, id: u32) {
        self.toasts.retain(|t| t.id != id);
        self.unseen.remove(&id);
        self.popups.retain(|p| p.id != id);
        self.unhold_if_empty();
    }

    pub fn clear(&mut self) {
        self.toasts.clear();
        self.unseen.clear();
        self.popups.clear();
        self.paused = None;
    }

    /// Open the list: everything is seen, and the popups give way to it.
    pub fn open_list(&mut self) {
        self.list_open = true;
        self.unseen.clear();
        self.popups.clear();
        self.paused = None;
    }

    pub fn close_list(&mut self) {
        self.list_open = false;
        self.paused = None;
    }

    /// The list's groups: one per app, the app with the newest notification first.
    pub fn groups(&self) -> Vec<Group<'_>> {
        let mut groups: Vec<Group> = Vec::new();
        for t in self.toasts.iter().rev() {
            match groups.iter_mut().find(|g| g.app_id == t.app_id) {
                Some(g) => g.toasts.push(t),
                None => groups.push(Group { app_id: &t.app_id, toasts: vec![t] }),
            }
        }
        groups
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shellman_core::notify::source::sample;

    fn ids(feed: &Feed) -> Vec<u32> {
        feed.popups().iter().map(|p| p.id).collect()
    }

    #[test]
    fn what_was_there_at_start_is_counted_but_does_not_pop_up() {
        let now = Instant::now();
        let mut f = Feed::default();
        assert!(f.update(vec![sample(1, "a", "old", 1)], now, false));
        assert_eq!((f.unseen(), f.visible()), (1, false));
        f.update(vec![sample(1, "a", "old", 1), sample(2, "a", "new", 2)], now, false);
        assert_eq!((f.unseen(), ids(&f)), (2, vec![2]));
        // The same read again changes nothing.
        assert!(!f.update(f.toasts().to_vec(), now, false));
    }

    #[test]
    fn a_popup_stays_six_seconds_then_slides_away() {
        let t0 = Instant::now();
        let mut f = Feed::default();
        f.update(vec![], t0, false);
        f.update(vec![sample(1, "a", "hi", 1)], t0, false);
        assert_eq!(f.due(t0), Some(SHOW_FOR));
        assert!(!f.tick(t0 + Duration::from_secs(5)));
        assert!(f.tick(t0 + SHOW_FOR));
        assert!(f.popups()[0].leaving.is_some() && f.moving(t0 + SHOW_FOR));
        f.tick(t0 + SHOW_FOR + SLIDE_OUT);
        assert!(!f.visible());
        // Still in the store, still unseen.
        assert_eq!((f.toasts().len(), f.unseen()), (1, 1));
    }

    #[test]
    fn hovering_holds_the_popups_and_adds_the_time_back() {
        let t0 = Instant::now();
        let mut f = Feed::default();
        f.update(vec![], t0, false);
        f.update(vec![sample(1, "a", "hi", 1)], t0, false);
        f.hold(true, t0 + Duration::from_secs(5));
        assert_eq!(f.due(t0 + Duration::from_secs(5)), None);
        f.tick(t0 + Duration::from_secs(30));
        assert!(f.popups()[0].leaving.is_none());
        f.hold(false, t0 + Duration::from_secs(30));
        // One second was left when the pointer came.
        assert!(!f.tick(t0 + Duration::from_millis(30_900)));
        assert!(f.tick(t0 + Duration::from_secs(31)));
    }

    #[test]
    fn a_hold_left_by_a_clicked_toast_does_not_stall_the_next() {
        let t0 = Instant::now();
        let at = |s: u64| t0 + Duration::from_secs(s);
        let mut f = Feed::default();
        f.update(vec![], t0, false);
        f.update(vec![sample(1, "a", "hi", 1)], t0, false);
        // The pointer rests on it and clicks it: it goes, and no frame lets go of the hold.
        f.hold(true, at(2));
        f.remove(1);
        assert!(!f.visible());
        // Hours later a new toast comes; its first frame lets go of a hold that is no more.
        f.update(vec![sample(2, "a", "next", 2)], at(3 * 3600), false);
        f.hold(false, at(3 * 3600));
        assert_eq!(f.due(at(3 * 3600)), Some(SHOW_FOR), "a normal stay, not hours");
        assert!(f.tick(at(3 * 3600) + SHOW_FOR));
        // Cleared or slid away while held: the same.
        f.tick(at(3 * 3600) + SHOW_FOR + SLIDE_OUT);
        assert!(f.popups().is_empty() && f.paused.is_none());
    }

    #[test]
    fn a_popup_that_came_during_a_hold_is_held_only_from_then() {
        let t0 = Instant::now();
        let at = |s: u64| t0 + Duration::from_secs(s);
        let mut f = Feed::default();
        f.update(vec![], t0, false);
        f.update(vec![sample(1, "a", "x", 1)], t0, false);
        f.hold(true, at(1));
        f.update(vec![sample(1, "a", "x", 1), sample(2, "a", "y", 2)], at(20), false);
        f.hold(false, at(30));
        let until = |id| f.popups().iter().find(|p| p.id == id).unwrap().until;
        assert_eq!(until(1), t0 + SHOW_FOR + Duration::from_secs(29));
        assert_eq!(until(2), at(20) + SHOW_FOR + Duration::from_secs(10));
    }

    #[test]
    fn do_not_disturb_counts_without_popping_up() {
        let now = Instant::now();
        let mut f = Feed::with_dnd(true);
        f.update(vec![], now, false);
        f.update(vec![sample(1, "a", "hi", 1)], now, false);
        assert_eq!((f.unseen(), f.visible()), (1, false));
        // Nor while a fullscreen app is in front.
        let mut f = Feed::default();
        f.update(vec![], now, false);
        f.update(vec![sample(1, "a", "hi", 1)], now, true);
        assert_eq!((f.unseen(), f.visible()), (1, false));
    }

    #[test]
    fn at_most_three_popups_the_oldest_give_way() {
        let now = Instant::now();
        let mut f = Feed::default();
        f.update(vec![], now, false);
        let all: Vec<_> = (1..=4).map(|i| sample(i, "a", "x", i as u64)).collect();
        f.update(all, now, false);
        let leaving: Vec<u32> = f.popups().iter().filter(|p| p.leaving.is_some()).map(|p| p.id).collect();
        assert_eq!(leaving, [1]);
    }

    #[test]
    fn opening_the_list_sees_everything_and_closes_popups() {
        let now = Instant::now();
        let mut f = Feed::default();
        f.update(vec![], now, false);
        f.update(vec![sample(1, "a", "x", 1), sample(2, "b", "y", 2), sample(3, "a", "z", 3)], now, false);
        f.open_list();
        assert_eq!((f.unseen(), f.popups().len(), f.visible()), (0, 0, true));
        let groups = f.groups();
        assert_eq!(groups.iter().map(|g| g.app_id).collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!(groups[0].toasts.iter().map(|t| t.id).collect::<Vec<_>>(), [3, 1]);
        // New toasts while it is open join it without popping up.
        f.update(vec![sample(1, "a", "x", 1), sample(4, "c", "w", 4)], now, false);
        assert_eq!((f.popups().len(), f.unseen()), (0, 1));
        f.close_list();
        assert!(!f.visible());
    }

    #[test]
    fn toasts_gone_from_the_store_go_from_view() {
        let now = Instant::now();
        let mut f = Feed::default();
        f.update(vec![], now, false);
        f.update(vec![sample(1, "a", "x", 1)], now, false);
        f.update(vec![], now, false);
        assert_eq!((f.unseen(), f.visible()), (0, false));
        f.update(vec![sample(2, "a", "x", 1)], now, false);
        f.remove(2);
        assert_eq!((f.unseen(), f.visible(), f.toasts().len()), (0, false, 0));
    }

    #[test]
    fn slides_ease_in_and_out() {
        let t0 = Instant::now();
        let p = Popup { id: 1, appeared: t0, until: t0 + SHOW_FOR, leaving: None };
        assert_eq!(Feed::offset(&p, t0), 1.0);
        assert_eq!(Feed::offset(&p, t0 + SLIDE_IN), 0.0);
        let gone = Popup { leaving: Some(t0), ..p };
        assert_eq!(Feed::offset(&gone, t0 + SLIDE_OUT), 1.0);
    }
}
