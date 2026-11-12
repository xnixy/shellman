//! Sound: the outputs and microphones with the default marked, the chosen device's volume, mute,
//! format and (for outputs) spatial sound, the input meter, the volume mixer, and a way into the
//! classic Sound applet. All audio work is done by a worker (`SoundPage`); the page only draws
//! what it last published and sends it what was asked.

use super::sound_mixer;
use super::sound_rows::{device_row, link_row, meter_bar, volume_row};
use crate::page;
use crate::state::State;
use shellman_core::system::sound_dev::{self, Device, Flow};
use shellman_core::system::sound_page::{Detail, Page, Role, Section, SoundPage};
use shellman_ui::theme;
use shellman_ui::widgets::switch;

/// The page's live state. The worker starts when the page is first drawn and ends when another
/// page is shown, so nothing runs for it otherwise.
#[derive(Default)]
pub struct SoundView {
    handle: Option<SoundPage>,
    page: Page,
    loaded: bool,
    /// The microphone the meter was asked to run on.
    meter: Option<String>,
}

impl SoundView {
    pub fn stop(&mut self) {
        if self.handle.take().is_some() {
            *self = Self::default();
        }
    }

    fn start(&mut self, ctx: &egui::Context) {
        if self.handle.is_none() {
            let ctx = ctx.clone();
            self.handle = Some(SoundPage::start(std::sync::Arc::new(move || ctx.request_repaint())));
        }
    }

    fn poll(&mut self) {
        if let Some(page) = self.handle.as_ref().and_then(SoundPage::take) {
            self.page = page;
            self.loaded = true;
        }
    }
}

pub fn show(ui: &mut egui::Ui, st: &mut State) {
    page::heading(ui, "Sound", "Your speakers and microphones, and the volume of each app. Changes apply at once.");
    st.sound.start(ui.ctx());
    st.sound.poll();
    let SoundView { handle, page, loaded, meter } = &mut st.sound;
    let Some(handle) = handle.as_ref() else { return };
    if !*loaded {
        // The page is open at once; the worker fills it in as soon as Windows has answered.
        ui.label(theme::muted("Reading the audio devices..."));
        return;
    }
    // The meter runs only while the microphone is on screen and the window is in use.
    let watching = ui.ctx().input(|i| i.focused);
    device_section(ui, handle, Flow::Output, &page.output, None, false);
    page::gap(ui);
    device_section(ui, handle, Flow::Input, &page.input, Some(&mut *meter), watching);
    page::gap(ui);
    sound_mixer::show(ui, handle, &page.sessions, &mut st.thumbs);
    page::gap(ui);
    if link_row(ui, "More sound settings").clicked() {
        sound_dev::open_classic(0);
    }
}

/// `meter` is the microphone section's alone: the output section has none, and two sections
/// both steering it would start and stop it every frame.
fn device_section(ui: &mut egui::Ui, h: &SoundPage, flow: Flow, section: &Section, meter: Option<&mut Option<String>>, watching: bool) {
    let output = flow == Flow::Output;
    page::label(ui, if output { "Output" } else { "Input" });
    if section.devices.is_empty() {
        let none = if output { "No output device is connected." } else { "No microphone is connected." };
        ui.label(theme::muted(none));
        return;
    }
    ui.spacing_mut().item_spacing.y = 4.0;
    for d in &section.devices {
        if device_row(ui, d, section.selected.as_deref() == Some(d.id.as_str())).clicked() {
            h.select(flow, &d.id);
        }
    }
    let Some(dev) = section.selected_device() else { return };
    ui.add_space(10.0);
    controls(ui, h, flow, section, dev);
    if let Some(detail) = &section.detail {
        details(ui, h, flow, dev, detail);
    }
    let Some(meter) = meter else { return };
    // The meter follows the chosen microphone, and stops with it.
    let want = watching.then(|| dev.id.clone());
    if *meter != want {
        h.meter(want.as_deref());
        *meter = want;
    }
    if meter.is_some() {
        page::gap(ui);
        page::label(ui, "Input level");
        meter_bar(ui, h.peak());
    }
}

/// Volume and mute, and which role the device plays.
fn controls(ui: &mut egui::Ui, h: &SoundPage, flow: Flow, section: &Section, dev: &Device) {
    page::label(ui, "Volume");
    let out = volume_row(ui, ("volume", &dev.id), dev.volume, dev.muted);
    if let Some(level) = out.set {
        h.set_volume(&dev.id, level);
    }
    if out.toggle_mute {
        h.set_muted(&dev.id, !dev.muted);
    }
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        if !dev.default && ui.button("Use as default").clicked() {
            h.set_default(&dev.id, flow, Role::Everything);
        }
        let mut calls = dev.default_comm;
        if switch(ui, &mut calls, "Also use for calls").changed() {
            // Turning it off hands calls back to the default device.
            let target = if calls { Some(&dev.id) } else { section.devices.iter().find(|d| d.default).map(|d| &d.id) };
            if let Some(id) = target {
                h.set_default(id, flow, Role::Calls);
            }
        }
    });
}

/// Balance and a test sound (outputs), the format, and spatial sound (outputs).
fn details(ui: &mut egui::Ui, h: &SoundPage, flow: Flow, dev: &Device, detail: &Detail) {
    let output = flow == Flow::Output;
    if output {
        ui.add_space(10.0);
        if ui.button("Play a test sound").on_hover_text("A short, quiet tone on this device").clicked() {
            h.play_tone(&dev.id);
        }
        if let Some(e) = &detail.tone_error {
            ui.label(theme::muted(e).size(12.0));
        }
        if let Some(mut b) = detail.balance {
            ui.add_space(10.0);
            page::label(ui, "Balance");
            ui.horizontal(|ui| {
                ui.label(theme::muted("Left").size(11.5));
                ui.spacing_mut().slider_width = 240.0;
                let resp = ui.add(egui::Slider::new(&mut b, -1.0..=1.0).show_value(false));
                ui.label(theme::muted("Right").size(11.5));
                if resp.changed() {
                    // A little play around the middle, so even is easy to land on.
                    h.set_balance(&dev.id, if b.abs() < 0.04 { 0.0 } else { b });
                }
            });
        }
    }
    format_picker(ui, h, dev, detail);
    if output {
        spatial_picker(ui, h, dev, detail);
    }
}

/// Spatial sound: Off plus what the device takes right now, the active one selected. The page
/// follows Windows' own change event, so a choice shows once Windows has taken it.
fn spatial_picker(ui: &mut egui::Ui, h: &SoundPage, dev: &Device, detail: &Detail) {
    ui.add_space(10.0);
    page::label(ui, "Spatial sound");
    let Some(sp) = detail.spatial.as_ref().filter(|s| s.supported) else {
        let why = if detail.spatial.is_some() {
            "This device does not offer spatial sound."
        } else {
            "Windows does not report this device's spatial sound."
        };
        ui.label(theme::muted(why).size(12.0));
        return;
    };
    let off = sp.is_off();
    egui::ComboBox::from_id_salt(("spatial", &dev.id)).width(300.0).selected_text(sp.active_name()).show_ui(ui, |ui| {
        if ui.selectable_label(off, "Off").clicked() && !off {
            h.set_spatial(&dev.id, "");
        }
        for (id, name) in &sp.formats {
            let on = id.eq_ignore_ascii_case(&sp.active);
            if ui.selectable_label(on, name).clicked() && !on {
                h.set_spatial(&dev.id, id);
            }
        }
    });
    if let Some(e) = &detail.spatial_error {
        ui.label(theme::muted(e).size(12.0));
    }
}

fn format_picker(ui: &mut egui::Ui, h: &SoundPage, dev: &Device, detail: &Detail) {
    ui.add_space(10.0);
    page::label(ui, "Format");
    let Some(now) = detail.format else {
        ui.label(theme::muted("Windows does not report this device's format."));
        return;
    };
    // Several layouts in the list (stereo, 5.1, 7.1) need the channels said.
    let layouts = detail.supported.iter().any(|f| f.channels != now.channels);
    let text = |f: &shellman_core::system::sound_format::FormatSpec| if layouts || f.encoded { f.full_label() } else { f.label() };
    if detail.supported.is_empty() {
        ui.label(theme::muted(format!("{} (this device offers no other formats)", text(&now))));
        return;
    }
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt(("format", &dev.id))
            .width(300.0)
            .selected_text(text(&now))
            .show_ui(ui, |ui| {
                for f in &detail.supported {
                    if ui.selectable_label(*f == now, text(f)).clicked() && *f != now {
                        h.set_format(&dev.id, *f);
                    }
                }
            });
        if detail.can_undo && ui.button("Undo").on_hover_text("Back to the format it had before").clicked() {
            h.undo_format(&dev.id);
        }
    });
}
