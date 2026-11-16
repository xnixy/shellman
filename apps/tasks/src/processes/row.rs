//! One line of the list: a section heading, a group's row, a process inside an open group, or
//! the details under the selection.

use super::details;
use crate::app::State;
use crate::model::Line;
use egui::{pos2, vec2, Align2, Color32, FontId, Rect, Sense};
use shellman_core::procs::policy::{self, Verdict};
use shellman_core::procs::sort::Column;
use shellman_core::procs::{format, Key, Proc, Status};
use shellman_ui::glass::islands::shows_as;
use shellman_ui::theme::{self, colors, glyph, icon_font, semibold_font};

pub const ROW_H: f32 = 30.0;
const ICON: f32 = 18.0;
/// How far a process inside a group is set in.
const INDENT: f32 = 24.0;
const SHIELD: char = '\u{EA18}';
const CHEVRON_DOWN: char = '\u{E70D}';

/// A numeric column: its header and width, laid out from the right.
pub struct Col {
    pub column: Column,
    pub title: &'static str,
    pub width: f32,
}

pub const COLUMNS: [Col; 6] = [
    Col { column: Column::Cpu, title: "CPU", width: 64.0 },
    Col { column: Column::Memory, title: "Memory", width: 100.0 },
    Col { column: Column::Disk, title: "Disk", width: 80.0 },
    Col { column: Column::Gpu, title: "GPU", width: 60.0 },
    Col { column: Column::Pid, title: "PID", width: 70.0 },
    Col { column: Column::Status, title: "Status", width: 118.0 },
];

/// Where `column`'s cell is in a row spanning `rect`'s width.
pub fn cell(rect: Rect, column: Column) -> Rect {
    let mut right = rect.right();
    for c in COLUMNS.iter().rev() {
        let left = right - c.width;
        if c.column == column {
            return Rect::from_x_y_ranges(left..=right, rect.y_range());
        }
        right = left;
    }
    name_cell(rect)
}

/// The name's cell: everything left of the numbers.
pub fn name_cell(rect: Rect) -> Rect {
    let numbers: f32 = COLUMNS.iter().map(|c| c.width).sum();
    Rect::from_x_y_ranges(rect.left()..=(rect.right() - numbers), rect.y_range())
}

/// What a click on a row asks for; `bool`: the row is inside a group.
pub enum Press {
    Select(Key, bool),
    Toggle(Key),
    Details(Key, bool),
}

/// The numbers of a row, a group's added up.
struct Numbers {
    cpu: f32,
    memory: u64,
    disk: f64,
    gpu: Option<f32>,
    pid: u32,
    status: Status,
}

pub fn show(ui: &mut egui::Ui, rect: Rect, line: Line, st: &mut State) -> Option<Press> {
    match line {
        Line::Heading { apps, count } => {
            heading(ui, rect, apps, count);
            None
        }
        Line::Details { proc } => {
            details::show(ui, rect, proc, st);
            None
        }
        Line::Group { group, open } => {
            let g = &st.list.groups[group];
            let root = g.root;
            let t = st.list.totals[group];
            let n = Numbers {
                cpu: t.cpu,
                memory: t.memory,
                disk: t.disk,
                gpu: t.gpu,
                pid: st.list.procs[root].key.pid,
                status: st.list.group_status(group),
            };
            let label = st.list.group_label(group).to_owned();
            let members = g.members.len();
            let extra = if members > 0 {
                format!("({})", members + 1)
            } else if g.app && *st.list.procs[root].description != label {
                st.list.procs[root].description.to_string()
            } else {
                String::new()
            };
            let open = (members > 0).then_some(open);
            draw(ui, rect, root, 0.0, open, &label, &extra, &n, st)
        }
        Line::Member { proc, .. } => {
            let p = &st.list.procs[proc];
            let n = Numbers { cpu: p.cpu, memory: p.memory, disk: p.disk, gpu: p.gpu, pid: p.key.pid, status: p.status() };
            let label = p.title().map(str::to_owned).unwrap_or_else(|| p.description.to_string());
            let extra = match p.title() {
                Some(_) => p.description.to_string(),
                None => helper_kind(&p.command_line).unwrap_or_default().to_owned(),
            };
            draw(ui, rect, proc, INDENT, None, &label, &extra, &n, st)
        }
    }
}

fn heading(ui: &mut egui::Ui, rect: Rect, apps: bool, count: usize) {
    let c = colors();
    let title = if apps { "Apps" } else { "Background processes" };
    let resp = ui.interact(rect, egui::Id::new(("heading", apps)), Sense::hover());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, format!("{title} ({count})")));
    let y = rect.center().y + 3.0;
    let t = ui.painter().text(pos2(rect.left() + 8.0, y), Align2::LEFT_CENTER, title, semibold_font(13.0), c.text);
    ui.painter().text(pos2(t.right() + 6.0, y), Align2::LEFT_CENTER, format!("({count})"), FontId::proportional(12.5), c.muted);
}

#[allow(clippy::too_many_arguments)]
fn draw(
    ui: &mut egui::Ui,
    rect: Rect,
    proc: usize,
    indent: f32,
    open: Option<bool>,
    label: &str,
    extra: &str,
    n: &Numbers,
    st: &mut State,
) -> Option<Press> {
    let c = colors();
    let p: &Proc = &st.list.procs[proc];
    let key = p.key;
    let id = egui::Id::new(("row", key, indent as i32));
    // A group's root has two rows (the group's, and its own inside); only the one clicked lights.
    let member_row = indent > 0.0;
    let selected = st.list.selected == Some(key) && (st.list.as_member == member_row || !is_group_row(st, proc));
    // One Tab stop for the whole list: the selected row (the first row when none is), and the
    // arrow keys move it (see `list::keys`).
    let tab_stop = selected || (st.list.selected.is_none() && st.list.tab_stop == Some(key) && !member_row);
    let resp = ui.interact(rect, id, if tab_stop { Sense::click() } else { Sense::CLICK });
    let verdict = policy::may_end(p, st.own_pid);
    let name = row_name(label, extra, n, open, verdict.reason().is_some());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, &name));
    if tab_stop {
        if resp.has_focus() {
            // The arrows move the selection, not egui's focus.
            ui.memory_mut(|m| m.set_focus_lock_filter(id, egui::EventFilter { horizontal_arrows: true, vertical_arrows: true, ..Default::default() }));
            st.list.row_focus = true;
        } else if selected && (st.list.take_focus || st.list.focus_pending) {
            resp.request_focus();
            st.list.focus_pending = false;
        }
    }
    let row_rect = rect.shrink2(vec2(0.0, 1.0));
    if c.high_contrast {
        // Highlight only under highlight text: the selected row is outlined instead.
        if selected {
            ui.painter().rect_stroke(row_rect, 10.0, egui::Stroke::new(2.0_f32, c.accent_fill), egui::StrokeKind::Inside);
        }
    } else {
        // Given as they should show over the glass (see `glass::islands::shows_as`).
        let fill = if selected {
            shows_as(theme::with_alpha(c.accent_fill, if c.light { 40 } else { 52 }))
        } else if resp.hovered() {
            shows_as(c.row_hover)
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(row_rect, 10.0, fill);
    }
    theme::focus_ring(ui, &resp, row_rect, 10.0);
    // Tab into the list selects its first row.
    let tabbed_in = resp.gained_focus() && !selected;

    let mut x = rect.left() + 4.0 + indent;
    let mut press = None;
    if let Some(open) = open {
        let chevron = Rect::from_min_size(pos2(x, rect.top()), vec2(22.0, rect.height()));
        // Not a Tab stop (Right and Left open and close a group), but named for screen readers.
        let cr = ui.interact(chevron, id.with("chevron"), Sense::CLICK);
        let what = if open { "Collapse" } else { "Expand" };
        cr.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("{what} {label}")));
        let g = if open { CHEVRON_DOWN } else { glyph::CHEVRON };
        let color = if cr.hovered() { c.text } else { c.muted };
        ui.painter().text(chevron.center(), Align2::CENTER_CENTER, g, icon_font(10.0), color);
        if cr.clicked() {
            press = Some(Press::Toggle(key));
        }
    }
    x += 24.0;
    let icon = Rect::from_center_size(pos2(x + ICON / 2.0, rect.center().y), vec2(ICON, ICON));
    let image = p.exe.as_ref().and_then(|exe| shellman_ui::app_icon::get(ui.ctx(), exe, false, &mut st.thumbs));
    match image {
        Some((tex, _)) => {
            ui.painter().image(tex, icon, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        None => {
            ui.painter().text(icon.center(), Align2::CENTER_CENTER, glyph::APP, icon_font(14.0), c.muted);
        }
    }
    x = icon.right() + 10.0;

    // The name, a muted note after it, and a mark when it cannot (or should not) be ended.
    let name = Rect::from_x_y_ranges(x..=name_cell(rect).right() - 8.0, rect.y_range());
    let painter = ui.painter().with_clip_rect(name);
    let font = FontId::proportional(13.0);
    let shown = elide(ui, label, &font, name.width());
    let t = painter.text(pos2(name.left(), rect.center().y), Align2::LEFT_CENTER, &shown, font, c.text);
    let mut after = t.right() + 8.0;
    if !extra.is_empty() && after < name.right() - 20.0 {
        let e = painter.text(pos2(after, rect.center().y), Align2::LEFT_CENTER, extra, FontId::proportional(12.0), c.muted);
        after = e.right() + 8.0;
    }
    let mark = match verdict {
        Verdict::Critical(_) | Verdict::Denied(_) => Some(glyph::LOCK),
        Verdict::Protected(_) => Some(SHIELD),
        Verdict::Allowed => None,
    };
    if let Some(g) = mark {
        let at = Rect::from_center_size(pos2(after + 6.0, rect.center().y), vec2(14.0, 14.0));
        if at.right() < name.right() {
            painter.text(at.center(), Align2::CENTER_CENTER, g, icon_font(11.0), c.muted);
            if let Some(reason) = verdict.reason() {
                let m = ui.interact(at, id.with("mark"), Sense::hover());
                m.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, reason));
                m.on_hover_text(reason);
            }
        }
    }

    numbers(ui, rect, n);
    if press.is_none() {
        if resp.double_clicked() {
            press = Some(if open.is_some() { Press::Toggle(key) } else { Press::Details(key, member_row) });
        } else if resp.clicked() || tabbed_in {
            press = Some(Press::Select(key, member_row));
        }
    }
    if let Some(reason) = verdict.reason() {
        let _ = resp.on_hover_text_at_pointer(reason);
    }
    press
}

/// True when `proc` heads a group of several (its row stands for them all).
fn is_group_row(st: &State, proc: usize) -> bool {
    st.list.groups.iter().any(|g| g.root == proc && !g.members.is_empty())
}

/// The right-hand numbers. Zeros are muted, so what is busy stands out.
fn numbers(ui: &egui::Ui, rect: Rect, n: &Numbers) {
    let c = colors();
    let font = FontId::proportional(12.5);
    let tone = |busy: bool| if busy { c.text } else { theme::with_alpha(c.muted, 200) };
    let right = |column: Column, text: String, busy: bool| {
        let cell = cell(rect, column);
        ui.painter().text(pos2(cell.right() - 8.0, rect.center().y), Align2::RIGHT_CENTER, text, font.clone(), tone(busy));
    };
    right(Column::Cpu, format::percent(n.cpu), n.cpu >= 0.05);
    right(Column::Memory, format::memory_mb(n.memory), true);
    right(Column::Disk, format::rate(n.disk), n.disk >= 50_000.0);
    match n.gpu {
        Some(g) => right(Column::Gpu, format::percent(g), g >= 0.05),
        None => right(Column::Gpu, "—".into(), false),
    }
    right(Column::Pid, n.pid.to_string(), false);
    let (text, color) = match n.status {
        Status::Running => ("", c.muted),
        Status::Suspended => ("Suspended", c.muted),
        Status::NotResponding => ("Not responding", c.warn),
    };
    let cell = cell(rect, Column::Status);
    ui.painter().text(pos2(cell.left() + 8.0, rect.center().y), Align2::LEFT_CENTER, text, font, color);
}

/// What a screen reader says for a row: its name and note, every column, whether its group is
/// open, and that it cannot be ended (the reason is on the lock beside it).
fn row_name(label: &str, extra: &str, n: &Numbers, open: Option<bool>, locked: bool) -> String {
    let mut s = label.to_owned();
    if !extra.is_empty() {
        s += &format!(" {extra}");
    }
    let gpu = n.gpu.map(format::percent).unwrap_or_else(|| "none".into());
    s += &format!(
        ", CPU {}, Memory {}, Disk {}, GPU {gpu}, PID {}",
        format::percent(n.cpu),
        format::memory_mb(n.memory),
        format::rate(n.disk),
        n.pid
    );
    match n.status {
        Status::Running => {}
        Status::Suspended => s += ", Suspended",
        Status::NotResponding => s += ", Not responding",
    }
    match open {
        Some(true) => s += ", expanded",
        Some(false) => s += ", collapsed",
        None => {}
    }
    if locked {
        s += ", cannot be ended";
    }
    s
}

/// What a browser-style helper is for, from its command line: `--type=renderer` is "renderer",
/// `-contentproc ... tab` is "tab". Tells the many rows of one app apart.
fn helper_kind(command_line: &str) -> Option<&str> {
    if let Some(at) = command_line.find("--type=") {
        let rest = &command_line[at + 7..];
        let end = rest.find(|c: char| c.is_whitespace() || c == '"').unwrap_or(rest.len());
        let kind = &rest[..end];
        // A utility says what it serves: --utility-sub-type=network.mojom.NetworkService.
        if kind == "utility" {
            if let Some(sub) = command_line.find("--utility-sub-type=") {
                let s = &command_line[sub + 19..];
                let s = &s[..s.find(|c: char| c.is_whitespace() || c == '"').unwrap_or(s.len())];
                return s.split('.').next().filter(|s| !s.is_empty()).or(Some(kind));
            }
        }
        return (!kind.is_empty()).then_some(kind);
    }
    if command_line.contains("-contentproc") {
        return command_line.split_whitespace().last().filter(|w| w.chars().all(|c| c.is_ascii_alphabetic()));
    }
    None
}

/// `text` cut to fit `width`, with an ellipsis when it was cut.
fn elide(ui: &egui::Ui, text: &str, font: &FontId, width: f32) -> String {
    let fits = |s: &str| ui.fonts(|f| f.layout_no_wrap(s.to_owned(), font.clone(), Color32::WHITE).size().x) <= width;
    if fits(text) {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    let (mut lo, mut hi) = (0usize, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let s: String = chars[..mid].iter().collect::<String>() + "…";
        if fits(&s) {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    chars[..lo].iter().collect::<String>() + "…"
}

#[cfg(test)]
mod tests {
    use super::{helper_kind, row_name, Numbers};
    use shellman_core::procs::Status;

    #[test]
    fn rows_say_every_column() {
        let n = Numbers { cpu: 3.0, memory: 512 * 1024 * 1024, disk: 0.0, gpu: None, pid: 42, status: Status::NotResponding };
        let s = row_name("Firefox", "(5)", &n, Some(false), true);
        assert!(s.starts_with("Firefox (5), CPU "), "{s}");
        assert!(s.contains("PID 42") && s.contains("GPU none") && s.contains("Memory "), "{s}");
        assert!(s.ends_with("Not responding, collapsed, cannot be ended"), "{s}");
        let plain = row_name("x", "", &Numbers { status: Status::Running, ..n }, None, false);
        assert!(plain.starts_with("x, CPU ") && plain.ends_with("PID 42"), "{plain}");
    }

    #[test]
    fn helpers_say_what_they_are_for() {
        assert_eq!(helper_kind(r#""C:\x\helium.exe" --type=renderer --lang=en"#), Some("renderer"));
        assert_eq!(helper_kind("x.exe --type=gpu-process"), Some("gpu-process"));
        assert_eq!(helper_kind("x.exe --type=utility --utility-sub-type=network.mojom.NetworkService"), Some("network"));
        assert_eq!(helper_kind(r#""firefox.exe" -contentproc -childID 3 -isForBrowser 12 tab"#), Some("tab"));
        assert_eq!(helper_kind("notepad.exe"), None);
    }
}
