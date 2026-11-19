//! Where the window's parts go, from its size alone: pure geometry, tested without a window.
//!
//! ```text
//!  sidebar   | < >  [ Home / Projects / shellman ]  [ search ]  i  [=|#]  ^v  | - [] x
//!  Files     | [ tab ] [ tab ] +
//!  Places    | content: cards or rows straight on the pane       [ details ]
//!  ..        |                                                   [  box    ]
//!  ..        |--------------------------------------------------------------
//!  ..        | 3 selected  12 MB               Open  Copy  Cut  Rename  Delete
//! ```
//!
//! The window is one sheet (`shellman_ui::sheet::Sheet`): the sidebar, the header bar, the row of
//! tabs, the body and the bottom bar tile it. The folder, the tabs and the header's tools all
//! start `INSET` in from the sidebar, so they line up.

use egui::{pos2, vec2, Rect};
use shellman_ui::chips::CAPTIONS_W;
use shellman_ui::sheet::HEADER_H;
use shellman_ui::tab_strip;

/// From the pane's edges to what is in it: the header's tools, the tabs, the folder, the details.
pub(super) const INSET: f32 = 12.0;
/// The sidebar's least width when it shows names (as Settings').
pub(super) const SIDEBAR_MIN: f32 = 240.0;
/// The row of tabs under the header.
pub(super) const TABS_H: f32 = 40.0;
/// The bar along the bottom: the selection and what can be done to it (or the picker's fields).
pub(super) const BAND_H: f32 = 44.0;
/// From the tabs to the folder's first line, and to the details box.
const FOLDER_TOP: f32 = 4.0;
/// The details box, when it shows (the design's 336 of 1600).
pub(super) const DETAILS_W: f32 = 300.0;
/// The folder's least room beside the details box.
const DETAILS_MIN_CONTENT: f32 = 400.0;
/// The UI language's narrow window: below this much room right of the sidebar, one pane at a time.
pub(super) const ONE_PANE_BELOW: f32 = 620.0;

/// The header's round buttons, and its fields' (the path, the search, the view switch) height.
pub(super) const BUTTON: f32 = 34.0;
pub(super) const FIELD_H: f32 = 32.0;
/// Between the header's tools and the window buttons.
const CAPTION_GAP: f32 = 8.0;
/// After back and forward; between the path and the search; before the details switch; between
/// the view switch and the sort button.
const NAV_GAP: f32 = 8.0;
const PATH_GAP: f32 = 12.0;
const SEARCH_GAP: f32 = 10.0;
const SMALL_GAP: f32 = 6.0;
/// The search field's widths: as wide as it likes, and the least it gives way to.
const SEARCH_W: f32 = 230.0;
const SEARCH_MIN: f32 = 90.0;
/// The path's least room before the view switch and the sort button make way for it.
const PATH_MIN: f32 = 80.0;

/// The parts of one frame's window.
#[derive(Debug, Clone, Copy)]
pub(super) struct Plan {
    /// The bar across the top of the pane, and the window buttons flush in its corner.
    pub(super) header: Rect,
    pub(super) captions: Rect,
    /// The header's room for its tools: from `INSET` in to short of the window buttons.
    pub(super) tools: Rect,
    /// The row under the header, across the pane.
    pub(super) tabs: Rect,
    /// Where the tab strip lies in it: its tabs start where the tools and the folder do (the
    /// strip has its own padding around them).
    pub(super) tabs_row: Rect,
    /// The bar along the bottom, a hairline above it.
    pub(super) band: Rect,
    /// The folder's part of the body: the ground the rubber band starts on and drops land in.
    pub(super) content: Rect,
    /// Where the folder's items are drawn, inside `content`.
    pub(super) folder: Rect,
    /// The details box at the right of the body, when it shows; in place of the folder when
    /// `alone`.
    pub(super) details: Option<Rect>,
    /// Too narrow for the folder and the details side by side: one pane at a time (never two
    /// squeezed ones), the details with a back arrow to the folder.
    pub(super) one_pane: bool,
    /// The details fill the folder's place.
    pub(super) alone: bool,
}

impl Plan {
    /// The plan for a window of `screen`, with the sidebar `sidebar_w` wide. `details`: the
    /// details are on; `alone`: in a narrow window, they show instead of the folder.
    pub(super) fn new(screen: Rect, details: bool, alone: bool, sidebar_w: f32) -> Self {
        let (top, right, bottom) = (screen.top(), screen.right(), screen.bottom());
        // As `Sheet::new` splits the window.
        let split = (screen.left() + sidebar_w).min(right);
        let header = Rect::from_min_max(pos2(split, top), pos2(right, top + HEADER_H));
        let captions = Rect::from_min_max(pos2((right - CAPTIONS_W).max(split), top), header.right_bottom());
        let tools_left = split + INSET;
        let tools = Rect::from_min_max(pos2(tools_left, top), pos2((captions.left() - CAPTION_GAP).max(tools_left), header.bottom()));
        let tabs = Rect::from_min_max(header.left_bottom(), pos2(right, header.bottom() + TABS_H));
        let tabs_row = Rect::from_min_max(
            pos2(tools_left - tab_strip::PAD, tabs.top()),
            pos2((right - INSET + tab_strip::PAD).max(tools_left), tabs.bottom()),
        );
        let band = Rect::from_min_max(pos2(split, (bottom - BAND_H).max(tabs.bottom())), pos2(right, bottom));
        // Between the tabs and the bottom bar.
        let body = Rect::from_min_max(tabs.left_bottom(), pos2(right, band.top()));

        let one_pane = right - split < ONE_PANE_BELOW.max(DETAILS_W + DETAILS_MIN_CONTENT + 3.0 * INSET);
        let alone = details && alone && one_pane;
        let box_top = body.top() + FOLDER_TOP;
        let box_bottom = (body.bottom() - INSET).max(box_top);
        let details = if alone {
            Some(Rect::from_min_max(pos2(tools_left, box_top), pos2((right - INSET).max(tools_left), box_bottom)))
        } else {
            (details && !one_pane).then(|| Rect::from_min_max(pos2(right - INSET - DETAILS_W, box_top), pos2(right - INSET, box_bottom)))
        };
        let content_right = details.filter(|_| !alone).map_or(right, |d| d.left());
        let content = Rect::from_min_max(body.min, pos2(content_right, body.bottom()));
        let folder = Rect::from_min_max(
            pos2(tools_left, (content.top() + FOLDER_TOP).min(content.bottom())),
            pos2((content.right() - INSET).max(tools_left), content.bottom()),
        );
        Self { header, captions, tools, tabs, tabs_row, band, content, folder, details, one_pane, alone }
    }
}

/// The header's tools, left to right: back and forward, the path, the search, the details
/// switch, the view switch (or the Recycle Bin's buttons) and the sort button.
#[derive(Debug, Clone, Copy)]
pub(super) struct Tools {
    pub(super) back: Rect,
    pub(super) forward: Rect,
    pub(super) path: Rect,
    pub(super) search: Rect,
    pub(super) info: Rect,
    /// None when the header is too narrow for it (the bottom bar and Ctrl+1/2 still switch).
    pub(super) view: Option<Rect>,
    /// None when the header is too narrow for it (the bottom bar and the column titles still sort).
    pub(super) sort: Option<Rect>,
}

/// Lay the tools out in `bar` (`Plan::tools`), the view switch wanting `view_w` and able to make
/// do with `view_least` (the bin's buttons without their labels). When the path would get less
/// than its least room, what gives way, in turn: the search down to its least, the view switch
/// to its least, the sort button, the view switch. The search never goes below its least while
/// there is room for it.
pub(super) fn tools(bar: Rect, view_w: f32, view_least: f32) -> Tools {
    let y = bar.center().y;
    let square = |left: f32| Rect::from_min_size(pos2(left, y - BUTTON / 2.0), vec2(BUTTON, BUTTON));
    let field = |l: f32, r: f32| Rect::from_min_max(pos2(l, y - FIELD_H / 2.0), pos2(r.max(l), y + FIELD_H / 2.0));
    let back = square(bar.left());
    let forward = square(back.right());
    let path_left = forward.right() + NAV_GAP;
    // The right-hand part's width, for a view switch `view` wide (if shown) and the sort button.
    let right_part = |view: Option<f32>, sort: bool| {
        BUTTON + SEARCH_GAP + view.map_or(0.0, |v| v + NAV_GAP) + if sort { BUTTON + SMALL_GAP } else { 0.0 }
    };
    let tries = [(Some(view_w), true), (Some(view_least), true), (Some(view_least), false), (None, false)];
    let (view_w, sort_on) = tries
        .into_iter()
        .find(|&(v, s)| bar.right() - right_part(v, s) - path_left >= SEARCH_MIN + PATH_GAP + PATH_MIN)
        .unwrap_or((None, false));
    let mut x = bar.right();
    let sort = sort_on.then(|| {
        let r = square(x - BUTTON);
        x = r.left() - SMALL_GAP;
        r
    });
    let view = view_w.map(|w| {
        let r = field(x - w, x);
        x = r.left() - NAV_GAP;
        r
    });
    let info = square(x - BUTTON);
    x = info.left() - SEARCH_GAP;
    let free = (x - path_left).max(0.0);
    let search_w = (free * 0.4)
        .clamp(SEARCH_MIN, SEARCH_W)
        .min((free - PATH_GAP - PATH_MIN).max(SEARCH_MIN))
        .min(free);
    let search = field(x - search_w, x);
    let path = field(path_left, search.left() - PATH_GAP);
    Tools { back, forward, path, search, info, view, sort }
}

/// A strip `wanted` wide at the left of `row`, no wider than the row.
pub(super) fn from_left(row: Rect, wanted: f32) -> Rect {
    Rect::from_min_size(row.min, vec2(wanted.min(row.width()), row.height()))
}

#[cfg(test)]
mod tests {
    use super::*;
    /// The smallest window, which must still fit every part apart.
    use crate::placement::MIN_SIZE;
    use shellman_ui::sheet::Sheet;
    /// The sidebar folded to icons (it is wider with its names; see `shellman_ui::rail::width`).
    const FOLDED: f32 = shellman_ui::rail::FOLDED_W;
    /// Wider than the widest the rail's names may make it.
    const WIDEST: f32 = 270.0;

    impl Plan {
        /// The left column, the window's whole height: its head, then the rail. (The tests'
        /// windows start at 0, 0.)
        fn sidebar(&self) -> Rect {
            Rect::from_min_max(pos2(0.0, self.header.top()), pos2(self.header.left(), self.band.bottom()))
        }

        /// Between the tabs and the bottom bar.
        fn body(&self) -> Rect {
            Rect::from_min_max(self.tabs.left_bottom(), self.band.right_top())
        }
    }

    fn screen(w: f32, h: f32) -> Rect {
        Rect::from_min_size(pos2(0.0, 0.0), vec2(w, h))
    }

    fn apart(a: Rect, b: Rect) -> bool {
        a.right() <= b.left() || b.right() <= a.left() || a.bottom() <= b.top() || b.bottom() <= a.top()
    }

    /// The window sizes to try: the smallest (always folded), just wide enough to show names
    /// with the widest sidebar, and two ordinary ones.
    fn sizes() -> [(f32, f32, f32); 4] {
        [
            (MIN_SIZE.0, MIN_SIZE.1, FOLDED),
            (shellman_ui::rail::NARROW_BELOW, MIN_SIZE.1, WIDEST),
            (1100.0, 700.0, SIDEBAR_MIN),
            (2560.0, 1400.0, WIDEST),
        ]
    }

    #[test]
    fn the_parts_tile_the_window_as_the_sheet_splits_it() {
        let ctx = egui::Context::default();
        for (w, h, side) in sizes() {
            let s = screen(w, h);
            let p = Plan::new(s, false, false, side);
            let all = [p.sidebar(), p.header, p.tabs, p.body(), p.band];
            let mut area = 0.0;
            for (i, a) in all.iter().enumerate() {
                assert!(a.width() > 0.0 && a.height() > 0.0, "{w}x{h}: part {i} is empty");
                assert!(s.contains_rect(*a), "{w}x{h}: part {i} is off the window");
                for b in &all[i + 1..] {
                    assert!(apart(*a, *b), "{w}x{h}: {a:?} and {b:?} overlap");
                }
                area += a.area();
            }
            assert!((area - s.area()).abs() < 0.5, "{w}x{h}: the parts leave a hole");
            // The same split as the sheet's, which paints the panes and the line between them.
            let sheet = Sheet::new(&ctx, s, side);
            assert_eq!(p.sidebar(), sheet.sidebar);
            assert_eq!(p.header, sheet.header);
            assert!(sheet.body.contains_rect(p.tabs) && sheet.body.contains_rect(p.body()) && sheet.body.contains_rect(p.band));
            // The window buttons in the header's corner, the tools short of them.
            assert_eq!(p.captions.right_top(), s.right_top());
            assert!(p.header.contains_rect(p.captions) && p.header.contains_rect(p.tools));
            assert!(apart(p.tools, p.captions));
            // Room for a few tabs, and for the folder.
            assert!(p.tabs_row.width() >= 250.0, "{w}x{h}");
            assert!(p.folder.height() >= 200.0, "{w}x{h}");
            assert!(p.body().contains_rect(p.content) && p.content.contains_rect(p.folder));
        }
    }

    #[test]
    fn content_lines_up_with_the_header_and_the_tabs() {
        for side in [FOLDED, SIDEBAR_MIN] {
            let p = Plan::new(screen(1100.0, 700.0), false, false, side);
            // The folder's items, the first tab and the first tool start the same way in.
            assert_eq!(p.folder.left(), p.tools.left());
            assert_eq!(p.folder.left(), p.tabs_row.left() + tab_strip::PAD);
            assert_eq!(p.folder.left() - p.sidebar().right(), INSET);
            // And end the same way in from the window's edge.
            assert_eq!(p.folder.right(), p.tabs_row.right() - tab_strip::PAD);
            assert_eq!(p.header.right() - p.folder.right(), INSET);
            // The folder's ground, the tabs and the bottom bar span the pane under the header.
            for r in [p.content, p.tabs, p.band, p.body()] {
                assert_eq!((r.left(), r.right()), (p.header.left(), p.header.right()));
            }
            assert_eq!(p.content.top(), p.tabs.bottom());
            assert_eq!(p.content.bottom(), p.band.top());
            assert_eq!(p.band.bottom(), p.sidebar().bottom());
        }
    }

    #[test]
    fn the_details_box_takes_the_right_and_never_overlaps() {
        let p = Plan::new(screen(1400.0, 800.0), true, false, SIDEBAR_MIN);
        let d = p.details.expect("room for it");
        assert!(!p.one_pane && !p.alone);
        assert_eq!(d.width(), DETAILS_W);
        assert_eq!(p.header.right() - d.right(), INSET);
        assert_eq!(d.top(), p.folder.top());
        assert_eq!(d.left() - p.folder.right(), INSET);
        assert_eq!(p.content.right(), d.left());
        assert!(p.body().contains_rect(d));
        assert!(apart(d, p.band) && apart(d, p.content) && apart(d, p.tabs));
        // Too narrow: the content keeps its room.
        let (w, _) = MIN_SIZE;
        assert!(Plan::new(screen(w, 600.0), true, false, FOLDED).details.is_none());
        assert!(Plan::new(screen(1400.0, 800.0), false, false, FOLDED).details.is_none());
        // Asked for alone in a wide window, they still go beside the folder.
        assert!(!Plan::new(screen(1400.0, 800.0), true, true, FOLDED).alone);
    }

    #[test]
    fn a_narrow_window_shows_one_pane_at_a_time() {
        // 620 of room right of the sidebar (the text size makes a window this narrow in points).
        let w = FOLDED + 620.0;
        let list = Plan::new(screen(w, 700.0), true, false, FOLDED);
        assert!(list.one_pane && !list.alone && list.details.is_none());
        let details = Plan::new(screen(w, 700.0), true, true, FOLDED);
        assert!(details.alone);
        // The details take the folder's whole place, as wide as it would be.
        let d = details.details.unwrap();
        assert_eq!((d.left(), d.right()), (details.folder.left(), details.folder.right()));
        assert_eq!(d.top(), details.folder.top());
        // Never two panes squeezed: whenever both show, the folder keeps its room.
        for w in (500..2000).step_by(10) {
            let p = Plan::new(screen(w as f32, 700.0), true, false, FOLDED);
            if let Some(d) = p.details {
                assert!(p.folder.width() >= DETAILS_MIN_CONTENT && d.left() > p.folder.right(), "{w}");
                assert!(p.body().width() >= ONE_PANE_BELOW, "{w}");
            }
        }
    }

    #[test]
    fn the_header_tools_fit_short_of_the_window_buttons() {
        let bin_w = super::super::bin::BUTTONS_W;
        let bin_least = super::super::bin::BUTTONS_LEAST_W;
        for (w, h, side) in sizes() {
            let p = Plan::new(screen(w, h), false, false, side);
            for (view_w, least) in [(76.0, 76.0), (bin_w, bin_least)] {
                let t = tools(p.tools, view_w, least);
                let mut all = vec![t.back, t.forward, t.path, t.search, t.info];
                all.extend(t.view);
                all.extend(t.sort);
                for (i, a) in all.iter().enumerate() {
                    assert!(a.width() >= 0.0, "{w}: tool {i} has no room");
                    assert!(p.tools.contains_rect(*a), "{w}: tool {i} is out of the header's room");
                    for b in &all[i + 1..] {
                        assert!(apart(*a, *b), "{w}: {a:?} and {b:?} overlap");
                    }
                }
                // Left to right, and the path and the search keep their least room.
                assert!(t.forward.right() < t.path.left() && t.path.right() < t.search.left());
                assert!(t.search.right() < t.info.left());
                assert!(t.path.width() >= PATH_MIN, "{w}: the path has {}", t.path.width());
                assert!(t.search.width() >= SEARCH_MIN, "{w}");
            }
        }
        // The view switch stays wherever the sidebar shows names; the sort button makes way only
        // in the narrowest of those windows.
        for (w, side, sort) in [(800.0, WIDEST, false), (824.0, SIDEBAR_MIN, true), (MIN_SIZE.0, FOLDED, true)] {
            let p = Plan::new(screen(w, 600.0), false, false, side);
            let t = tools(p.tools, 76.0, 76.0);
            assert!(t.view.is_some() || side == WIDEST, "{w}");
            assert_eq!(t.sort.is_some(), sort, "{w}");
        }
        // A roomy window shows everything, the bin's buttons with their labels.
        let p = Plan::new(screen(1600.0, 900.0), false, false, SIDEBAR_MIN);
        let t = tools(p.tools, bin_w, bin_least);
        assert_eq!(t.view.map(|v| v.width()), Some(bin_w));
        assert!(t.sort.is_some());
        assert_eq!(t.sort.unwrap().right(), p.tools.right());
        assert_eq!(t.search.width(), SEARCH_W);
    }

    #[test]
    fn a_strip_fits_its_row() {
        let row = Rect::from_min_size(pos2(80.0, 12.0), vec2(400.0, 40.0));
        assert_eq!(from_left(row, 120.0).width(), 120.0);
        assert_eq!(from_left(row, 900.0), row);
    }
}
