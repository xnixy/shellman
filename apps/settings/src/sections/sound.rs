//! Sound: the outputs and microphones with the default marked, the chosen device's volume, mute,
//! format and (for outputs) spatial sound, the input meter, the volume mixer, and a way into the
//! classic Sound applet. All audio work is done by a worker (`SoundPage`); the page only draws
//! what it last published and sends it what was asked.

use shellman_ui::menu::dropdown::dropdown;
use super::sound_mixer;
use super::sound_rows::{device_row, disabled_row, link_row, meter_bar, volume_row};
use crate::page;
use crate::state::State;
use shellman_core::system::sound_choices::{self, Choices, FormatControl};
use shellman_core::system::sound_dev::{self, Device, Flow};
use shellman_core::system::sound_page::{Detail, Page, Role, Section, SoundPage};
use shellman_ui::theme;
use shellman_ui::widgets::toggle;

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
        page::group(ui, "", |ui| {
            page::row_with(ui, |ui| {
                ui.label(theme::muted("Reading the audio devices..."));
            });
        });
        return;
    }
    // The meter runs only while the microphone is on screen and the window is in use.
    let watching = ui.ctx().input(|i| i.focused);
    page::group(ui, "Output", |ui| device_section(ui, handle, Flow::Output, &page.output, None, false));
    page::group(ui, "Input", |ui| device_section(ui, handle, Flow::Input, &page.input, Some(&mut *meter), watching));
    page::group(ui, "Volume mixer", |ui| sound_mixer::show(ui, handle, &page.sessions, &mut st.thumbs));
    page::group(ui, "", |ui| {
        if let Some(why) = &page.notice {
            page::row_with(ui, |ui| {
                ui.add(egui::Label::new(theme::muted(why).size(12.5)).wrap());
            });
        }
        if link_row(ui, "More sound settings").clicked() {
            sound_dev::open_classic(0);
        }
    });
}

/// A line of muted text as a row of the group.
fn note(ui: &mut egui::Ui, text: &str) {
    page::row_with(ui, |ui| {
        ui.add(egui::Label::new(theme::muted(text).size(12.5)).wrap());
    });
}

/// `meter` is the microphone section's alone: the output section has none, and two sections
/// both steering it would start and stop it every frame.
fn device_section(ui: &mut egui::Ui, h: &SoundPage, flow: Flow, section: &Section, meter: Option<&mut Option<String>>, watching: bool) {
    if section.devices.is_empty() {
        note(ui, if flow == Flow::Output { "No output device is connected." } else { "No microphone is connected." });
        // Everything may be turned off, and then this is the way back.
        disabled_group(ui, h, &section.disabled);
        return;
    }
    for d in &section.devices {
        if device_row(ui, d, section.selected.as_deref() == Some(d.id.as_str())).clicked() {
            h.select(flow, &d.id);
        }
    }
    if let Some(dev) = section.selected_device() {
        controls(ui, h, flow, section, dev);
        if let Some(detail) = &section.detail {
            details(ui, h, flow, dev, detail);
        }
    }
    disabled_group(ui, h, &section.disabled);
    let (Some(meter), Some(dev)) = (meter, section.selected_device()) else { return };
    // The meter follows the chosen microphone, and stops with it.
    let want = watching.then(|| dev.id.clone());
    if *meter != want {
        h.meter(want.as_deref());
        *meter = want;
    }
    if meter.is_some() {
        page::stacked(ui, "Input level", "", |ui| meter_bar(ui, h.peak()));
    }
}

/// Devices that are turned off, set apart and muted. Pressing one turns it on (Windows may ask
/// for administrator rights first).
fn disabled_group(ui: &mut egui::Ui, h: &SoundPage, disabled: &[Device]) {
    if disabled.is_empty() {
        return;
    }
    page::row_with(ui, |ui| {
        ui.label(theme::muted("Disabled").size(12.5));
    });
    for d in disabled {
        if disabled_row(ui, d).clicked() {
            h.set_enabled(&d.id, true);
        }
    }
}

/// Volume and mute, and which role the device plays.
fn controls(ui: &mut egui::Ui, h: &SoundPage, flow: Flow, section: &Section, dev: &Device) {
    let out = page::stacked(ui, "Volume", "", |ui| volume_row(ui, ("volume", &dev.id), dev.volume, dev.muted));
    if let Some(level) = out.set {
        h.set_volume(&dev.id, level);
    }
    if out.toggle_mute {
        h.set_muted(&dev.id, !dev.muted);
    }
    if !dev.default && page::row(ui, "Use as default", "", |ui| ui.button("Use as default").clicked()) {
        h.set_default(&dev.id, flow, Role::Everything);
    }
    let mut calls = dev.default_comm;
    let title = "Also use for calls";
    if page::row(ui, title, "", |ui| toggle(ui, &mut calls, title)).changed() {
        // Turning it off hands calls back to the default device.
        let target = if calls { Some(&dev.id) } else { section.devices.iter().find(|d| d.default).map(|d| &d.id) };
        if let Some(id) = target {
            h.set_default(id, flow, Role::Calls);
        }
    }
    let off = page::row(ui, "Disable", "Turn this device off. It stays listed below, to turn on again.", |ui| ui.button("Disable").clicked());
    if off {
        h.set_enabled(&dev.id, false);
    }
}

/// Balance and a test sound (outputs), the format, and spatial sound (outputs).
fn details(ui: &mut egui::Ui, h: &SoundPage, flow: Flow, dev: &Device, detail: &Detail) {
    let output = flow == Flow::Output;
    if output {
        let why = detail.tone_error.as_deref().unwrap_or("A short, quiet tone on this device.");
        if page::row(ui, "Play a test sound", why, |ui| ui.button("Play").clicked()) {
            h.play_tone(&dev.id);
        }
        if let Some(mut b) = detail.balance {
            page::stacked(ui, "Balance", "", |ui| {
                ui.horizontal(|ui| {
                    ui.label(theme::muted("Left").size(12.5));
                    let width = (ui.available_width() - 56.0).clamp(120.0, 300.0);
                    let out = shellman_ui::slider::thin(ui, egui::Id::new(("balance", &dev.id)), (b + 1.0) / 2.0, Some(width), true).named("Balance");
                    ui.label(theme::muted("Right").size(12.5));
                    if let Some(v) = out.value {
                        b = v * 2.0 - 1.0;
                        // A little play around the middle, so even is easy to land on.
                        h.set_balance(&dev.id, if b.abs() < 0.04 { 0.0 } else { b });
                    }
                });
            });
        }
    }
    let choices = sound_choices::decide(detail.format.as_ref(), detail.spatial.as_ref());
    format_picker(ui, h, dev, detail, &choices);
    if output {
        spatial_picker(ui, h, dev, detail, &choices);
    }
}

/// Spatial sound: Off plus what the device takes right now, the active one selected. The page
/// follows Windows' own change event, so a choice shows once Windows has taken it.
fn spatial_picker(ui: &mut egui::Ui, h: &SoundPage, dev: &Device, detail: &Detail, choices: &Choices) {
    let Some(sp) = detail.spatial.as_ref().filter(|s| s.supported) else {
        let why = if detail.spatial.is_some() {
            "This device does not offer spatial sound."
        } else {
            "Windows does not report this device's spatial sound."
        };
        page::row(ui, "Spatial sound", why, |_| {});
        return;
    };
    let off = sp.is_off();
    let mut rows: Vec<(String, bool)> = vec![("Off".to_string(), off)];
    rows.extend(sp.formats.iter().map(|(id, name)| (name.clone(), id.eq_ignore_ascii_case(&sp.active))));
    let mut notes: Vec<&str> = Vec::new();
    notes.extend(choices.spatial_hint.as_deref());
    notes.extend(detail.spatial_error.as_deref());
    let picked = page::row(ui, "Spatial sound", &notes.join("\n"), |ui| dropdown(ui, ("spatial", &dev.id), 260.0_f32.min(ui.available_width() * 0.5), &sp.active_name(), &rows));
    match picked {
        Some(0) if !off => h.set_spatial(&dev.id, ""),
        Some(k) if k > 0 && !rows[k].1 => h.set_spatial(&dev.id, &sp.formats[k - 1].0),
        _ => {}
    }
}

fn format_picker(ui: &mut egui::Ui, h: &SoundPage, dev: &Device, detail: &Detail, choices: &Choices) {
    let Some(now) = detail.format else {
        page::row(ui, "Format", "Windows does not report this device's format.", |_| {});
        return;
    };
    // Several layouts in the list (stereo, 5.1, 7.1) need the channels said.
    let layouts = detail.supported.iter().any(|f| f.channels != now.channels);
    let text = |f: &shellman_core::system::sound_format::FormatSpec| if layouts || f.encoded { f.full_label() } else { f.label() };
    if let FormatControl::Locked { by } = &choices.format {
        // Spatial sound decides this one: shown, not offered, so a PCM pick cannot knock it out.
        page::row(ui, "Format", &sound_choices::locked_note(by), |ui| {
            ui.add_enabled(false, egui::Button::new(text(&now)));
        });
        return;
    }
    if detail.supported.is_empty() {
        page::row(ui, "Format", &format!("{} (this device offers no other formats)", text(&now)), |_| {});
        return;
    }
    let rows: Vec<(String, bool)> = detail.supported.iter().map(|f| (text(f), *f == now)).collect();
    let (picked, undo) = page::row(ui, "Format", "", |ui| {
        // Right to left: Undo at the row's end, the picker before it.
        let undo = detail.can_undo && ui.button("Undo").on_hover_text("Back to the format it had before").clicked();
        let picked = dropdown(ui, ("format", &dev.id), 260.0_f32.min(ui.available_width() * 0.6), &text(&now), &rows);
        (picked, undo)
    });
    if let Some(k) = picked {
        let f = detail.supported[k];
        if f != now {
            h.set_format(&dev.id, f);
        }
    }
    if undo {
        h.undo_format(&dev.id);
    }
}
