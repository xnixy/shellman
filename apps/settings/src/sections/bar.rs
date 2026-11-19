//! Top bar: how the clock reads, the "now" pill, where notifications pop up, and the calendar's
//! week numbers.

use crate::page;
use crate::state::State;
use shellman_core::settings::bar::{HourCycle, ToastScreen, TrayMonitors, WeekNumbers};
use shellman_core::system::clock;
use shellman_ui::theme::{self, colors};
use shellman_ui::widgets::toggle;

pub fn show(ui: &mut egui::Ui, st: &mut State) {
    page::heading(ui, "Top bar", "The clock, the \"now\" pill, notifications and the tray icons along the top of the screen.");

    let mut changed = false;
    page::group(ui, "Clock", |ui| {
        let options = HourCycle::ALL.map(|h| (h, h.label()));
        changed |= page::row(ui, "Hours", "", |ui| page::segmented(ui, &mut st.bar.clock.hour_cycle, &options));
        let title = "Show the date";
        changed |= page::row(ui, title, "", |ui| toggle(ui, &mut st.bar.clock.show_date, title)).changed();
        let title = "Show the weekday";
        let on = st.bar.clock.show_date;
        ui.add_enabled_ui(on, |ui| {
            changed |= page::row(ui, title, "", |ui| toggle(ui, &mut st.bar.clock.show_weekday, title)).changed();
        });
        let title = "Show seconds";
        changed |= page::row(ui, title, "", |ui| toggle(ui, &mut st.bar.clock.show_seconds, title)).changed();
    });

    page::group(ui, "It will read", |ui| {
        // The sample moves on with the minute (or the second), like the bar's own clock.
        let next = if st.bar.clock.show_seconds { clock::until_next_second() } else { clock::until_next_minute() };
        ui.ctx().request_repaint_after(next);
        page::row_with(ui, |ui| {
            ui.label(egui::RichText::new(clock::text(&st.bar.clock)).size(15.0).color(colors().text));
            ui.label(theme::muted("The bar picks this up as soon as you change it.").size(12.5));
        });
    });

    page::group(ui, "Tray icons", |ui| {
        let options = TrayMonitors::ALL.map(|t| (t, t.label()));
        changed |= page::row(ui, "Show tray icons on", "", |ui| page::segmented(ui, &mut st.bar.tray, &options));
    });

    page::group(ui, "\"Now\" pill", |ui| {
        let title = "Show the pill beside the clock";
        changed |= page::row(ui, title, "", |ui| toggle(ui, &mut st.bar.now.on, title)).changed();
        let on = st.bar.now.on;
        ui.add_enabled_ui(on, |ui| {
            let title = "Events starting soon";
            changed |= page::row(ui, title, "Events need a calendar: Settings › Desktop › Calendar.", |ui| toggle(ui, &mut st.bar.now.events, title)).changed();
            let title = "What is playing";
            changed |= page::row(ui, title, "", |ui| toggle(ui, &mut st.bar.now.media, title)).changed();
        });
    });

    page::group(ui, "Notifications", |ui| {
        let options = ToastScreen::ALL.map(|t| (t, t.label()));
        changed |= page::row(ui, "Show notifications on", "", |ui| page::segmented(ui, &mut st.bar.toasts_on, &options));
    });

    page::group(ui, "Calendar", |ui| {
        let options = WeekNumbers::ALL.map(|w| (w, w.label()));
        changed |= page::row(ui, "Week numbers in the calendar", "", |ui| page::segmented(ui, &mut st.bar.week_numbers, &options));
    });

    if changed {
        st.save_bar();
    }
}
