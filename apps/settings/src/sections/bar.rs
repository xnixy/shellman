//! Top bar: how the clock reads.

use crate::page;
use crate::state::State;
use shellman_core::settings::bar::HourCycle;
use shellman_core::system::clock;
use shellman_ui::theme::{self, colors};
use shellman_ui::widgets::switch;

pub fn show(ui: &mut egui::Ui, st: &mut State) {
    page::heading(ui, "Top bar", "The clock in the corner of the top bar.");

    page::label(ui, "Hours");
    let options = HourCycle::ALL.map(|h| (h, h.label()));
    let mut changed = page::segmented(ui, &mut st.bar.clock.hour_cycle, &options);

    page::gap(ui);
    changed |= switch(ui, &mut st.bar.clock.show_date, "Show the date").changed();

    page::gap(ui);
    page::label(ui, "It will read");
    // The sample moves on with the minute, like the bar's own clock.
    ui.ctx().request_repaint_after(clock::until_next_minute());
    ui.label(
        egui::RichText::new(clock::text(&st.bar.clock))
            .size(15.0)
            .color(colors().text),
    );
    ui.label(theme::muted("The bar picks this up as soon as you change it.").size(12.0));

    if changed {
        st.save_bar();
    }
}
