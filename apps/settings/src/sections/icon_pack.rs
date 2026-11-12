//! The icon pack group (on the Appearance page): the installed packs, each with a strip of icons
//! it would give and how many of this PC's apps it has one for; one is chosen, and a folder can
//! be added. The choice is saved to `icons.json`, which every shellman app follows.

use crate::pick::Pick;
use crate::state::State;
use egui::{Align, CornerRadius, FontId, Layout, Rect, Sense, Stroke, StrokeKind, UiBuilder, Vec2};
use shellman_core::icon_pack::{self, import, IconSettings};
use shellman_core::thumbs::Thumbs;
use shellman_pick::Mode;
use shellman_ui::theme::colors;
use shellman_ui::widgets::draw_text;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// Icons in a pack's strip.
const PREVIEW: usize = 8;
const ICON: f32 = 30.0;
const ROW_H: f32 = 98.0;

/// What reading one pack found.
#[derive(Clone, Default)]
struct PackInfo {
    name: String,
    preview: Vec<PathBuf>,
    icons: usize,
    matched: usize,
    total: usize,
}

type Slot<T> = Arc<Mutex<Option<T>>>;

enum Work {
    /// The folder chooser is open.
    Choosing(Pick),
    /// Copying a folder into the packs folder.
    Importing { count: Arc<AtomicUsize>, done: Slot<Result<Vec<String>, String>> },
}

/// The group's live state. Reading a pack and copying one happen on their own threads.
#[derive(Default)]
pub struct IconsView {
    settings: IconSettings,
    generation: u64,
    packs: Vec<PackInfo>,
    /// The pack names the list was read for.
    listed: Vec<String>,
    scan: Slot<Vec<PackInfo>>,
    scanning: bool,
    scanned_once: bool,
    last_check: f64,
    work: Option<Work>,
    /// The pack whose removal is waiting for a second press.
    confirm_remove: Option<String>,
    note: Option<String>,
}

impl IconsView {
    /// Keep up with the file (this window's own saves, and any other change to it).
    fn follow_settings(&mut self) {
        let generation = icon_pack::generation();
        if generation != self.generation {
            self.generation = generation;
            self.settings = icon_pack::current_settings();
        }
    }

    fn save(&mut self) {
        self.settings.save();
    }

    fn start_scan(&mut self, ctx: &egui::Context, names: Vec<String>) {
        self.scanning = true;
        self.listed = names.clone();
        let (slot, ctx) = (self.scan.clone(), ctx.clone());
        std::thread::spawn(move || {
            let apps = shellman_core::apps_index::enumerate();
            let packs: Vec<PackInfo> = names
                .into_iter()
                .map(|name| {
                    let index = icon_pack::read_pack(&name);
                    let preview = index.sample(&icon_pack::SHOWCASE).into_iter().map(|(_, p)| p).take(PREVIEW).collect();
                    let (matched, total) = icon_pack::coverage(&index, &apps);
                    PackInfo { name, preview, icons: index.len(), matched, total }
                })
                .collect();
            *slot.lock().unwrap() = Some(packs);
            ctx.request_repaint();
        });
    }

    /// Take in what the threads finished, and read the packs again when the folder changed.
    fn poll(&mut self, ctx: &egui::Context) {
        if let Some(packs) = self.scan.lock().unwrap().take() {
            self.packs = packs;
            self.scanning = false;
            self.scanned_once = true;
        }
        let mut finished = None;
        match &mut self.work {
            Some(Work::Choosing(pick)) => {
                if let Some(choice) = pick.poll() {
                    finished = Some(Ok(choice));
                }
            }
            Some(Work::Importing { done, .. }) => {
                if let Some(result) = done.lock().unwrap().take() {
                    finished = Some(Err(result));
                }
            }
            None => {}
        }
        match finished {
            Some(Ok(Some(folder))) => self.begin_import(ctx, folder),
            Some(Ok(None)) => self.work = None,
            Some(Err(Ok(added))) => {
                self.work = None;
                self.note = None;
                // A pack added to nothing becomes the pack: that is what adding one is for.
                if self.settings.pack.is_none() {
                    if let Some(first) = added.first() {
                        self.settings.pack = Some(first.clone());
                        self.save();
                    }
                }
                self.listed.clear();
            }
            Some(Err(Err(message))) => {
                self.work = None;
                self.note = Some(message);
            }
            None => {}
        }
        let now = ctx.input(|i| i.time);
        if !self.scanning && now - self.last_check > 1.0 {
            self.last_check = now;
            let names = import::list();
            if names != self.listed || !self.scanned_once {
                self.start_scan(ctx, names);
            }
        }
        if self.scanning || self.work.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(120));
        }
    }

    fn begin_import(&mut self, ctx: &egui::Context, folder: PathBuf) {
        let count = Arc::new(AtomicUsize::new(0));
        let done: Slot<Result<Vec<String>, String>> = Arc::default();
        let (c, d, ctx) = (count.clone(), done.clone(), ctx.clone());
        std::thread::spawn(move || {
            let result = import::add_from_folder(&folder, &c).map_err(|e| match e.kind() {
                std::io::ErrorKind::InvalidInput => "That folder holds no icons: expected an icon theme, or images named by app.".to_string(),
                _ => format!("Could not add the pack: {e}"),
            });
            *d.lock().unwrap() = Some(result);
            ctx.request_repaint();
        });
        self.work = Some(Work::Importing { count, done });
    }
}

fn small_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let c = colors();
    let button = egui::Button::new(egui::RichText::new(text).size(13.0).color(c.text))
        .fill(c.control_hover)
        .stroke(Stroke::NONE)
        .corner_radius(CornerRadius::same(8))
        .min_size(Vec2::new(0.0, 30.0));
    ui.add(button)
}

pub fn group(ui: &mut egui::Ui, st: &mut State) {
    let ctx = ui.ctx().clone();
    let v = &mut st.icons;
    v.follow_settings();
    v.poll(&ctx);
    let thumbs = &mut st.thumbs;

    ui.spacing_mut().item_spacing.y = 8.0;
    let none = v.settings.pack.is_none();
    if none_row(ui, none).clicked() && !none {
        v.settings.pack = None;
        v.save();
    }
    let mut remove: Option<String> = None;
    let mut choose: Option<String> = None;
    for info in &v.packs {
        let selected = v.settings.pack.as_deref() == Some(info.name.as_str());
        let asking = v.confirm_remove.as_deref() == Some(info.name.as_str());
        match pack_row(ui, info, selected, asking, thumbs) {
            Some(Press::Choose) => choose = Some(info.name.clone()),
            Some(Press::Remove) => remove = Some(info.name.clone()),
            Some(Press::ConfirmRemove) => v.confirm_remove = Some(info.name.clone()),
            Some(Press::Keep) => v.confirm_remove = None,
            None => {}
        }
    }
    if v.packs.is_empty() {
        let text = if v.scanned_once {
            "No icon packs yet. Add a folder with an icon theme (Papirus, Tela, WhiteSur...) or with images named by app."
        } else {
            "Reading the icon packs..."
        };
        ui.label(shellman_ui::theme::muted(text).size(12.5));
    }
    if let Some(name) = choose {
        v.settings.pack = Some(name);
        v.save();
    }
    if let Some(name) = remove {
        v.confirm_remove = None;
        if v.settings.pack.as_deref() == Some(name.as_str()) {
            v.settings.pack = None;
            v.save();
        }
        if let Err(e) = import::remove(&name) {
            v.note = Some(format!("Could not remove {name}: {e}"));
        }
        v.listed.clear();
    }

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let busy = v.work.is_some();
        ui.add_enabled_ui(!busy, |ui| {
            if small_button(ui, "Add pack from folder...").clicked() {
                match Pick::start(Mode::Folder, "Choose a folder with an icon pack", Vec::new()) {
                    Ok(pick) => {
                        v.note = None;
                        v.work = Some(Work::Choosing(pick));
                    }
                    Err(e) => v.note = Some(e),
                }
            }
        });
        let status = match &v.work {
            Some(Work::Importing { count, .. }) => Some(format!("Copying icons... {}", count.load(Ordering::Relaxed))),
            Some(Work::Choosing(_)) => Some("Choose a folder in the window that opened.".to_string()),
            None => v.note.clone(),
        };
        if let Some(text) = status {
            ui.label(shellman_ui::theme::muted(text).size(12.0));
        }
    });
    ui.add_space(2.0);
    ui.label(
        shellman_ui::theme::muted("Icon themes (Papirus, Tela, WhiteSur, Fluent...) and plain folders of images named by app both work. Apps the pack has no icon for keep their own.")
            .size(11.5),
    );
}

enum Press {
    Choose,
    ConfirmRemove,
    Remove,
    Keep,
}

/// The "no pack" choice.
fn none_row(ui: &mut egui::Ui, selected: bool) -> egui::Response {
    let c = colors();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 52.0), Sense::click());
    paint_card(ui, rect, selected, resp.hovered());
    draw_text(ui, Rect::from_min_size(rect.min + Vec2::new(16.0, 8.0), Vec2::new(rect.width() - 32.0, 20.0)), "None", FontId::proportional(14.0), c.text, false);
    draw_text(
        ui,
        Rect::from_min_size(rect.min + Vec2::new(16.0, 28.0), Vec2::new(rect.width() - 32.0, 16.0)),
        "Every app keeps its own icon",
        FontId::proportional(11.5),
        c.muted,
        false,
    );
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn paint_card(ui: &egui::Ui, rect: Rect, selected: bool, hovered: bool) {
    let c = colors();
    let fill = if hovered && !selected { c.control_hover } else { c.control };
    ui.painter().rect_filled(rect, CornerRadius::same(10), fill);
    if selected {
        ui.painter().rect_stroke(rect, CornerRadius::same(10), Stroke::new(2.0_f32, c.accent_fill), StrokeKind::Inside);
    }
}

/// One pack: its name, what it covers, a strip of its icons, and Remove.
fn pack_row(ui: &mut egui::Ui, info: &PackInfo, selected: bool, asking: bool, thumbs: &mut Thumbs) -> Option<Press> {
    let c = colors();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::click());
    paint_card(ui, rect, selected, resp.hovered());
    let mut press = resp.clone().on_hover_cursor(egui::CursorIcon::PointingHand).clicked().then_some(Press::Choose);

    let title_w = (rect.width() - 32.0 - 150.0).max(60.0);
    draw_text(ui, Rect::from_min_size(rect.min + Vec2::new(16.0, 10.0), Vec2::new(title_w, 20.0)), &info.name, FontId::proportional(14.0), c.text, false);
    let sub = if info.total > 0 {
        format!("{} of {} apps matched  \u{b7}  {} icons", info.matched, info.total, info.icons)
    } else {
        format!("{} icons", info.icons)
    };
    draw_text(ui, Rect::from_min_size(rect.min + Vec2::new(16.0, 30.0), Vec2::new(title_w, 16.0)), &sub, FontId::proportional(11.5), c.muted, false);

    // The strip of what the pack gives, left to right.
    let fit = (((rect.width() - 32.0) / (ICON + 8.0)).floor() as usize).clamp(1, PREVIEW);
    let top = rect.top() + 52.0;
    for (i, path) in info.preview.iter().take(fit).enumerate() {
        let cell = Rect::from_min_size(egui::pos2(rect.left() + 16.0 + i as f32 * (ICON + 8.0), top), Vec2::splat(ICON + 8.0));
        if let Some(tex) = thumbs.get(path, false) {
            let size = tex.size_vec2();
            let scale = (ICON / size.x).min(ICON / size.y);
            egui::Image::from_texture((tex.id(), size)).paint_at(ui, Rect::from_center_size(cell.center(), size * scale));
        }
    }
    if info.preview.is_empty() {
        draw_text(ui, Rect::from_min_size(egui::pos2(rect.left() + 16.0, top + 4.0), Vec2::new(rect.width() - 32.0, 20.0)), "No icons for popular apps in this pack", FontId::proportional(11.5), c.muted, false);
    }

    // Remove, on the right of the first line; asking first.
    let area = Rect::from_min_size(egui::pos2(rect.right() - 190.0, rect.top() + 6.0), Vec2::new(182.0, 34.0));
    let mut child = ui.new_child(UiBuilder::new().max_rect(area).layout(Layout::right_to_left(Align::Center)));
    child.spacing_mut().item_spacing.x = 8.0;
    if asking {
        if small_button(&mut child, "Remove").clicked() {
            press = Some(Press::Remove);
        }
        if small_button(&mut child, "Keep").clicked() {
            press = Some(Press::Keep);
        }
    } else if child.add(egui::Label::new(shellman_ui::theme::muted("Remove").size(12.0)).sense(Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
        press = Some(Press::ConfirmRemove);
    }
    press
}
