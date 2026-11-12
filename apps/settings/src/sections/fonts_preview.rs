//! The preview: a File Explorer window, a message box, and a Settings page drawn in the faces
//! Windows would get, at the sizes Windows uses them. Ported from Fontman's window: the drawing is
//! its own, with colours that follow shellman's light or dark look and rounder corners.

use shellman_ui::theme::colors;
use egui::{
    Align2, Color32, CornerRadius, FontFamily, FontId, Painter, Rect, Sense, Stroke, StrokeKind, Ui,
};

/// The faces the preview draws with. Missing ones fall back to `regular`.
#[derive(Clone, Default)]
pub struct Faces {
    pub light: Option<FontFamily>,
    pub semilight: Option<FontFamily>,
    pub regular: Option<FontFamily>,
    pub semibold: Option<FontFamily>,
    pub bold: Option<FontFamily>,
    pub black: Option<FontFamily>,
    pub italic: Option<FontFamily>,
    /// Segoe UI Variable Display Semibold, for big titles in the Windows 11 shell.
    pub display: Option<FontFamily>,
    /// Segoe UI Variable Text Regular, for the Windows 11 shell's body text.
    pub text: Option<FontFamily>,
    /// Segoe UI Variable Small Regular, for its captions.
    pub small: Option<FontFamily>,
}

impl Faces {
    fn pick(&self, face: &Option<FontFamily>) -> FontFamily {
        face.clone()
            .or_else(|| self.regular.clone())
            .unwrap_or(FontFamily::Proportional)
    }

    fn font(&self, face: &Option<FontFamily>, size: f32) -> FontId {
        FontId::new(size, self.pick(face))
    }

    pub fn is_ready(&self) -> bool {
        self.regular.is_some()
    }
}

/// Windows' own window colours, dark or light as shellman is.
struct Win {
    bg: Color32,
    pane: Color32,
    text: Color32,
    dim: Color32,
    line: Color32,
    accent: Color32,
    card: Color32,
    footer: Color32,
    button: Color32,
    on_accent: Color32,
}

fn win() -> Win {
    if colors().light {
        Win {
            bg: Color32::from_rgb(243, 243, 243),
            pane: Color32::from_rgb(235, 235, 235),
            text: Color32::from_rgb(26, 26, 26),
            dim: Color32::from_rgb(96, 96, 96),
            line: Color32::from_rgb(220, 220, 220),
            accent: Color32::from_rgb(0, 95, 184),
            card: Color32::from_rgb(251, 251, 251),
            footer: Color32::from_rgb(243, 243, 243),
            button: Color32::from_rgb(253, 253, 253),
            on_accent: Color32::WHITE,
        }
    } else {
        Win {
            bg: Color32::from_rgb(32, 32, 32),
            pane: Color32::from_rgb(39, 39, 39),
            text: Color32::from_rgb(255, 255, 255),
            dim: Color32::from_rgb(200, 200, 200),
            line: Color32::from_rgb(58, 58, 58),
            accent: Color32::from_rgb(76, 194, 255),
            card: Color32::from_rgb(43, 43, 43),
            footer: Color32::from_rgb(32, 32, 32),
            button: Color32::from_rgb(55, 55, 55),
            on_accent: Color32::BLACK,
        }
    }
}
/// Windows UI text is 9 points: 12 pixels at 100% scale.
const UI_SIZE: f32 = 12.0;

/// Draw the File Explorer window with a message box over it.
pub fn explorer(ui: &mut Ui, faces: &Faces) {
    let w = win();
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 300.0), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let p = ui.painter().with_clip_rect(rect);
    let regular = faces.font(&faces.regular, UI_SIZE);
    p.rect(
        rect,
        CornerRadius::same(12),
        w.bg,
        Stroke::new(1.0_f32, w.line),
        StrokeKind::Inside,
    );

    // Title bar.
    let title = Rect::from_min_size(rect.min, egui::vec2(width, 32.0));
    p.rect_filled(
        Rect::from_min_size(title.min + egui::vec2(12.0, 9.0), egui::vec2(14.0, 12.0)),
        CornerRadius::same(2),
        Color32::from_rgb(255, 200, 70),
    );
    p.text(
        title.left_center() + egui::vec2(34.0, 0.0),
        Align2::LEFT_CENTER,
        "Documents - File Explorer",
        regular.clone(),
        w.text,
    );
    caption_buttons(&p, title);

    // Command bar.
    let bar = Rect::from_min_size(title.left_bottom(), egui::vec2(width, 30.0));
    let mut x = bar.left() + 14.0;
    for item in ["New", "Cut", "Copy", "Paste", "Rename", "Sort", "View"] {
        let galley = p.layout_no_wrap(item.into(), regular.clone(), w.text);
        p.galley(
            egui::pos2(x, bar.center().y - galley.size().y / 2.0),
            galley.clone(),
            w.text,
        );
        x += galley.size().x + 22.0;
    }
    p.line_segment(
        [bar.left_bottom(), bar.right_bottom()],
        Stroke::new(1.0_f32, w.line),
    );

    // Navigation pane.
    let status_h = 24.0;
    let nav = Rect::from_min_max(
        bar.left_bottom(),
        egui::pos2(
            bar.left() + (width * 0.26).min(170.0),
            rect.bottom() - status_h,
        ),
    );
    p.rect_filled(nav, CornerRadius::ZERO, w.pane);
    let places = [
        "Home",
        "Gallery",
        "Desktop",
        "Downloads",
        "Documents",
        "Pictures",
        "Music",
    ];
    for (i, place) in places.iter().enumerate() {
        let row = Rect::from_min_size(
            nav.min + egui::vec2(6.0, 8.0 + i as f32 * 26.0),
            egui::vec2(nav.width() - 12.0, 24.0),
        );
        if *place == "Documents" {
            p.rect_filled(row, CornerRadius::same(4), w.text.gamma_multiply(0.07));
            p.rect_filled(
                Rect::from_min_size(
                    row.left_center() + egui::vec2(0.0, -7.0),
                    egui::vec2(3.0, 14.0),
                ),
                CornerRadius::same(2),
                w.accent,
            );
        }
        p.text(
            row.left_center() + egui::vec2(14.0, 0.0),
            Align2::LEFT_CENTER,
            *place,
            regular.clone(),
            w.text,
        );
    }

    // Files.
    let list = Rect::from_min_max(
        egui::pos2(nav.right() + 14.0, nav.top() + 8.0),
        egui::pos2(rect.right() - 10.0, nav.bottom()),
    );
    // Narrow previews drop the Type column, as Explorer does when its window is small.
    let wide = list.width() > 520.0;
    let columns: &[f32] = if wide {
        &[0.0, 0.40, 0.66, 0.88, 1.0]
    } else {
        &[0.0, 0.48, 0.84, 1.0]
    };
    let shown: &[usize] = if wide { &[0, 1, 2, 3] } else { &[0, 1, 3] };
    let col_x = |i: usize| list.left() + list.width() * columns[i];
    // Each cell is clipped to its column, so long text never runs into the next one.
    let cell = |p: &Painter, col: usize, y: f32, text: &str, color: Color32| {
        let clip = Rect::from_min_max(
            egui::pos2(col_x(col), y - 10.0),
            egui::pos2(col_x(col + 1) - 8.0, y + 10.0),
        );
        p.with_clip_rect(clip.intersect(p.clip_rect())).text(
            egui::pos2(col_x(col), y),
            Align2::LEFT_CENTER,
            text,
            regular.clone(),
            color,
        );
    };
    let heads = ["Name", "Date modified", "Type", "Size"];
    for (col, &field) in shown.iter().enumerate() {
        cell(&p, col, list.top() + 10.0, heads[field], w.dim);
    }
    let files = [
        (
            "Budget 2026.xlsx",
            "10/18/2026 9:41 AM",
            "Excel Worksheet",
            "24 KB",
        ),
        ("Holiday photos", "10/12/2026 6:02 PM", "File folder", ""),
        (
            "Lease agreement.pdf",
            "9/30/2026 11:15 AM",
            "PDF Document",
            "1,204 KB",
        ),
        ("Notes.txt", "9/28/2026 8:20 PM", "Text Document", "2 KB"),
        (
            "Résumé – Final.docx",
            "9/02/2026 3:47 PM",
            "Word Document",
            "88 KB",
        ),
    ];
    for (row, file) in files.iter().enumerate() {
        let y = list.top() + 36.0 + row as f32 * 26.0;
        if row == 2 {
            p.rect_filled(
                Rect::from_min_max(
                    egui::pos2(list.left() - 6.0, y - 12.0),
                    egui::pos2(list.right(), y + 12.0),
                ),
                CornerRadius::same(4),
                Color32::from_rgba_unmultiplied(76, 194, 255, 40),
            );
        }
        let fields = [file.0, file.1, file.2, file.3];
        for (col, &field) in shown.iter().enumerate() {
            let color = if field == 0 { w.text } else { w.dim };
            cell(&p, col, y, fields[field], color);
        }
    }

    // Status bar.
    let status = Rect::from_min_max(egui::pos2(rect.left(), rect.bottom() - status_h), rect.max);
    p.text(
        status.left_center() + egui::vec2(14.0, 0.0),
        Align2::LEFT_CENTER,
        "5 items    1 item selected  1.17 MB",
        regular.clone(),
        w.dim,
    );

    message_box(&p, faces, rect);
}

fn caption_buttons(p: &Painter, title: Rect) {
    let w = win();
    let stroke = Stroke::new(1.0_f32, w.text);
    let y = title.center().y;
    let right = title.right();
    // Close, maximize, minimize, right to left, 46 pixels apart as Windows draws them.
    let close = egui::pos2(right - 23.0, y);
    p.line_segment(
        [close + egui::vec2(-5.0, -5.0), close + egui::vec2(5.0, 5.0)],
        stroke,
    );
    p.line_segment(
        [close + egui::vec2(-5.0, 5.0), close + egui::vec2(5.0, -5.0)],
        stroke,
    );
    let max = egui::pos2(right - 69.0, y);
    p.rect_stroke(
        Rect::from_center_size(max, egui::vec2(10.0, 10.0)),
        CornerRadius::same(1),
        stroke,
        StrokeKind::Inside,
    );
    let min = egui::pos2(right - 115.0, y);
    p.line_segment(
        [min + egui::vec2(-5.0, 0.0), min + egui::vec2(5.0, 0.0)],
        stroke,
    );
}

fn message_box(p: &Painter, faces: &Faces, window: Rect) {
    let w = win();
    let size = egui::vec2(290.0_f32.min(window.width() * 0.5), 140.0);
    let rect = Rect::from_min_size(
        egui::pos2(
            window.right() - size.x - 14.0,
            window.bottom() - size.y - 30.0,
        ),
        size,
    );
    p.rect_filled(
        rect.translate(egui::vec2(0.0, 6.0)).expand(4.0),
        CornerRadius::same(10),
        Color32::from_black_alpha(70),
    );
    p.rect(
        rect,
        CornerRadius::same(8),
        w.card,
        Stroke::new(1.0_f32, w.line),
        StrokeKind::Inside,
    );
    let regular = faces.font(&faces.regular, UI_SIZE);
    p.text(
        rect.min + egui::vec2(12.0, 14.0),
        Align2::LEFT_CENTER,
        "Notepad",
        regular.clone(),
        w.text,
    );
    let instruction = p.layout(
        "Do you want to save changes to Untitled?".into(),
        faces.font(&faces.regular, 16.0),
        w.accent,
        rect.width() - 28.0,
    );
    p.galley(rect.min + egui::vec2(14.0, 36.0), instruction, w.accent);
    let footer = Rect::from_min_max(egui::pos2(rect.left(), rect.bottom() - 44.0), rect.max);
    p.rect_filled(
        footer,
        CornerRadius {
            nw: 0,
            ne: 0,
            sw: 8,
            se: 8,
        },
        w.footer,
    );
    let mut x = footer.right() - 10.0;
    for (label, primary) in [("Cancel", false), ("Don't save", false), ("Save", true)] {
        let galley = p.layout_no_wrap(label.into(), regular.clone(), w.text);
        let bw = (galley.size().x + 24.0).max(72.0);
        let button = Rect::from_min_size(
            egui::pos2(x - bw, footer.center().y - 14.0),
            egui::vec2(bw, 28.0),
        );
        p.rect(
            button,
            CornerRadius::same(4),
            if primary {
                w.accent
            } else {
                w.button
            },
            Stroke::new(1.0_f32, Color32::from_white_alpha(20)),
            StrokeKind::Inside,
        );
        let color = if primary { w.on_accent } else { w.text };
        p.text(
            button.center(),
            Align2::CENTER_CENTER,
            label,
            regular.clone(),
            color,
        );
        x -= bw + 8.0;
    }
}

/// A Windows 11 Settings page, in the Segoe UI Variable faces.
pub fn settings_page(ui: &mut Ui, faces: &Faces) {
    let w = win();
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 196.0), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let p = ui.painter().with_clip_rect(rect);
    p.rect(
        rect,
        CornerRadius::same(12),
        w.bg,
        Stroke::new(1.0_f32, w.line),
        StrokeKind::Inside,
    );
    let display = faces.pick(&faces.display);
    let text = faces.pick(&faces.text);
    let small = faces.pick(&faces.small);
    p.text(
        rect.min + egui::vec2(20.0, 30.0),
        Align2::LEFT_CENTER,
        "System  ›  Display",
        FontId::new(28.0, display.clone()),
        w.text,
    );
    let rows = [
        ("Brightness", "Adjust the brightness of built-in displays"),
        ("Night light", "Use warmer colors to help block blue light"),
    ];
    for (i, (title, detail)) in rows.iter().enumerate() {
        let row = Rect::from_min_size(
            rect.min + egui::vec2(16.0, 66.0 + i as f32 * 62.0),
            egui::vec2(width - 32.0, 56.0),
        );
        p.rect_filled(row, CornerRadius::same(6), w.card);
        p.text(
            row.left_center() + egui::vec2(16.0, -9.0),
            Align2::LEFT_CENTER,
            *title,
            FontId::new(14.0, text.clone()),
            w.text,
        );
        p.text(
            row.left_center() + egui::vec2(16.0, 10.0),
            Align2::LEFT_CENTER,
            *detail,
            FontId::new(12.0, small.clone()),
            w.dim,
        );
        let toggle = Rect::from_center_size(
            egui::pos2(row.right() - 40.0, row.center().y),
            egui::vec2(40.0, 20.0),
        );
        let on = i == 1;
        p.rect(
            toggle,
            CornerRadius::same(10),
            if on { w.accent } else { Color32::TRANSPARENT },
            Stroke::new(1.0_f32, if on { w.accent } else { w.dim }),
            StrokeKind::Inside,
        );
        let knob_x = if on {
            toggle.right() - 10.0
        } else {
            toggle.left() + 10.0
        };
        p.circle_filled(
            egui::pos2(knob_x, toggle.center().y),
            6.0,
            if on { w.on_accent } else { w.dim },
        );
        p.text(
            egui::pos2(toggle.left() - 10.0, row.center().y),
            Align2::RIGHT_CENTER,
            if on { "On" } else { "Off" },
            FontId::new(14.0, text.clone()),
            w.text,
        );
    }
}

/// Every weight, one line each, as text in the window's own colors.
pub fn weights(ui: &mut Ui, faces: &Faces) {
    let c = colors();
    let lines: [(&str, &Option<FontFamily>); 7] = [
        ("Light", &faces.light),
        ("Semilight", &faces.semilight),
        ("Regular", &faces.regular),
        ("Semibold", &faces.semibold),
        ("Bold", &faces.bold),
        ("Black", &faces.black),
        ("Italic", &faces.italic),
    ];
    for (label, face) in lines {
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(78.0, 22.0), Sense::hover());
            ui.painter().text(
                rect.left_center(),
                Align2::LEFT_CENTER,
                label,
                FontId::proportional(12.0),
                c.muted,
            );
            ui.add(
                egui::Label::new(
                    egui::RichText::new("The quick brown fox jumps over the lazy dog 0123")
                        .font(faces.font(face, 16.0))
                        .color(c.text),
                )
                .truncate(),
            );
        });
    }
}
