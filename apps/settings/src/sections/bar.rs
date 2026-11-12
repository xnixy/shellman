//! Top bar: how the clock reads.

use crate::page;
use crate::state::State;
use shellman_core::settings::bar::{HourCycle, TrayMonitors};
use shellman_core::system::clock;
use shellman_ui::theme::{self, colors};
use shellman_ui::widgets::switch;

pub fn show(ui: &mut egui::Ui, st: &mut State) {
    page::heading(ui, "Top bar", "The clock and the tray icons in the corner of the top bar.");

    let mut changed = false;
    page::pair(
        ui,
        &mut changed,
        (
            "Clock",
            &mut |ui, changed| {
                page::label(ui, "Hours");
                let options = HourCycle::ALL.map(|h| (h, h.label()));
                *changed |= page::segmented(ui, &mut st.bar.clock.hour_cycle, &options);
                page::gap(ui);
                *changed |= switch(ui, &mut st.bar.clock.show_date, "Show the date").changed();
            },
        ),
        (
            "",
            &mut |ui, changed| {
                page::label(ui, "Show tray icons on");
                let options = TrayMonitors::ALL.map(|t| (t, t.label()));
                *changed |= page::segmented(ui, &mut st.bar.tray, &options);
            },
        ),
    );

    page::group(ui, "It will read", |ui| {
        // The sample moves on with the minute, like the bar's own clock.
        ui.ctx().request_repaint_after(clock::until_next_minute());
        ui.label(
            egui::RichText::new(clock::text(&st.bar.clock))
                .size(15.0)
                .color(colors().text),
        );
        ui.label(theme::muted("The bar picks this up as soon as you change it.").size(12.0));
    });

    if changed {
        st.save_bar();
    }
}
