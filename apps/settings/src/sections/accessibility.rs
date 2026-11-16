//! Accessibility: Windows' vision, hearing and interaction settings. What Windows lets a program
//! change (`shellman_core::system::access`: visual effects, the text cursor, the touch indicator,
//! the keyboard aids, mouse keys, flashing on sounds) is changed here at once; Narrator, Magnifier,
//! the on-screen keyboard and Live captions are started from here. What only Windows' Settings
//! changes (text size, pointer, colour filters, contrast themes, mono audio, scroll bars, caption
//! style) shows its value and opens Windows' Settings on its page.

use super::live::Live;
use super::{slider_row, winlink};
use crate::page;
use crate::state::State;
use egui::Ui;
use shellman_core::system::access::{self as a, Access, Change, Flash, Keys, Program, Touch};
use shellman_ui::look_controls::choice_chips;
use shellman_ui::theme;
use std::time::Duration;

/// The page's live state; its worker runs while the page shows.
#[derive(Default)]
pub struct AccessView {
    live: Live<Access, Job>,
    /// The text cursor thickness while its knob is held.
    caret_drag: Option<u32>,
}

/// What the worker does: a change, or starting a program.
pub enum Job {
    Change(Change),
    Start(Program),
}

impl AccessView {
    pub fn stop(&mut self) {
        *self = Self::default();
    }
}

fn run(job: Job) -> Result<(), String> {
    match job {
        Job::Change(c) => a::apply(c).then_some(()).ok_or_else(|| "Windows did not take that change.".to_owned()),
        Job::Start(p) => a::start(p).then_some(()).ok_or_else(|| format!("{} did not start.", p.exe())),
    }
}

/// Queue a change; the page shows it at once.
fn change(v: &mut AccessView, c: Change) {
    v.live.change(Job::Change(c), |s| a::expect(s, c));
}

fn start(v: &mut AccessView, p: Program) {
    v.live.change(Job::Start(p), |s| s);
}

fn on_off(on: bool) -> &'static str {
    if on {
        "On"
    } else {
        "Off"
    }
}

pub fn show(ui: &mut Ui, st: &mut State) {
    page::heading(ui, "Accessibility", "Make Windows easier to see, hear and use. Changes apply at once.");
    st.access.live.start(ui.ctx(), Box::new(a::read), Box::new(run));
    st.access.live.poll();
    // Narrator, Magnifier and the keyboard start and stop on their own shortcuts too.
    st.access.live.refresh_every(ui.ctx(), Duration::from_secs(2));
    let Some(s) = st.access.live.shown.clone() else {
        page::group(ui, "", |ui| {
            ui.label(theme::muted("Reading Windows' accessibility settings..."));
        });
        return;
    };
    let host = winlink::host();
    winlink::host_group(ui, st, host);
    let v = &mut st.access;
    if let Some(n) = &v.live.notice {
        let n = n.clone();
        page::group(ui, "", |ui| {
            ui.label(theme::muted(n).size(12.0));
        });
    }
    vision(ui, v, &s, host);
    hearing(ui, v, &s, host);
    interaction(ui, v, &s, host);
}

fn vision(ui: &mut Ui, v: &mut AccessView, s: &Access, host: bool) {
    page::group(ui, "Text and colour", |ui| {
        let detail = format!("{}%. shellman's own windows follow it too.", s.look.text_scale);
        winlink::row(ui, "Text size", &detail, "ms-settings:easeofaccess-display", host);
        ui.add_space(8.0);
        let value = if s.look.color_filter { format!("On: {}", a::filter_label(s.look.color_filter_kind)) } else { "Off".to_owned() };
        let value = format!("{value}. Keyboard shortcut {}", if s.look.color_filter_hotkey { "on" } else { "off" });
        winlink::row(ui, "Colour filters", &value, "ms-settings:easeofaccess-colorfilter", host);
        ui.add_space(8.0);
        let value = match &s.high_contrast {
            Some(h) if h.on && !h.theme.is_empty() => format!("On: {}", h.theme),
            Some(h) if h.on => "On".to_owned(),
            Some(_) => "None".to_owned(),
            None => "Windows did not say".to_owned(),
        };
        let value = match &s.high_contrast {
            Some(h) => format!("{value}. Left Alt + Left Shift + Print Screen {}", if h.hotkey { "turns one on" } else { "is off" }),
            None => value,
        };
        winlink::row(ui, "Contrast theme", &value, "ms-settings:easeofaccess-highcontrast", host);
    });
    page::group(ui, "Visual effects", |ui| {
        if let Some(on) = winlink::setting(ui, "Transparency effects", "Windows and surfaces look see-through.", s.transparency, None) {
            change(v, Change::Transparency(on));
        }
        ui.add_space(8.0);
        match s.animations {
            Some(anim) => {
                if let Some(on) = winlink::setting(ui, "Animation effects", "Windows and controls move when they open and close.", anim, None) {
                    change(v, Change::Animations(on));
                }
            }
            None => winlink::row(ui, "Animation effects", "Windows did not say.", "ms-settings:easeofaccess-visualeffects", host),
        }
        ui.add_space(8.0);
        winlink::row(ui, "Always show scroll bars", on_off(s.look.scrollbars_always), "ms-settings:easeofaccess-visualeffects", host);
        if let Some(secs) = s.message_seconds {
            ui.add_space(12.0);
            page::label(ui, "Dismiss notifications after this amount of time");
            let labels: Vec<String> = a::MESSAGE_SECONDS.iter().map(|s| a::seconds_label(*s)).collect();
            let opts: Vec<(u32, &str)> = a::MESSAGE_SECONDS.iter().copied().zip(labels.iter().map(String::as_str)).collect();
            if let Some(n) = choice_chips(ui, a::nearest_seconds(secs), &opts, true) {
                change(v, Change::MessageSeconds(n));
            }
        }
    });
    page::group(ui, "Mouse pointer and touch", |ui| {
        let detail = format!("{}, size {} of 15", a::pointer_style_label(s.look.pointer_style), s.look.pointer_size);
        winlink::row(ui, "Mouse pointer style and size", &detail, "ms-settings:easeofaccess-mousepointer", host);
        if let Some(raw) = s.touch {
            ui.add_space(8.0);
            let t = Touch::from_raw(raw);
            if let Some(on) = winlink::setting(ui, "Touch indicator", "Show a circle where the screen is touched.", t != Touch::Off, None) {
                change(v, Change::Touch(if on { Touch::On } else { Touch::Off }));
            }
            if t != Touch::Off {
                ui.add_space(4.0);
                let mut larger = t == Touch::Larger;
                if theme::checkbox(ui, &mut larger, "Make the circle darker and larger").changed() {
                    change(v, Change::Touch(if larger { Touch::Larger } else { Touch::On }));
                }
            }
        }
    });
    page::group(ui, "Text cursor", |ui| {
        if let Some(w) = s.caret_width {
            let range = a::CARET_RANGE;
            let at = v.caret_drag.unwrap_or(w);
            let m = slider_row::show(ui, egui::Id::new("caret-width"), "Text cursor thickness", fraction(at, range), &format!("{at} px"));
            if let Some(f) = m.value {
                v.caret_drag = Some(value_at(f, range));
            }
            // Windows hears it once the knob is let go (at once from the keyboard).
            if !m.held {
                if let Some(nw) = v.caret_drag.take().filter(|nw| *nw != w) {
                    change(v, Change::CaretWidth(nw));
                }
            }
            // A preview of the thickness, as Windows shows one.
            let at = v.caret_drag.unwrap_or(at);
            let (rect, _) = ui.allocate_exact_size(egui::vec2(24.0, 22.0), egui::Sense::hover());
            ui.painter().rect_filled(egui::Rect::from_min_size(rect.min + egui::vec2(4.0, 0.0), egui::vec2(at as f32, 22.0)), 0.0, theme::colors().text);
            ui.add_space(8.0);
        }
        winlink::row(ui, "Text cursor indicator", "A coloured marker on the text cursor.", "ms-settings:easeofaccess-cursor", host);
    });
    page::group(ui, "Narrator and Magnifier", |ui| {
        program(ui, v, Program::Narrator, s.narrator, "Reads the screen aloud. Windows + Ctrl + Enter turns it on and off; so does Caps Lock + Esc while it runs.");
        ui.add_space(8.0);
        winlink::row(ui, "Narrator's voice, verbosity and shortcuts", "", "ms-settings:easeofaccess-narrator", host);
        ui.add_space(12.0);
        program(ui, v, Program::Magnifier, s.magnifier, "Zooms in on part of the screen. Windows + Plus turns it on, Windows + Esc turns it off.");
        ui.add_space(8.0);
        winlink::row(ui, "Magnifier's zoom, views and shortcuts", "", "ms-settings:easeofaccess-magnifier", host);
    });
}

fn hearing(ui: &mut Ui, v: &mut AccessView, s: &Access, host: bool) {
    page::group(ui, "Audio", |ui| {
        winlink::row(ui, "Mono audio", on_off(s.look.mono_audio), "ms-settings:easeofaccess-audio", host);
        if s.sound_sentry.is_some() {
            ui.add_space(12.0);
            page::label(ui, "Flash during audio notifications");
            let opts: Vec<(Flash, &str)> = Flash::ALL.iter().map(|f| (*f, f.label())).collect();
            if let Some(f) = choice_chips(ui, s.flash(), &opts, true) {
                change(v, Change::Flash(f));
            }
        }
    });
    page::group(ui, "Captions", |ui| {
        if s.has_live_captions {
            program(ui, v, Program::LiveCaptions, s.live_captions, "Captions of what any app plays. Windows + Ctrl + L turns them on.");
            ui.add_space(8.0);
        }
        winlink::row(ui, "Caption style", "", "ms-settings:easeofaccess-closedcaptioning", host);
    });
}

fn interaction(ui: &mut Ui, v: &mut AccessView, s: &Access, host: bool) {
    page::group(ui, "Keyboard", |ui| {
        let aids: [(Keys, &str, &str, &str); 3] = [
            (Keys::Sticky, "Sticky keys", "Press Shift, Ctrl, Alt or Windows one at a time for a shortcut.", "Keyboard shortcut: press Shift five times"),
            (Keys::Filter, "Filter keys", "Ignore brief or repeated keystrokes.", "Keyboard shortcut: hold the right Shift for eight seconds"),
            (Keys::Toggle, "Toggle keys", "Play a sound when Caps Lock, Num Lock or Scroll Lock is pressed.", "Keyboard shortcut: hold Num Lock for five seconds"),
        ];
        let mut first = true;
        for (k, title, detail, shortcut) in aids {
            let Some(flags) = s.keys(k) else { continue };
            if !first {
                ui.add_space(10.0);
            }
            first = false;
            keys_rows(ui, v, k, flags, title, detail, shortcut);
        }
        if let Some(cues) = s.keyboard_cues {
            ui.add_space(10.0);
            if let Some(on) = winlink::setting(ui, "Underline access keys", "Underline the letter that picks a menu item or button with Alt.", cues, None) {
                change(v, Change::KeyboardCues(on));
            }
        }
        ui.add_space(10.0);
        program(ui, v, Program::Keyboard, s.keyboard, "A keyboard on the screen, for a mouse, pen or touch.");
        ui.add_space(8.0);
        winlink::row(ui, "More keyboard settings", "", "ms-settings:easeofaccess-keyboard", host);
    });
    page::group(ui, "Mouse", |ui| {
        if let Some(flags) = s.mouse_keys {
            keys_rows(ui, v, Keys::Mouse, flags, "Mouse keys", "Move the pointer with the number pad.", "Keyboard shortcut: Left Alt + Left Shift + Num Lock");
            ui.add_space(8.0);
        }
        winlink::row(ui, "Mouse keys speed", "", "ms-settings:easeofaccess-mouse", host);
    });
}

/// A keyboard aid's switch, and the checkbox for its shortcut.
fn keys_rows(ui: &mut Ui, v: &mut AccessView, k: Keys, flags: u32, title: &str, detail: &str, shortcut: &str) {
    if let Some(on) = winlink::setting(ui, title, detail, flags & a::ON != 0, None) {
        change(v, Change::KeysOn(k, on));
    }
    ui.add_space(4.0);
    let mut hot = flags & a::HOTKEY != 0;
    if theme::checkbox(ui, &mut hot, shortcut).changed() {
        change(v, Change::KeysHotkey(k, hot));
    }
}

/// One of Windows' programs: whether it runs, and a button to start it.
fn program(ui: &mut Ui, v: &mut AccessView, p: Program, running: bool, detail: &str) {
    let name = match p {
        Program::Narrator => "Narrator",
        Program::Magnifier => "Magnifier",
        Program::Keyboard => "On-screen keyboard",
        Program::LiveCaptions => "Live captions",
    };
    let c = theme::colors();
    let pressed = theme::trailing(
        ui,
        |ui| {
            if running {
                ui.label(theme::muted("Running").size(12.5));
                false
            } else {
                ui.add(theme::secondary_button(&format!("Start {}", name.to_lowercase()))).clicked()
            }
        },
        |ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.add(egui::Label::new(egui::RichText::new(name).size(13.0).color(c.text)).wrap());
                ui.add(egui::Label::new(egui::RichText::new(detail).size(11.5).color(c.muted)).wrap());
            });
        },
    );
    if pressed {
        start(v, p);
    }
}

fn fraction(v: u32, (lo, hi): (u32, u32)) -> f32 {
    (v.clamp(lo, hi) - lo) as f32 / (hi - lo).max(1) as f32
}

fn value_at(f: f32, (lo, hi): (u32, u32)) -> u32 {
    lo + (f.clamp(0.0, 1.0) * (hi - lo) as f32).round() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caret_slider_round_trips() {
        for w in 1..=20 {
            assert_eq!(value_at(fraction(w, a::CARET_RANGE), a::CARET_RANGE), w);
        }
        assert_eq!(value_at(2.0, a::CARET_RANGE), 20);
    }
}
