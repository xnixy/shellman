//! The control centre window: the modules top to bottom, live while the panel shows.
//!
//! Each module keeps its last known state while the panel is hidden, so a summon paints at once;
//! its watcher starts with the showing and brings fresh readings within moments. Hiding drops
//! every watcher (and the outside-click hook), so a hidden panel uses no CPU at all.

use crate::power_menu::PowerMenu;
use crate::summon::{self, Shared};
use crate::views::{connect, display::DisplayView, footer, kit, media::MediaView, sound::SoundView};
use crate::outside;
use shellman_core::system::audio_watch::SoundWatch;
use shellman_core::system::brightness::BrightnessWatch;
use shellman_core::system::live::Wake;
use shellman_core::system::media::MediaWatch;
use shellman_core::system::power::{self, Action};
use shellman_core::system::radios::{Connectivity, ConnectivityWatch};
use shellman_core::system::status::{self, Battery};
use shellman_ui::theme::{self, colors};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// If the window has no keyboard focus this long after appearing, ask for it again.
const FOCUS_RETRY: Duration = Duration::from_millis(250);

/// What runs only while the panel shows.
struct Live {
    sound: SoundWatch,
    connectivity: ConnectivityWatch,
    brightness: BrightnessWatch,
    media: MediaWatch,
}

pub struct Control {
    hwnd: isize,
    /// Whether the window has real acrylic; without it the panel is painted opaque.
    glass: bool,
    shared: Arc<Shared>,
    wake: Wake,
    live: Option<Live>,
    net: Connectivity,
    sound: SoundView,
    display: DisplayView,
    media: MediaView,
    battery: Option<Battery>,
    power: PowerMenu,
    /// The showing this window last saw, whether it has had focus since, and when it appeared.
    seen_shown: u32,
    had_focus: bool,
    shown_at: Instant,
}

impl Control {
    pub fn new(ctx: egui::Context, hwnd: isize, glass: bool, shared: Arc<Shared>) -> Self {
        Self {
            hwnd,
            glass,
            shared,
            wake: Arc::new(move || ctx.request_repaint()),
            live: None,
            net: Connectivity::default(),
            sound: SoundView::default(),
            display: DisplayView::default(),
            media: MediaView::default(),
            battery: None,
            power: PowerMenu::default(),
            seen_shown: 0,
            had_focus: false,
            shown_at: Instant::now(),
        }
    }

    pub fn hwnd(&self) -> isize {
        self.hwnd
    }

    /// The panel just appeared: start listening.
    fn opened(&mut self, ctx: &egui::Context) {
        self.had_focus = false;
        self.shown_at = Instant::now();
        // One wake-up to check that focus arrived; no repeating timer.
        ctx.request_repaint_after(FOCUS_RETRY + Duration::from_millis(10));
        self.battery = status::battery();
        self.live = Some(Live {
            sound: SoundWatch::start(self.wake.clone()),
            connectivity: ConnectivityWatch::start(self.wake.clone()),
            brightness: BrightnessWatch::start(self.wake.clone()),
            media: MediaWatch::start(self.wake.clone()),
        });
        let repaint = ctx.clone();
        outside::start(move || repaint.request_repaint());
    }

    /// Out of sight, and nothing left running.
    fn hide(&mut self) {
        summon::hide(self.hwnd, &self.shared);
        outside::stop();
        self.live = None;
        self.power = PowerMenu::default();
    }

    /// Notice a new showing, and hide on Esc, a second summon, a click elsewhere, or focus
    /// lost after having had it.
    fn follow_window(&mut self, ctx: &egui::Context) -> bool {
        let shown = self.shared.shown_count();
        if shown != self.seen_shown {
            self.seen_shown = shown;
            self.opened(ctx);
        }
        if self.live.is_none() {
            return false;
        }
        if outside::take_click() {
            self.shared.closed_by_click();
            self.hide();
            return false;
        }
        let esc = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        if self.shared.take_hide_request() || esc {
            self.hide();
            return false;
        }
        let focused = ctx.input(|i| i.focused);
        if focused {
            self.had_focus = true;
        } else if self.had_focus {
            self.hide();
            return false;
        } else if self.shown_at.elapsed() > FOCUS_RETRY && self.shown_at.elapsed() < FOCUS_RETRY * 2 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        true
    }

    pub fn frame(&mut self, ctx: &egui::Context) {
        let showing = self.follow_window(ctx);
        if let Some(live) = &self.live {
            if let Some(n) = live.connectivity.take() {
                self.net = n;
            }
            self.sound.take(&live.sound);
            self.display.take(&live.brightness);
            self.media.take(&live.media);
        }
        theme::set_light(ctx.theme() == egui::Theme::Light);
        let mut asked = Asked::None;
        let mut height = 0.0;
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(egui::Margin::same(kit::PAD as i8)))
            .show(ctx, |ui| {
                paint_surface(ui, ctx.screen_rect(), self.glass);
                ui.spacing_mut().item_spacing = egui::vec2(kit::GAP, kit::GAP);
                asked = self.modules(ui);
                height = ui.min_rect().height() + kit::PAD * 2.0;
            });
        if showing {
            summon::fit(self.hwnd, &self.shared, height);
        }
        self.act(asked);
    }

    fn modules(&mut self, ui: &mut egui::Ui) -> Asked {
        let live = self.live.as_ref();
        let mut asked = Asked::None;
        if let Some(a) = connect::show(ui, &self.net) {
            asked = Asked::Connect(a);
        }
        self.sound.show(ui, live.map(|l| &l.sound));
        self.display.show(ui, live.map(|l| &l.brightness));
        self.media.show(ui, live.map(|l| &l.media));
        if let Some(b) = self.battery {
            footer::battery(ui, b);
        }
        if let Some(a) = footer::show(ui, &mut self.power) {
            asked = Asked::Footer(a);
        }
        asked
    }

    fn act(&mut self, asked: Asked) {
        match asked {
            Asked::None => {}
            Asked::Connect(connect::Asked::Toggle(t)) => {
                connect::assume(&mut self.net, t);
                if let Some(live) = &self.live {
                    match t {
                        connect::Toggle::Radio(kind, on) => live.connectivity.set(kind, on),
                        connect::Toggle::Focus(on) => live.connectivity.set_focus(on),
                    }
                }
            }
            Asked::Connect(connect::Asked::Open(uri)) => {
                self.hide();
                open_uri(uri);
            }
            Asked::Footer(footer::Asked::Settings) => {
                self.hide();
                shellman_core::sibling::start("settings");
            }
            Asked::Footer(footer::Asked::Power(action)) => {
                self.hide();
                perform_later(action);
            }
        }
    }
}

enum Asked {
    None,
    Connect(connect::Asked),
    Footer(footer::Asked),
}

/// The panel's background, the one place it is painted: the window is DWM acrylic (or opaque
/// without it), tinted like the launcher. The rounded real-blur surface from `shellman_ui` will
/// replace this function's body.
fn paint_surface(ui: &egui::Ui, rect: egui::Rect, glass: bool) {
    let c = colors();
    if glass {
        ui.painter().rect_filled(rect, 0.0, c.shell);
        ui.painter().rect_filled(rect, 0.0, c.sheet);
    } else {
        ui.painter().rect_filled(rect, 0.0, c.bg);
    }
}

/// Open a Settings page (`ms-settings:` URI) off the UI thread.
fn open_uri(uri: &'static str) {
    std::thread::spawn(move || {
        use windows::core::HSTRING;
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            ShellExecuteW(None, &HSTRING::from("open"), &HSTRING::from(uri), None, None, SW_SHOWNORMAL);
        }
    });
}

/// Lock, sleep and the rest, once the panel is gone, so it is not the last thing on screen.
fn perform_later(action: Action) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(150));
        power::perform(action);
    });
}
