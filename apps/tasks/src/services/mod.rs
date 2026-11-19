//! The Services page: Windows services, read only. The list (a click on a header sorts, the
//! header's search filters), the selected service's description under it, and a bar of
//! commands, each a box on the sheet. The services are read through the Service Control Manager off the UI thread, and
//! only while this page shows (see [`Page::tick`]).

mod list;
mod pill;

use crate::app::{card_box, State, MARGIN};
use egui::{pos2, Rect};
use shellman_core::services::logic::{self, SortBy};
use shellman_core::services::reader::Reader;
use shellman_core::services::Service;
use std::time::{Duration, Instant};

const PILL_H: f32 = 52.0;
/// The selected service's card: its name, how it starts, and its description.
const CARD_H: f32 = 98.0;
/// Services change slowly: read again at most this often, even when the numbers update faster.
const SLOWEST: Duration = Duration::from_secs(2);

#[derive(Default)]
pub struct Page {
    reader: Reader,
    all: Vec<Service>,
    /// Indexes into `all` of what shows, in order.
    rows: Vec<usize>,
    sort: SortBy,
    /// The selected service's name.
    selected: Option<String>,
    loaded: bool,
    last_read: Option<Instant>,
    /// What `rows` was made from.
    shown_query: String,
    shown_sort: Option<SortBy>,
    stale: bool,
    /// Scroll the selection into view on the next frame.
    reveal: bool,
    /// A row has the keyboard this frame / had it last frame (the selected row takes it over).
    row_focus: bool,
    take_focus: bool,
    /// The arrows moved the selection while a row had the keyboard; the new row takes it once drawn.
    focus_pending: bool,
}

impl Page {
    /// Each frame while the page shows: take a finished reading, start the next one when due
    /// (never while minimised), and ask for a frame only when the next one is due. Nothing runs
    /// while another page shows, because nothing calls this.
    pub fn tick(&mut self, ctx: &egui::Context, query: &str, interval: Duration, minimised: bool) {
        if let Some(all) = self.reader.take() {
            self.all = all;
            self.loaded = true;
            self.stale = true;
        }
        if self.stale || self.shown_query != query || self.shown_sort != Some(self.sort) {
            self.rows = logic::shown(&self.all, query, self.sort);
            query.clone_into(&mut self.shown_query);
            self.shown_sort = Some(self.sort);
            self.stale = false;
        }
        if minimised {
            return;
        }
        let every = interval.max(SLOWEST);
        let due = self.last_read.is_none_or(|t| t.elapsed() >= every);
        if due {
            let wake = ctx.clone();
            if self.reader.request(move || wake.request_repaint()) {
                self.last_read = Some(Instant::now());
            }
        }
        let wait = self.last_read.map_or(Duration::from_millis(100), |t| every.saturating_sub(t.elapsed()));
        ctx.request_repaint_after(wait.max(Duration::from_millis(50)));
    }

    /// The selected service, if it is still listed.
    fn current(&self) -> Option<&Service> {
        let name = self.selected.as_deref()?;
        self.all.iter().find(|s| s.name == name)
    }
}

pub fn show(ui: &mut egui::Ui, area: Rect, st: &mut State) {
    let pill = Rect::from_min_max(pos2(area.left(), area.bottom() - PILL_H), area.max);
    let mut bottom = pill.top() - MARGIN;
    let card = st.services.current().is_some().then(|| {
        let r = Rect::from_min_max(pos2(area.left(), bottom - CARD_H), pos2(area.right(), bottom));
        bottom = r.top() - MARGIN;
        r
    });
    let list = Rect::from_min_max(area.min, pos2(area.right(), bottom));
    if list.height() > 60.0 {
        card_box(ui, list);
        list::show(ui, list, st);
    }
    if let Some(card) = card.filter(|c| c.top() > area.top()) {
        card_box(ui, card);
        list::card(ui, card, st);
    }
    card_box(ui, pill);
    pill::show(ui, pill, st);
}
