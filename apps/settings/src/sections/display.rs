//! Display: the arrangement of the monitors, and for the chosen one its resolution, refresh rate,
//! orientation, scale, HDR and brightness. Changes are staged here and applied with a button;
//! a mode, orientation or arrangement change then asks "Keep these display settings?" and goes
//! back by itself if nobody answers. All system work is done by a worker (`DisplayPage`).

use super::display_map::{self, Drag};
use super::sound_rows::{level_slider, link_row};
use crate::page;
use crate::state::State;
use shellman_core::system::brightness::{BrightnessWatch, Screen};
use shellman_core::system::display::identify;
use shellman_core::system::display::modes::{self, Group};
use shellman_core::system::display::page::{self as model, Change, Monitor, Orientation, Page, Topology};
use shellman_core::system::display::{revert, DisplayPage};
use shellman_ui::theme;
use shellman_ui::widgets::switch;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The page's live state. The worker starts when the page is first drawn and ends when another
/// page is shown, so nothing runs for it otherwise.
#[derive(Default)]
pub struct DisplayView {
    handle: Option<Arc<DisplayPage>>,
    brightness: Option<BrightnessWatch>,
    screens: Vec<Screen>,
    page: Page,
    loaded: bool,
    selected: String,
    /// What the user has changed and not applied yet.
    staged: Vec<Change>,
    primary: String,
    scale: HashMap<String, u32>,
    hdr: HashMap<String, bool>,
    drag: Option<Drag>,
}

impl DisplayView {
    pub fn stop(&mut self) {
        if self.handle.take().is_some() {
            *self = Self::default();
        }
    }

    fn start(&mut self, ctx: &egui::Context) {
        if self.handle.is_none() {
            let c = ctx.clone();
            self.handle = Some(Arc::new(DisplayPage::start(Arc::new(move || c.request_repaint()))));
            let c = ctx.clone();
            self.brightness = Some(BrightnessWatch::start(Arc::new(move || c.request_repaint())));
        }
    }

    fn poll(&mut self) {
        if let Some(screens) = self.brightness.as_ref().and_then(BrightnessWatch::take) {
            self.screens = screens;
        }
        let Some(page) = self.handle.as_ref().and_then(|h| h.take()) else { return };
        // A change from outside (a plug, another tool) or our own apply: start from the truth.
        if !self.loaded || page.monitors != self.page.monitors {
            self.reset_to(&page.monitors);
        }
        self.page = page;
        self.loaded = true;
    }

    fn reset_to(&mut self, monitors: &[Monitor]) {
        self.staged = monitors.iter().map(Change::of).collect();
        self.primary = monitors.iter().find(|m| m.primary).map(|m| m.id.clone()).unwrap_or_default();
        self.scale.clear();
        self.hdr.clear();
        self.drag = None;
        if !monitors.iter().any(|m| m.id == self.selected) {
            self.selected = self.primary.clone();
        }
    }

    fn plan(&self) -> Vec<Change> {
        model::plan(&self.page.monitors, &self.staged, &self.primary)
    }

    /// Make the staged corners the planned (normalised) ones, so a new primary or a drag leaves
    /// every monitor's position consistent.
    fn settle_positions(&mut self) {
        let plan = self.plan();
        for (s, p) in self.staged.iter_mut().zip(&plan) {
            s.x = p.x;
            s.y = p.y;
        }
    }

    fn dirty(&self) -> bool {
        let plan = self.plan();
        self.mode_dirty(&plan) || self.page.monitors.iter().any(|m| self.scale_wanted(m).is_some() || self.hdr_wanted(m).is_some())
    }

    fn mode_dirty(&self, plan: &[Change]) -> bool {
        plan.iter().zip(&self.page.monitors).any(|(c, m)| c.differs_from(m))
    }

    fn scale_wanted(&self, m: &Monitor) -> Option<u32> {
        let want = *self.scale.get(&m.id)?;
        (m.scale.and_then(|s| s.current()) != Some(want)).then_some(want)
    }

    fn hdr_wanted(&self, m: &Monitor) -> Option<bool> {
        let want = *self.hdr.get(&m.id)?;
        (m.hdr.map(|h| h.enabled) != Some(want)).then_some(want)
    }
}

pub fn show(ui: &mut egui::Ui, st: &mut State) {
    page::heading(ui, "Display", "Arrange your screens and set each one's resolution, scale and brightness.");
    st.display.start(ui.ctx());
    st.display.poll();
    let v = &mut st.display;
    let Some(h) = v.handle.clone() else { return };
    if !v.loaded {
        ui.label(theme::muted("Reading the displays..."));
        return;
    }
    if v.page.monitors.is_empty() {
        ui.label(theme::muted("Windows reports no active display."));
        return;
    }
    if let Some(deadline) = v.page.pending {
        confirm(ui, &h, deadline);
        page::gap(ui);
    }
    let duplicated = v.page.topology == Some(Topology::Duplicate);
    arrangement(ui, v, duplicated);
    page::gap(ui);
    if let Some(m) = v.page.monitors.iter().find(|m| m.id == v.selected).cloned() {
        monitor_section(ui, v, &m);
        page::gap(ui);
    }
    apply_row(ui, v);
    page::gap(ui);
    links(ui);
}

/// "Keep these display settings?" with the seconds left. The worker reverts by itself at the end;
/// this only shows it.
fn confirm(ui: &mut egui::Ui, h: &DisplayPage, deadline: Instant) {
    let left = revert::seconds_left(deadline, Instant::now());
    page::label(ui, "Keep these display settings?");
    ui.label(theme::muted(format!("The previous settings come back in {left} s if you do nothing.")).size(12.5));
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if ui.button("Keep changes").clicked() {
            h.keep();
        }
        if ui.button("Revert").clicked() {
            h.revert();
        }
    });
    ui.ctx().request_repaint_after(Duration::from_millis(500));
}

fn arrangement(ui: &mut egui::Ui, v: &mut DisplayView, duplicated: bool) {
    page::label(ui, "Arrangement");
    let Some(h) = v.handle.clone() else { return };
    if duplicated {
        ui.label(theme::muted("The screens show the same picture, so there is nothing to arrange.").size(12.5));
    } else {
        let plan = v.plan();
        let sel = v.page.monitors.iter().position(|m| m.id == v.selected).unwrap_or(0);
        let out = display_map::show(ui, &plan, &v.page.monitors, sel, &v.primary, &mut v.drag);
        if let Some(i) = out.select {
            v.selected = v.page.monitors[i].id.clone();
        }
        if let Some((i, (x, y))) = out.moved {
            v.staged[i].x = x;
            v.staged[i].y = y;
            v.settle_positions();
        }
        ui.add_space(4.0);
        ui.label(theme::muted("Drag a screen to move it. Screens snap to each other's edges.").size(12.0));
    }
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if ui.button("Identify").on_hover_text("Show each screen's number for a few seconds").clicked() {
            identify::show(v.page.monitors.iter().map(|m| (m.rect, m.number)).collect());
        }
        if v.page.monitors.len() > 1 {
            ui.add_space(12.0);
            let mut layout = v.page.topology.unwrap_or(Topology::Other);
            let before = layout;
            ui.label(theme::muted("Multiple displays").size(12.0));
            let busy = v.page.pending.is_some();
            let opts = [(Topology::Extend, "Extend"), (Topology::Duplicate, "Duplicate")];
            if page::segmented(ui, &mut layout, &opts) && layout != before && !busy {
                h.set_topology(layout);
            }
        }
    });
}

fn monitor_section(ui: &mut egui::Ui, v: &mut DisplayView, m: &Monitor) {
    let i = v.page.monitors.iter().position(|x| x.id == m.id).unwrap_or(0);
    let main = v.primary == m.id;
    page::label(ui, &format!("{} \u{b7} {}{}", m.number, m.name.trim(), if main { " \u{b7} main display" } else { "" }));
    ui.add_space(4.0);
    let staged = v.staged[i].clone();
    resolution_and_rate(ui, m, &staged, &mut v.staged[i]);
    ui.add_space(10.0);
    page::label(ui, "Orientation");
    let mut o = v.staged[i].orientation;
    if egui::ComboBox::from_id_salt(("orient", &m.id))
        .width(220.0)
        .selected_text(o.label())
        .show_ui(ui, |ui| {
            for opt in Orientation::ALL {
                ui.selectable_value(&mut o, opt, opt.label());
            }
        })
        .response
        .changed()
        || o != v.staged[i].orientation
    {
        v.staged[i].orientation = o;
    }
    ui.add_space(10.0);
    if let Some(s) = m.scale {
        page::label(ui, "Scale");
        let now = v.scale.get(&m.id).copied().or(s.current()).unwrap_or(100);
        egui::ComboBox::from_id_salt(("scale", &m.id)).width(220.0).selected_text(format!("{now}%")).show_ui(ui, |ui| {
            for p in s.options() {
                let tag = if Some(p) == s.recommended() { " (recommended)" } else { "" };
                if ui.selectable_label(p == now, format!("{p}%{tag}")).clicked() {
                    v.scale.insert(m.id.clone(), p);
                }
            }
        });
        ui.add_space(10.0);
    }
    if let Some(hdr) = m.hdr {
        let mut on = v.hdr.get(&m.id).copied().unwrap_or(hdr.enabled);
        if switch(ui, &mut on, "HDR").changed() {
            v.hdr.insert(m.id.clone(), on);
        }
        ui.add_space(10.0);
    }
    if let Some(screen) = v.screens.iter().find(|s| s.id == m.id) {
        page::label(ui, "Brightness");
        let id = egui::Id::new(("display-brightness", &m.id));
        let level = ui.horizontal(|ui| level_slider(ui, id, screen.percent as f32 / 100.0, false, 0.0)).inner;
        if let (Some(level), Some(b)) = (level, &v.brightness) {
            b.set(&m.id, (level * 100.0).round() as u8);
        }
        ui.add_space(10.0);
    }
    if !main && ui.button("Make this my main display").clicked() {
        v.primary = m.id.clone();
        v.settle_positions();
    }
}

/// The resolution and refresh pickers; picking a resolution keeps the rate where it can.
fn resolution_and_rate(ui: &mut egui::Ui, m: &Monitor, staged: &Change, out: &mut Change) {
    page::label(ui, "Resolution");
    let group = |c: &Change| m.groups.iter().find(|g| g.w == c.w && g.h == c.h);
    let text = |g: &Group| format!("{}{}", g.label(), if g.recommended { " (recommended)" } else { "" });
    let shown = group(staged).map(text).unwrap_or_else(|| format!("{} \u{d7} {}", staged.w, staged.h));
    egui::ComboBox::from_id_salt(("res", &m.id)).width(300.0).selected_text(shown).show_ui(ui, |ui| {
        for g in &m.groups {
            if ui.selectable_label(g.w == staged.w && g.h == staged.h, text(g)).clicked() {
                out.w = g.w;
                out.h = g.h;
                out.hz = modes::rate_for(g, staged.hz);
            }
        }
    });
    ui.add_space(10.0);
    page::label(ui, "Refresh rate");
    let rates: Vec<u32> = group(out).map(|g| g.rates.clone()).unwrap_or_default();
    egui::ComboBox::from_id_salt(("hz", &m.id)).width(220.0).selected_text(format!("{} Hz", out.hz)).show_ui(ui, |ui| {
        for r in rates {
            if ui.selectable_label(r == out.hz, format!("{r} Hz")).clicked() {
                out.hz = r;
            }
        }
    });
}

fn apply_row(ui: &mut egui::Ui, v: &mut DisplayView) {
    let Some(h) = v.handle.clone() else { return };
    let plan = v.plan();
    let fits = model::plan_valid(&plan);
    let busy = v.page.pending.is_some();
    let dirty = v.dirty();
    ui.horizontal(|ui| {
        if ui.add_enabled(dirty && fits && !busy, egui::Button::new("Apply")).clicked() {
            for m in &v.page.monitors {
                if let Some(p) = v.scale_wanted(m) {
                    h.set_scale(&m.id, p);
                }
                if let Some(on) = v.hdr_wanted(m) {
                    h.set_hdr(&m.id, on);
                }
            }
            if v.mode_dirty(&plan) {
                h.apply(v.staged.clone(), &v.primary);
            } else {
                v.scale.clear();
                v.hdr.clear();
                h.refresh();
            }
        }
        if ui.add_enabled(dirty, egui::Button::new("Reset")).clicked() {
            let monitors = v.page.monitors.clone();
            v.reset_to(&monitors);
        }
    });
    if dirty && !fits {
        ui.add_space(4.0);
        ui.label(theme::muted("The screens must touch along an edge without overlapping.").size(12.0));
    }
    if let Some(n) = &v.page.notice {
        ui.add_space(4.0);
        ui.label(theme::muted(n).size(12.5));
    }
}

fn links(ui: &mut egui::Ui) {
    ui.spacing_mut().item_spacing.y = 4.0;
    if link_row(ui, "Colour management").clicked() {
        let _ = std::process::Command::new("colorcpl.exe").spawn();
    }
    if link_row(ui, "Adjust ClearType text").clicked() {
        let _ = std::process::Command::new("cttune.exe").spawn();
    }
    ui.add_space(6.0);
    ui.label(theme::muted("Night light is kept by Windows in a private format, so it cannot be set from here.").size(12.0));
}
