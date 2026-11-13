//! The command pill under the services list: go to the process, copy the name or PID, or a line
//! about the list when nothing is selected. Read only: no command here changes a service.

use crate::app::{Page as Tab, State};
use egui::{pos2, vec2, Align2, FontId, Rect};
use shellman_core::services::logic;
use shellman_ui::chips::{self, Chip};
use shellman_ui::theme::{colors, glyph};

pub fn show(ui: &mut egui::Ui, rect: Rect, st: &mut State) {
    let c = colors();
    let y = rect.center().y;
    let mut x = rect.left() + 18.0;
    let Some(s) = st.services.current().cloned() else {
        let all = &st.services.all;
        let text = if st.services.loaded {
            format!("{} services, {} running. Select one to read what it does.", all.len(), logic::running(all))
        } else {
            "Reading services...".to_owned()
        };
        ui.painter().text(pos2(x, y), Align2::LEFT_CENTER, text, FontId::proportional(13.0), c.muted);
        return;
    };
    let mut chip = |ui: &mut egui::Ui, label: &str, icon: Option<char>, enabled: bool, tip: &str| -> bool {
        let w = chips::chip_width(ui, label, icon.is_some(), Chip::Soft);
        let r = Rect::from_min_size(pos2(x, y - chips::CHIP_H / 2.0), vec2(w, chips::CHIP_H));
        x += w + 8.0;
        ui.scope(|ui| {
            if !enabled {
                ui.disable();
            }
            let resp = chips::chip(ui, r, egui::Id::new(("service-pill", label)), label, icon, Chip::Soft);
            let resp = if enabled { resp.on_hover_text(tip) } else { resp.on_disabled_hover_text(tip) };
            enabled && resp.clicked()
        })
        .inner
    };
    let running = s.pid != 0;
    let tip = if running { "Show its process in Processes" } else { "It is not running, so it has no process" };
    if chip(ui, "Go to process", Some(glyph::OPEN_EXTERNAL), running, tip) {
        go_to_process(st, s.pid);
    }
    if chip(ui, "Copy name", Some(glyph::COPY), true, "Copy the service name") {
        ui.ctx().copy_text(s.name.clone());
    }
    if chip(ui, "Copy PID", None, running, "Copy the process ID") {
        ui.ctx().copy_text(s.pid.to_string());
    }
    ui.painter().text(
        pos2(rect.right() - 18.0, y),
        Align2::RIGHT_CENTER,
        "Read only",
        FontId::proportional(12.0),
        c.muted,
    );
}

/// Switch to Processes with `pid` selected, if it is listed.
fn go_to_process(st: &mut State, pid: u32) {
    if let Some(key) = st.list.procs.iter().find(|p| p.key.pid == pid).map(|p| p.key) {
        st.list.select(Some(key));
        st.list.reveal = true;
    }
    st.clear_search = true;
    st.go_to = Some(Tab::Processes);
}
