//! The details under the selected row: what Properties would say, read when it opens (who runs
//! it, elevated or not, its architecture) and from the sample (threads, handles, parent).

use crate::app::State;
use egui::{pos2, vec2, Align2, FontId, Rect};
use shellman_ui::glass::islands::shows_as;
use shellman_ui::theme::{self, colors};

pub const DETAILS_H: f32 = 132.0;

pub fn show(ui: &mut egui::Ui, rect: Rect, proc: usize, st: &mut State) {
    let c = colors();
    let key = st.list.procs[proc].key;
    let control = st.control;
    let details = st.details.entry(key).or_insert_with(|| shellman_core::procs::Control::details(&control, key)).clone();
    let p = &st.list.procs[proc];
    let card = rect.shrink2(vec2(6.0, 4.0));
    ui.painter().rect_filled(card, 14.0, shows_as(theme::with_alpha(c.text, if c.light { 10 } else { 8 })));
    let inner = card.shrink2(vec2(16.0, 10.0));
    let unknown = |s: Option<String>| s.unwrap_or_else(|| "Not readable".into());
    let parent = st
        .list
        .procs
        .iter()
        .find(|q| q.key.pid == p.parent && q.key.created <= p.key.created)
        .map(|q| format!("{} ({})", q.name, p.parent))
        .unwrap_or_else(|| format!("{} (ended)", p.parent));
    let path = p.exe.as_ref().map(|e| e.display().to_string()).unwrap_or_else(|| "Not readable".into());
    let command = if p.command_line.is_empty() { "Not readable".to_owned() } else { p.command_line.to_string() };
    let fields: [(&str, String); 8] = [
        ("User", unknown(details.user.clone())),
        ("Started", details.started.clone()),
        ("Architecture", details.architecture.map(str::to_owned).unwrap_or_else(|| "Not readable".into())),
        ("Elevated", details.elevated.map(|e| if e { "Yes" } else { "No" }.to_owned()).unwrap_or_else(|| "Not readable".into())),
        ("Threads", p.threads.to_string()),
        ("Handles", p.handles.to_string()),
        ("Parent", parent),
        ("Session", p.session.to_string()),
    ];
    let label_font = FontId::proportional(11.5);
    let value_font = FontId::proportional(12.5);
    let line = |y: f32, label: &str, value: &str, x: f32, w: f32| {
        let painter = ui.painter().with_clip_rect(Rect::from_min_size(pos2(x, y), vec2(w - 12.0, 36.0)));
        painter.text(pos2(x, y), Align2::LEFT_TOP, label, label_font.clone(), c.muted);
        painter.text(pos2(x, y + 15.0), Align2::LEFT_TOP, value, value_font.clone(), c.text);
    };
    let half = inner.width() / 2.0;
    line(inner.top(), "Path", &path, inner.left(), half);
    line(inner.top(), "Command line", &command, inner.left() + half, half);
    let col = inner.width() / 4.0;
    for (i, (label, value)) in fields.iter().enumerate() {
        let x = inner.left() + (i % 4) as f32 * col;
        let y = inner.top() + 38.0 + (i / 4) as f32 * 36.0;
        line(y, label, value, x, col);
    }
    // Read out as one piece; what could not be read says why when hovered.
    let resp = ui.interact(card, egui::Id::new(("denied", key)), egui::Sense::hover());
    let spoken = std::iter::once(("Path", path.as_str()))
        .chain(std::iter::once(("Command line", command.as_str())))
        .chain(fields.iter().map(|(l, v)| (*l, v.as_str())))
        .map(|(l, v)| format!("{l}: {v}"))
        .collect::<Vec<_>>()
        .join(", ");
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, format!("Details. {spoken}")));
    if let Some(why) = &details.denied {
        resp.on_hover_text(why);
    }
}
