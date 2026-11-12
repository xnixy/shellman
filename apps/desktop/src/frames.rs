//! Hosts the frames service (`shellman_core::frames`): works out the palette from shellman's
//! theme, watches `frames.json` and `appearance.json`, and re-applies when they, or Windows'
//! light/dark or accent, change. Runs on the controller's message loop; idle between events.

use shellman_core::frames::{self, FramesSettings, Palette, Rgb};
use shellman_core::watch::Watcher;
use shellman_ui::appearance::Appearance;
use shellman_ui::theme::Colors;
use std::cell::RefCell;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

/// A settings folder changed: the controller should look.
pub const WM_FRAMES_POLL: u32 = 0x8000 + 3;

thread_local! {
    static WATCHER: RefCell<Option<(Watcher, egui::Context)>> = const { RefCell::new(None) };
}

fn rgb(c: egui::Color32) -> Rgb {
    Rgb(c.r(), c.g(), c.b())
}

/// What the theme says the frames should look like: the page tone as the title bar, the text
/// colour, the accent for the focused border and the hairline (laid over the page) for the rest.
pub fn palette(c: &Colors) -> Palette {
    let [hr, hg, hb, ha] = c.hairline.to_srgba_unmultiplied();
    let over = |page: u8, line: u8| (page as f32 + (line as f32 - page as f32) * ha as f32 / 255.0).round() as u8;
    Palette {
        dark: !c.light,
        caption: rgb(c.bg),
        text: rgb(c.text),
        accent: rgb(c.accent_fill),
        dim: Rgb(over(c.bg.r(), hr), over(c.bg.g(), hg), over(c.bg.b(), hb)),
    }
}

fn current() -> (FramesSettings, Palette) {
    let look: Appearance = shellman_core::config::load("appearance");
    let colors = Colors::new(frames::system_is_light(), look.accent.resolve());
    (FramesSettings::load(), palette(&colors))
}

/// Start the service and watch the settings; `controller` gets `WM_FRAMES_POLL`.
pub fn start(controller: HWND) {
    let (settings, pal) = current();
    frames::start(settings, pal);
    let ctx = egui::Context::default();
    let ctl = controller.0 as isize;
    ctx.set_request_repaint_callback(move |_| unsafe {
        let _ = PostMessageW(Some(HWND(ctl as *mut _)), WM_FRAMES_POLL, WPARAM(0), LPARAM(0));
    });
    let watcher = Watcher::quick(ctx.clone());
    watcher.set_dirs(shellman_core::config::config_dir().into_iter().collect());
    WATCHER.with(|w| *w.borrow_mut() = Some((watcher, ctx)));
}

/// A settings file changed.
pub fn poll() {
    let changed = WATCHER.with(|w| {
        let w = w.borrow();
        let Some((watcher, ctx)) = w.as_ref() else { return false };
        // egui folds repaint requests until a pass has run, so run an empty one: otherwise only the
        // first change would ever be announced.
        let _ = ctx.run(Default::default(), |_| {});
        !watcher.poll().is_empty()
    });
    if changed {
        refresh();
    }
}

/// Re-read everything; the service does nothing when nothing differs.
pub fn refresh() {
    let (settings, pal) = current();
    frames::update(settings, pal);
}

/// Stop watching and hand every window's colours back.
pub fn stop() {
    WATCHER.with(|w| *w.borrow_mut() = None);
    frames::stop();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_palette_follows_the_theme() {
        let c = Colors::new(false, egui::Color32::from_rgb(0, 103, 192));
        let p = palette(&c);
        assert!(p.dark);
        assert_eq!(p.caption, rgb(c.bg));
        assert_eq!(p.text, rgb(c.text));
        // A white hairline over a dark page is a little lighter than the page.
        assert!(p.dim.0 > p.caption.0 && p.dim.0 < 80);
    }

    #[test]
    fn light_palette_is_light() {
        let p = palette(&Colors::new(true, egui::Color32::from_rgb(196, 54, 78)));
        assert!(!p.dark);
        // A black hairline over a light page is a little darker than the page.
        assert!(p.dim.0 < p.caption.0 && p.dim.0 > 180);
    }
}
