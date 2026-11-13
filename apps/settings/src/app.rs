//! The window: islands of glass with the desktop showing between them. A pill rail of sections
//! at the left, a title pill on top, and the page below it as one island per group, scrolling
//! under the title pill.

use crate::instance::Instance;
use crate::page;
use crate::search::{self, Hit};
use crate::sections::Section;
use crate::state::State;
use egui::{pos2, vec2, Align2, Rect, Sense, UiBuilder};
use shellman_ui::chips::{self, Chip};
use shellman_ui::glass::islands::{self, Island};
use shellman_ui::glass::{GlassPanel, Style};
use shellman_ui::rail;
use shellman_ui::theme::{self, colors, glyph, icon_font, semibold_font, AccentChoice};
use std::time::Duration;

/// Space between the window's edge and the islands, and between islands.
const MARGIN: f32 = 12.0;
const TITLE_H: f32 = 46.0;
const BUTTON_H: f32 = 30.0;
/// The page's column is kept to a readable width, however wide the window.
const COLUMN_MAX: f32 = 860.0;
/// The search field in the title pill.
const SEARCH_W: f32 = 210.0;
const SEARCH_ID: &str = "settings-search";
/// How long to wait before reading the files again if our own saves were still going.
const RELOAD_RETRY: Duration = Duration::from_millis(100);

pub struct SettingsApp {
    st: State,
    section: Section,
    /// What is typed in the title pill's search field.
    query: String,
    panel: GlassPanel,
    /// Wakes the window when something in the settings folder changes.
    watcher: shellman_core::watch::Watcher,
    reload_due: bool,
    /// Keeps the single-instance mutex for as long as the process runs.
    _instance: Instance,
}

impl SettingsApp {
    pub fn new(
        ctx: &egui::Context,
        look: shellman_ui::appearance::Appearance,
        panel: GlassPanel,
        instance: Instance,
        page: Option<Section>,
    ) -> Self {
        let watcher = shellman_core::watch::Watcher::new(ctx.clone());
        if let Some(dir) = shellman_core::config::config_dir() {
            // The folder may not exist yet; saving makes it, and the first run saves the look.
            watcher.set_dirs(vec![dir]);
        }
        let glass = panel.composed();
        Self {
            st: State::new(ctx, look, glass),
            section: page.unwrap_or(Section::Appearance),
            query: String::new(),
            panel,
            watcher,
            reload_due: false,
            _instance: instance,
        }
    }

    /// Follow the Windows app mode, and the Windows accent while that is the choice. Checked as
    /// frames come, not on a timer.
    fn track_system(&self, ctx: &egui::Context) {
        theme::set_light(ctx.theme() == egui::Theme::Light);
        if self.st.look.accent == AccentChoice::System {
            theme::set_accent(ctx, AccentChoice::System.resolve());
        }
    }

    /// Take in changes other apps saved (the dock pinning an app), once our own saves are done.
    fn follow_files(&mut self, ctx: &egui::Context) {
        if !self.watcher.poll().is_empty() {
            self.reload_due = true;
        }
        if !self.reload_due {
            return;
        }
        if self.st.saver.idle() {
            self.reload_due = false;
            self.st.reload(ctx);
        } else {
            ctx.request_repaint_after(RELOAD_RETRY);
        }
    }

    /// Go to the page another run asked for (`--page`, from Control Center's link).
    fn follow_requests(&mut self) {
        let asked = self._instance.requested.lock().ok().and_then(|mut r| r.take());
        if let Some(section) = asked {
            self.section = section;
        }
    }

    fn layout(&mut self, ctx: &egui::Context) {
        self.panel.set_style(Style::from_theme(page::GROUP_RADIUS));
        let mut list: Vec<Island> = Vec::new();
        egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx, |ui| {
            let screen = ctx.screen_rect();
            let under = ui.painter().add(egui::Shape::Noop);
            islands::drag_gaps(ui, screen);

            // The title pill and the page share one column; the rail stands left of it.
            let rail_w = rail::width(ui, &self.rail_entries(), rail::mode(screen.width(), self.st.look.rail_collapsed));
            let left = screen.left() + MARGIN + rail_w + MARGIN;
            let room = screen.right() - MARGIN - left;
            let buttons_w = BUTTON_H * 3.0;
            let width = room.min(COLUMN_MAX);
            // Wider than the column, the window buttons sit at the window's corner instead.
            let apart = room > width + MARGIN + buttons_w + 16.0;
            let title = Rect::from_min_size(pos2(left, screen.top() + MARGIN), vec2(width, TITLE_H));
            let content_top = title.bottom() + MARGIN;

            list.push(Island::pill(title));
            self.title(ui, title, !apart);
            if apart {
                let corner = Rect::from_min_size(
                    pos2(screen.right() - MARGIN - buttons_w - 16.0, title.top()),
                    vec2(buttons_w + 16.0, TITLE_H),
                );
                list.push(Island::pill(corner));
                chips::window_buttons(ui, Rect::from_center_size(corner.center(), vec2(buttons_w, BUTTON_H)));
            }
            self.rail(ui, &mut list, screen, content_top);

            let area = Rect::from_min_max(pos2(left, content_top), pos2(left + width, screen.bottom() - MARGIN));
            self.page(ui, area, &mut list);

            if !self.panel.composed() {
                ui.painter().set(under, egui::Shape::Vec(islands::solid_shapes(&list, &self.panel.style())));
            }
        });
        self.panel.set_islands(&list, ctx.pixels_per_point());
    }

    /// The pill on top: "Settings / <page>", the search field, and the window buttons when
    /// `buttons` is set.
    fn title(&mut self, ui: &mut egui::Ui, rect: Rect, buttons: bool) {
        let c = colors();
        let y = rect.center().y;
        let name = ui.painter().text(pos2(rect.left() + 22.0, y), Align2::LEFT_CENTER, "Settings", semibold_font(15.0), c.text);
        let slash = ui.painter().text(pos2(name.right() + 10.0, y), Align2::LEFT_CENTER, "/", egui::FontId::proportional(15.0), c.muted);
        let label = self.section.label();
        let w = chips::chip_width(ui, label, false, Chip::Accent);
        let chip = Rect::from_center_size(pos2(slash.right() + 10.0 + w / 2.0, y), vec2(w, chips::CHIP_H));
        chips::chip(ui, chip, egui::Id::new("title-chip"), label, None, Chip::Accent);
        let mut right = rect.right() - 12.0;
        if buttons {
            chips::window_buttons(ui, Rect::from_min_max(pos2(rect.right() - 10.0 - BUTTON_H * 3.0, y - BUTTON_H / 2.0), pos2(rect.right() - 10.0, y + BUTTON_H / 2.0)));
            right -= BUTTON_H * 3.0 + 4.0;
        }
        // The field takes what the name leaves, up to its width.
        let room = right - (chip.right() + 16.0);
        if room > 90.0 {
            let field = Rect::from_min_max(pos2(right - room.min(SEARCH_W), y - chips::CHIP_H / 2.0 - 1.0), pos2(right, y + chips::CHIP_H / 2.0 + 1.0));
            self.search_field(ui, field);
        }
    }

    /// The search box: a soft pill with a glass glyph. Enter goes to the best match, Esc clears.
    fn search_field(&mut self, ui: &mut egui::Ui, field: Rect) {
        let c = colors();
        let id = egui::Id::new(SEARCH_ID);
        let (soft, hover, line) = chips::soft_colors(c.light);
        let focused = ui.memory(|m| m.has_focus(id));
        let fill = if focused || ui.rect_contains_pointer(field) { hover } else { soft };
        ui.painter().rect(field, field.height() / 2.0, fill, line, egui::StrokeKind::Inside);
        ui.painter().text(pos2(field.left() + 16.0, field.center().y), Align2::CENTER_CENTER, glyph::SEARCH, icon_font(13.0), c.muted);
        let edit_rect = Rect::from_min_max(pos2(field.left() + 30.0, field.top()), pos2(field.right() - 12.0, field.bottom()));
        let edit = egui::TextEdit::singleline(&mut self.query)
            .id(id)
            .font(egui::FontId::proportional(13.0))
            .frame(false)
            .margin(egui::Margin::ZERO)
            .vertical_align(egui::Align::Center)
            .hint_text(egui::RichText::new("Search settings").color(c.muted))
            .desired_width(edit_rect.width());
        let resp = ui.put(edit_rect, edit);
        let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if enter {
            if let Some(hit) = search::find(&self.query).first() {
                self.go(*hit);
            }
        }
        if (focused || !self.query.is_empty()) && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.query.clear();
            ui.memory_mut(|m| m.surrender_focus(id));
        }
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::F)) {
            ui.memory_mut(|m| m.request_focus(id));
        }
    }

    /// Open the page a search result leads to, and leave the search.
    fn go(&mut self, hit: Hit) {
        self.section = hit.section;
        self.query.clear();
    }

    /// The rail of sections (shared with the other apps): icon and name each, folded to icons
    /// by the user or by a narrow window.
    fn rail(&mut self, ui: &mut egui::Ui, list: &mut Vec<Island>, screen: Rect, top: f32) {
        let entries = self.rail_entries();
        let solid = (!self.panel.composed()).then(|| self.panel.style());
        let mode = rail::mode(screen.width(), self.st.look.rail_collapsed);
        let out = rail::show(ui, list, screen, top, &entries, Some(self.section.slug()), mode, solid.as_ref(), page::GROUP_RADIUS);
        if let Some(i) = out.clicked {
            self.section = Section::ALL[i];
            self.query.clear();
        }
        if out.toggled {
            self.st.look.rail_collapsed = !self.st.look.rail_collapsed;
            self.st.save_look();
        }
    }

    /// The rail's rows: one per section, in the section list's order.
    fn rail_entries(&self) -> Vec<rail::Entry<'static>> {
        Section::ALL.iter().map(|s| rail::Entry::Item { key: s.slug(), glyph: s.glyph(), label: s.label() }).collect()
    }

    /// The page, scrolling under the title pill: its groups become islands, clipped to `area`.
    fn page(&mut self, ui: &mut egui::Ui, area: Rect, list: &mut Vec<Island>) {
        page::begin_frame();
        let searching = !self.query.trim().is_empty();
        ui.scope_builder(UiBuilder::new().max_rect(area), |ui| {
            ui.spacing_mut().scroll = egui::style::ScrollStyle::floating();
            // Dragging the gaps between islands moves the window, so the area must not take it.
            egui::ScrollArea::vertical().auto_shrink([false, false]).drag_to_scroll(false).show(ui, |ui| {
                if searching {
                    self.results(ui);
                    return;
                }
                // A page that declares no groups goes on one island as a whole.
                if self.section.grouped() {
                    self.section.show(ui, &mut self.st);
                } else {
                    page::group(ui, "", |ui| self.section.show(ui, &mut self.st));
                }
                // The workers run only while their page is shown.
                if self.section != Section::Sound {
                    self.st.sound.stop();
                }
                if self.section != Section::Display {
                    self.st.display.stop();
                }
                if self.section != Section::Power {
                    self.st.power.stop();
                }
                if self.section != Section::DateTime {
                    self.st.datetime.stop();
                }
                if self.section != Section::Storage {
                    self.st.storage.stop();
                }
                if self.section != Section::Updates {
                    self.st.updates.stop();
                }
                if self.section != Section::DefaultApps {
                    self.st.default_apps.stop();
                }
                if self.section != Section::Mouse {
                    self.st.mouse.stop();
                }
                if self.section != Section::Startup {
                    self.st.startup.stop();
                }
                if self.section != Section::Apps {
                    self.st.apps.stop();
                }
                if self.section != Section::Fonts {
                    self.st.fonts.stop();
                }
            });
        });
        if let Some(section) = self.st.goto.take() {
            self.section = section;
        }
        list.extend(page::take_groups().into_iter().map(|r| Island::new(r, page::GROUP_RADIUS)));
    }
}

impl SettingsApp {
    /// The search's answer, in place of the page: one row for each place the query leads, the
    /// best one lifted out in the accent (Enter takes it).
    fn results(&mut self, ui: &mut egui::Ui) {
        let hits = search::find(&self.query);
        let mut chosen = None;
        page::group(ui, "Search results", |ui| {
            if hits.is_empty() {
                ui.label(shellman_ui::theme::muted("Nothing in Settings matches that."));
            }
            ui.spacing_mut().item_spacing.y = 4.0;
            for (i, hit) in hits.iter().enumerate() {
                if result_row(ui, hit, i == 0).clicked() {
                    chosen = Some(*hit);
                }
            }
        });
        if let Some(hit) = chosen {
            self.go(hit);
        }
    }
}

/// One search result: where it leads, with "Enter" on the best one.
fn result_row(ui: &mut egui::Ui, hit: &Hit, best: bool) -> egui::Response {
    let c = colors();
    let (row, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::click());
    let fill = if best {
        c.accent_fill
    } else if resp.hovered() {
        c.control_hover
    } else {
        c.control
    };
    ui.painter().rect_filled(row, 14.0, fill);
    let ink = if best { c.on_accent } else { c.text };
    ui.painter().text(pos2(row.left() + 18.0, row.center().y), Align2::LEFT_CENTER, hit.label(), egui::FontId::proportional(14.0), ink);
    if best {
        ui.painter().text(pos2(row.right() - 16.0, row.center().y), Align2::RIGHT_CENTER, "Enter", egui::FontId::proportional(12.0), ink.gamma_multiply(0.85));
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

impl eframe::App for SettingsApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.st.thumbs.poll(ctx);
        self.track_system(ctx);
        shellman_ui::app_icon::sync_window_icon(ctx, shellman_ui::app_icon::Kind::Settings);
        self.follow_files(ctx);
        self.follow_requests();
        self.layout(ctx);
    }
}
