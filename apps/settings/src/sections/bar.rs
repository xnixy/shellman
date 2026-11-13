//! Top bar: how the clock reads, the "now" pill, where notifications pop up, and the calendar's
//! week numbers.

use crate::page;
use crate::state::State;
use shellman_core::settings::bar::{HourCycle, ToastScreen, TrayMonitors, WeekNumbers};
use shellman_core::system::clock;
use shellman_ui::theme::{self, colors};
use shellman_ui::widgets::switch;

pub fn show(ui: &mut egui::Ui, st: &mut State) {
    page::heading(ui, "Top bar", "The clock, the \"now\" pill, notifications and the tray icons along the top of the screen.");

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
                ui.add_space(8.0);
                ui.add_enabled_ui(st.bar.clock.show_date, |ui| {
                    *changed |= switch(ui, &mut st.bar.clock.show_weekday, "Show the weekday").changed();
                });
                ui.add_space(8.0);
                *changed |= switch(ui, &mut st.bar.clock.show_seconds, "Show seconds").changed();
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
        // The sample moves on with the minute (or the second), like the bar's own clock.
        let next = if st.bar.clock.show_seconds { clock::until_next_second() } else { clock::until_next_minute() };
        ui.ctx().request_repaint_after(next);
        ui.label(egui::RichText::new(clock::text(&st.bar.clock)).size(15.0).color(colors().text));
        ui.label(theme::muted("The bar picks this up as soon as you change it.").size(12.0));
    });

    page::pair(
        ui,
        &mut changed,
        (
            "\"Now\" pill",
            &mut |ui, changed| {
                *changed |= switch(ui, &mut st.bar.now.on, "Show the pill beside the clock").changed();
                ui.add_space(8.0);
                ui.add_enabled_ui(st.bar.now.on, |ui| {
                    *changed |= switch(ui, &mut st.bar.now.events, "Events starting soon").changed();
                    ui.add_space(8.0);
                    *changed |= switch(ui, &mut st.bar.now.media, "What is playing").changed();
                });
                ui.add_space(8.0);
                ui.label(theme::muted("Events need a calendar: Settings › Desktop › Calendar.").size(12.0));
            },
        ),
        (
            "Notifications",
            &mut |ui, changed| {
                page::label(ui, "Show notifications on");
                let options = ToastScreen::ALL.map(|t| (t, t.label()));
                *changed |= page::segmented(ui, &mut st.bar.toasts_on, &options);
                page::gap(ui);
                page::label(ui, "Week numbers in the calendar");
                let options = WeekNumbers::ALL.map(|w| (w, w.label()));
                *changed |= page::segmented(ui, &mut st.bar.week_numbers, &options);
            },
        ),
    );

    if changed {
        st.save_bar();
    }
}
