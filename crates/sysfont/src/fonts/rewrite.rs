//! Making a replacement face: a copy of the chosen font that carries the names, weight, and style
//! of the Segoe UI face it stands in for.
//!
//! A static font is copied table by table, with new names and patched style fields, so its
//! outlines and hinting are exactly what the designer shipped. A variable font, or a face that is
//! slanted to stand in for a missing italic, is redrawn at the one position it is used at and
//! written as a plain static TrueType font, which every part of Windows reads the same way.

use super::names::{self, NameSet};
use kurbo::{Affine, BezPath, PathEl, Point};
use read_fonts::{FontRef, TableProvider};
use serde::{Deserialize, Serialize};
use skrifa::instance::{Location, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{GlyphId, MetadataProvider, Tag};
use write_fonts::tables::glyf::{GlyfLocaBuilder, Glyph, SimpleGlyph};
use write_fonts::FontBuilder;

/// How tall a face sits in its line box. Copied from the Segoe UI face being replaced when
/// line heights are matched, so text keeps its place in buttons and rows sized for Segoe UI.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VMetrics {
    pub upem: u16,
    pub hhea_ascender: i16,
    pub hhea_descender: i16,
    pub hhea_line_gap: i16,
    pub typo_ascender: i16,
    pub typo_descender: i16,
    pub typo_line_gap: i16,
    pub win_ascent: u16,
    pub win_descent: u16,
    pub use_typo_metrics: bool,
}

impl VMetrics {
    pub fn from_font(font: &FontRef) -> Option<Self> {
        let head = font.head().ok()?;
        let hhea = font.hhea().ok()?;
        let os2 = font.os2().ok()?;
        Some(Self {
            upem: head.units_per_em(),
            hhea_ascender: hhea.ascender().to_i16(),
            hhea_descender: hhea.descender().to_i16(),
            hhea_line_gap: hhea.line_gap().to_i16(),
            typo_ascender: os2.s_typo_ascender(),
            typo_descender: os2.s_typo_descender(),
            typo_line_gap: os2.s_typo_line_gap(),
            win_ascent: os2.us_win_ascent(),
            win_descent: os2.us_win_descent(),
            use_typo_metrics: os2.fs_selection().bits() & 0x80 != 0,
        })
    }

    /// The same proportions in a font with `upem` units per em.
    fn scaled_to(&self, upem: u16) -> Self {
        let k = upem as f32 / self.upem.max(1) as f32;
        let i = |v: i16| (v as f32 * k).round() as i16;
        let u = |v: u16| (v as f32 * k).round() as u16;
        Self {
            upem,
            hhea_ascender: i(self.hhea_ascender),
            hhea_descender: i(self.hhea_descender),
            hhea_line_gap: i(self.hhea_line_gap),
            typo_ascender: i(self.typo_ascender),
            typo_descender: i(self.typo_descender),
            typo_line_gap: i(self.typo_line_gap),
            win_ascent: u(self.win_ascent),
            win_descent: u(self.win_descent),
            use_typo_metrics: self.use_typo_metrics,
        }
    }
}

/// Everything about the Segoe UI face that the replacement must copy.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Target {
    pub names: NameSet,
    pub weight: u16,
    pub width: u16,
    pub italic: bool,
    /// fsSelection of the original: the style bits are copied from it.
    pub fs_selection: u16,
    pub mac_style: u16,
    pub panose: [u8; 10],
    pub metrics: VMetrics,
    /// The optical size the original was drawn for, when it is a Segoe UI Variable instance.
    pub opsz: Option<f32>,
}

/// Where in the source font to take the outlines from.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Position {
    /// Axis settings in user units, like ("wght", 600.0). Empty for a static face.
    pub axes: Vec<(String, f32)>,
    /// Slant an upright face to stand in for an italic the family does not have.
    pub slant: bool,
}

/// Adjustments the user asked for.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tweaks {
    /// Give the face Segoe UI's line height and baseline.
    pub match_metrics: bool,
    /// Glyph size relative to the font's own, 1.0 for unchanged.
    pub scale: f32,
}

impl Default for Tweaks {
    fn default() -> Self {
        Self {
            match_metrics: true,
            scale: 1.0,
        }
    }
}

/// The slant given to an upright face that stands in for an italic, about 11 degrees.
const SLANT: f64 = 0.19;

/// Tables that describe variation, hinting, or device metrics. A redrawn face has none of these,
/// and a copied face whose em is rescaled loses the device metrics, which were measured per size.
const VARIATION_TABLES: &[&[u8; 4]] = &[
    b"fvar", b"gvar", b"avar", b"HVAR", b"VVAR", b"MVAR", b"STAT", b"cvar", b"CFF2",
];
const HINTING_TABLES: &[&[u8; 4]] = &[b"cvt ", b"fpgm", b"prep"];
const DEVICE_TABLES: &[&[u8; 4]] = &[b"hdmx", b"LTSH", b"VDMX"];
/// Never carried over: a signature that no longer matches, and vertical metrics for CJK layout
/// that are not updated.
const ALWAYS_DROPPED: &[&[u8; 4]] = &[b"DSIG", b"vhea", b"vmtx", b"VORG"];

/// Whether `source` at `position` has to be redrawn rather than copied.
pub fn needs_redraw(font: &FontRef, position: &Position) -> bool {
    position.slant || !font.axes().is_empty() || font.table_data(Tag::new(b"glyf")).is_none()
}

/// The replacement face, as the bytes of a font file.
pub fn build(
    font: &FontRef,
    position: &Position,
    target: &Target,
    tweaks: Tweaks,
    note: &str,
) -> Result<Vec<u8>, String> {
    let bytes = if needs_redraw(font, position) {
        redraw(font, position, target, tweaks, note)?
    } else {
        copy(font, target, tweaks, note)?
    };
    // Read it back, so a file Windows would choke on never leaves here.
    let check =
        FontRef::new(&bytes).map_err(|err| format!("The new face does not parse: {err}"))?;
    check
        .head()
        .and(check.maxp())
        .map_err(|err| format!("The new face is incomplete: {err}"))?;
    Ok(bytes)
}

fn copy(font: &FontRef, target: &Target, tweaks: Tweaks, note: &str) -> Result<Vec<u8>, String> {
    let head = font.head().map_err(|err| err.to_string())?;
    let source_upem = head.units_per_em();
    // A bigger glyph is the same outline in a smaller em. Every metric and kerning value stays
    // in proportion, and the hinting still works on the same outline.
    let upem = scaled_upem(source_upem, tweaks.scale);
    let rescaled = upem != source_upem;
    let own = VMetrics::from_font(font).ok_or("The font has no line metrics.")?;
    let metrics = if tweaks.match_metrics {
        target.metrics.scaled_to(upem)
    } else {
        own
    };

    let mut builder = FontBuilder::new();
    for record in font.table_directory().table_records() {
        let tag = record.tag();
        let raw = tag.to_be_bytes();
        if ALWAYS_DROPPED.contains(&&raw) || &raw == b"name" || &raw == b"STAT" {
            continue;
        }
        if rescaled && DEVICE_TABLES.contains(&&raw) {
            continue;
        }
        let Some(data) = font.table_data(tag) else {
            continue;
        };
        let mut data = data.as_bytes().to_vec();
        match &raw {
            b"head" => patch_head(&mut data, target, Some(upem), None),
            b"hhea" => patch_hhea_metrics(&mut data, &metrics),
            b"OS/2" => {
                let avg = None;
                patch_os2(&mut data, target, &metrics, tweaks.match_metrics, avg, 1.0)
            }
            _ => {}
        }
        builder.add_raw(tag, data);
    }
    builder.add_raw(
        Tag::new(b"name"),
        names::build_table(font, &target.names, note),
    );
    Ok(builder.build())
}

fn scaled_upem(upem: u16, scale: f32) -> u16 {
    if (scale - 1.0).abs() < 0.005 {
        return upem;
    }
    ((upem as f32 / scale).round() as u32).clamp(16, 16384) as u16
}

/// Collects a glyph outline as a path in font units, transformed on the way in.
struct PathPen {
    path: BezPath,
    transform: Affine,
}

impl PathPen {
    fn point(&self, x: f32, y: f32) -> Point {
        self.transform * Point::new(x as f64, y as f64)
    }
}

impl OutlinePen for PathPen {
    fn move_to(&mut self, x: f32, y: f32) {
        let p = self.point(x, y);
        self.path.move_to(p);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.point(x, y);
        self.path.line_to(p);
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        let (c, p) = (self.point(cx0, cy0), self.point(x, y));
        self.path.quad_to(c, p);
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let (c0, c1, p) = (self.point(cx0, cy0), self.point(cx1, cy1), self.point(x, y));
        // TrueType outlines are quadratic. Half a unit is far below what any size can show.
        let start = self.path.elements().last().and_then(PathEl::end_point);
        let Some(start) = start else {
            self.path.move_to(p);
            return;
        };
        let cubic = kurbo::CubicBez::new(start, c0, c1, p);
        for (_, _, quad) in cubic.to_quads(0.5) {
            self.path.quad_to(quad.p1, quad.p2);
        }
    }

    fn close(&mut self) {
        self.path.close_path();
    }
}

/// Drop contours that are a lone point or empty, which a glyf glyph cannot hold.
fn clean(path: &BezPath) -> BezPath {
    let mut out = BezPath::new();
    let mut contour: Vec<PathEl> = Vec::new();
    let flush = |contour: &mut Vec<PathEl>, out: &mut BezPath| {
        let drawn = contour
            .iter()
            .filter(|el| matches!(el, PathEl::LineTo(_) | PathEl::QuadTo(..)))
            .count();
        if drawn >= 2 {
            for el in contour.iter() {
                out.push(*el);
            }
        }
        contour.clear();
    };
    for el in path.elements() {
        if matches!(el, PathEl::MoveTo(_)) {
            flush(&mut contour, &mut out);
        }
        contour.push(*el);
    }
    flush(&mut contour, &mut out);
    out
}

fn location(font: &FontRef, position: &Position) -> Location {
    let settings: Vec<(Tag, f32)> = position
        .axes
        .iter()
        .filter_map(|(tag, value)| {
            Tag::new_checked(tag.as_bytes())
                .ok()
                .map(|tag| (tag, *value))
        })
        .collect();
    font.axes().location(settings)
}

fn redraw(
    font: &FontRef,
    position: &Position,
    target: &Target,
    tweaks: Tweaks,
    note: &str,
) -> Result<Vec<u8>, String> {
    let location = location(font, position);
    let glyph_count = font.maxp().map_err(|err| err.to_string())?.num_glyphs();
    let upem = font.head().map_err(|err| err.to_string())?.units_per_em();
    let scale = if (tweaks.scale - 1.0).abs() < 0.005 {
        1.0
    } else {
        tweaks.scale as f64
    };
    let mut transform = Affine::scale(scale);
    if position.slant {
        transform = Affine::new([1.0, 0.0, SLANT, 1.0, 0.0, 0.0]) * transform;
    }
    let outlines = font.outline_glyphs();
    let glyph_metrics = skrifa::metrics::GlyphMetrics::new(font, Size::unscaled(), &location);
    let line_metrics = skrifa::metrics::Metrics::new(font, Size::unscaled(), &location);

    let mut builder = GlyfLocaBuilder::new();
    let mut advances = Vec::with_capacity(glyph_count as usize);
    let mut bounds = Bounds::default();
    let (mut max_points, mut max_contours) = (0u16, 0u16);
    let mut dropped = 0usize;
    for gid in 0..glyph_count {
        let id = GlyphId::new(gid as u32);
        let advance = glyph_metrics.advance_width(id).unwrap_or(0.0) as f64 * scale;
        let mut pen = PathPen {
            path: BezPath::new(),
            transform,
        };
        if let Some(outline) = outlines.get(id) {
            if outline
                .draw(
                    DrawSettings::unhinted(Size::unscaled(), &location),
                    &mut pen,
                )
                .is_err()
            {
                dropped += 1;
            }
        }
        let path = clean(&pen.path);
        let glyph = if path.elements().is_empty() {
            Glyph::Empty
        } else {
            match SimpleGlyph::from_bezpath(&path) {
                Ok(mut simple) => {
                    simple.overlaps = true;
                    let points: usize = simple.contours.iter().map(|c| c.len()).sum();
                    max_points = max_points.max(points.min(u16::MAX as usize) as u16);
                    max_contours =
                        max_contours.max(simple.contours.len().min(u16::MAX as usize) as u16);
                    Glyph::Simple(simple)
                }
                Err(_) => {
                    dropped += 1;
                    Glyph::Empty
                }
            }
        };
        let bbox = match &glyph {
            Glyph::Simple(simple) => Some(simple.bbox),
            _ => None,
        };
        let advance = advance.round().clamp(0.0, u16::MAX as f64) as u16;
        advances.push((advance, bbox));
        if let Some(bbox) = bbox {
            bounds.add(bbox.x_min, bbox.y_min, bbox.x_max, bbox.y_max, advance);
        }
        builder
            .add_glyph(&glyph)
            .map_err(|err| format!("Glyph {gid} could not be written: {err}"))?;
    }
    // A few broken glyphs in a big font are survivable; a font that mostly fails is not.
    if dropped > glyph_count as usize / 10 + 4 {
        return Err(format!(
            "{dropped} of {glyph_count} glyphs could not be redrawn."
        ));
    }
    let (glyf, loca, loca_format) = builder.build();

    let own = VMetrics {
        upem,
        hhea_ascender: (line_metrics.ascent as f64 * scale).round() as i16,
        hhea_descender: (line_metrics.descent as f64 * scale).round() as i16,
        hhea_line_gap: (line_metrics.leading as f64 * scale).round() as i16,
        ..VMetrics::from_font(font)
            .map(|m| m.scaled_by(scale))
            .ok_or("The font has no line metrics.")?
    };
    let metrics = if tweaks.match_metrics {
        target.metrics.scaled_to(upem)
    } else {
        own
    };

    let mut fb = FontBuilder::new();
    let dropped_tables: Vec<&[u8; 4]> = VARIATION_TABLES
        .iter()
        .chain(HINTING_TABLES)
        .chain(DEVICE_TABLES)
        .chain(ALWAYS_DROPPED)
        .copied()
        .collect();
    for record in font.table_directory().table_records() {
        let tag = record.tag();
        let raw = tag.to_be_bytes();
        if dropped_tables.contains(&&raw)
            || matches!(
                &raw,
                b"glyf" | b"loca" | b"hmtx" | b"maxp" | b"name" | b"CFF " | b"gasp" | b"post"
            )
        {
            continue;
        }
        let Some(data) = font.table_data(tag) else {
            continue;
        };
        let mut data = data.as_bytes().to_vec();
        match &raw {
            b"head" => patch_head(&mut data, target, None, Some((bounds, loca_format as i16))),
            b"hhea" => {
                patch_hhea_metrics(&mut data, &metrics);
                patch_hhea_extents(&mut data, &bounds, glyph_count, position.slant);
            }
            b"OS/2" => {
                let avg = average_width(&advances);
                patch_os2(
                    &mut data,
                    target,
                    &metrics,
                    tweaks.match_metrics,
                    avg,
                    scale,
                );
                // A variable font's x-height and cap height change with weight.
                set_i16(
                    &mut data,
                    86,
                    (line_metrics.x_height.unwrap_or(0.0) as f64 * scale).round() as i16,
                );
                set_i16(
                    &mut data,
                    88,
                    (line_metrics.cap_height.unwrap_or(0.0) as f64 * scale).round() as i16,
                );
            }
            _ => {}
        }
        fb.add_raw(tag, data);
    }
    fb.add_raw(
        Tag::new(b"glyf"),
        write_fonts::dump_table(&glyf).map_err(|e| e.to_string())?,
    );
    fb.add_raw(
        Tag::new(b"loca"),
        write_fonts::dump_table(&loca).map_err(|e| e.to_string())?,
    );
    fb.add_raw(Tag::new(b"hmtx"), hmtx(&advances));
    fb.add_raw(
        Tag::new(b"maxp"),
        maxp(glyph_count, max_points, max_contours),
    );
    fb.add_raw(Tag::new(b"post"), post(font, position.slant));
    // Without hinting, every size is drawn smooth.
    fb.add_raw(Tag::new(b"gasp"), vec![0, 1, 0, 1, 0xFF, 0xFF, 0, 0x0A]);
    fb.add_raw(
        Tag::new(b"name"),
        names::build_table(font, &target.names, note),
    );
    Ok(fb.build())
}

impl VMetrics {
    fn scaled_by(&self, scale: f64) -> Self {
        let i = |v: i16| (v as f64 * scale).round() as i16;
        let u = |v: u16| (v as f64 * scale).round() as u16;
        Self {
            upem: self.upem,
            hhea_ascender: i(self.hhea_ascender),
            hhea_descender: i(self.hhea_descender),
            hhea_line_gap: i(self.hhea_line_gap),
            typo_ascender: i(self.typo_ascender),
            typo_descender: i(self.typo_descender),
            typo_line_gap: i(self.typo_line_gap),
            win_ascent: u(self.win_ascent),
            win_descent: u(self.win_descent),
            use_typo_metrics: self.use_typo_metrics,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Bounds {
    x_min: i16,
    y_min: i16,
    x_max: i16,
    y_max: i16,
    min_lsb: i16,
    min_rsb: i16,
    max_extent: i16,
    max_advance: u16,
    any: bool,
}

impl Default for Bounds {
    fn default() -> Self {
        Self {
            x_min: i16::MAX,
            y_min: i16::MAX,
            x_max: i16::MIN,
            y_max: i16::MIN,
            min_lsb: i16::MAX,
            min_rsb: i16::MAX,
            max_extent: i16::MIN,
            max_advance: 0,
            any: false,
        }
    }
}

impl Bounds {
    fn add(&mut self, x_min: i16, y_min: i16, x_max: i16, y_max: i16, advance: u16) {
        self.any = true;
        self.x_min = self.x_min.min(x_min);
        self.y_min = self.y_min.min(y_min);
        self.x_max = self.x_max.max(x_max);
        self.y_max = self.y_max.max(y_max);
        self.min_lsb = self.min_lsb.min(x_min);
        let rsb = (advance as i32 - x_max as i32).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        self.min_rsb = self.min_rsb.min(rsb);
        self.max_extent = self.max_extent.max(x_max);
        self.max_advance = self.max_advance.max(advance);
    }

    fn or_zero(mut self) -> Self {
        if !self.any {
            self = Self {
                x_min: 0,
                y_min: 0,
                x_max: 0,
                y_max: 0,
                min_lsb: 0,
                min_rsb: 0,
                max_extent: 0,
                max_advance: self.max_advance,
                any: false,
            };
        }
        self
    }
}

fn average_width(advances: &[(u16, Option<write_fonts::tables::glyf::Bbox>)]) -> Option<i16> {
    let drawn: Vec<u32> = advances
        .iter()
        .filter(|(advance, _)| *advance > 0)
        .map(|(advance, _)| *advance as u32)
        .collect();
    if drawn.is_empty() {
        return None;
    }
    Some((drawn.iter().sum::<u32>() / drawn.len() as u32).min(i16::MAX as u32) as i16)
}

fn hmtx(advances: &[(u16, Option<write_fonts::tables::glyf::Bbox>)]) -> Vec<u8> {
    let mut out = Vec::with_capacity(advances.len() * 4);
    for (advance, bbox) in advances {
        out.extend_from_slice(&advance.to_be_bytes());
        let lsb = bbox.map(|bbox| bbox.x_min).unwrap_or(0);
        out.extend_from_slice(&lsb.to_be_bytes());
    }
    out
}

/// maxp version 1.0 for an unhinted TrueType font with only simple glyphs.
fn maxp(glyphs: u16, points: u16, contours: u16) -> Vec<u8> {
    let mut out = Vec::with_capacity(32);
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    out.extend_from_slice(&glyphs.to_be_bytes());
    out.extend_from_slice(&points.to_be_bytes());
    out.extend_from_slice(&contours.to_be_bytes());
    // Composite points and contours, then zones (1: no twilight zone), then the hinting limits.
    for value in [0u16, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0] {
        out.extend_from_slice(&value.to_be_bytes());
    }
    out
}

/// post version 3 (no glyph names), keeping the underline, with the italic angle of a slant.
fn post(font: &FontRef, slant: bool) -> Vec<u8> {
    let mut out = vec![0u8; 32];
    out[0..4].copy_from_slice(&0x0003_0000u32.to_be_bytes());
    if let Ok(source) = font.post() {
        let mut angle = source.italic_angle().to_bits();
        if slant {
            angle = (-(SLANT.atan().to_degrees()) * 65536.0).round() as i32;
        }
        out[4..8].copy_from_slice(&angle.to_be_bytes());
        out[8..10].copy_from_slice(&source.underline_position().to_i16().to_be_bytes());
        out[10..12].copy_from_slice(&source.underline_thickness().to_i16().to_be_bytes());
        out[12..16].copy_from_slice(&source.is_fixed_pitch().to_be_bytes());
    }
    out
}

fn set_u16(data: &mut [u8], offset: usize, value: u16) {
    if let Some(slot) = data.get_mut(offset..offset + 2) {
        slot.copy_from_slice(&value.to_be_bytes());
    }
}

fn set_i16(data: &mut [u8], offset: usize, value: i16) {
    set_u16(data, offset, value as u16);
}

fn get_u16(data: &[u8], offset: usize) -> u16 {
    data.get(offset..offset + 2)
        .map(|b| u16::from_be_bytes([b[0], b[1]]))
        .unwrap_or(0)
}

/// head: the target's bold and italic bits, and for a redrawn face its new bounds and loca format.
fn patch_head(data: &mut [u8], target: &Target, upem: Option<u16>, redrawn: Option<(Bounds, i16)>) {
    if let Some(upem) = upem {
        set_u16(data, 18, upem);
    }
    set_u16(
        data,
        44,
        target.mac_style & 0x0003 | get_u16(data, 44) & !0x0003,
    );
    if let Some((bounds, loca_format)) = redrawn {
        let bounds = bounds.or_zero();
        set_i16(data, 36, bounds.x_min);
        set_i16(data, 38, bounds.y_min);
        set_i16(data, 40, bounds.x_max);
        set_i16(data, 42, bounds.y_max);
        set_i16(data, 50, loca_format);
        set_i16(data, 52, 0);
        // Flags: baseline at 0, left sidebearing at x=0, integer ppem. Instructions are gone.
        set_u16(data, 16, get_u16(data, 16) & !0x0010 | 0x000B);
    }
}

fn patch_hhea_metrics(data: &mut [u8], metrics: &VMetrics) {
    set_i16(data, 4, metrics.hhea_ascender);
    set_i16(data, 6, metrics.hhea_descender);
    set_i16(data, 8, metrics.hhea_line_gap);
}

fn patch_hhea_extents(data: &mut [u8], bounds: &Bounds, glyphs: u16, slant: bool) {
    let bounds = bounds.or_zero();
    set_u16(data, 10, bounds.max_advance);
    set_i16(data, 12, bounds.min_lsb);
    set_i16(data, 14, bounds.min_rsb);
    set_i16(data, 16, bounds.max_extent);
    if slant {
        // Caret slope as rise over run, matching the slant.
        set_i16(data, 18, 1000);
        set_i16(data, 20, (1000.0 * SLANT).round() as i16);
    }
    set_u16(data, 34, glyphs);
}

/// fsSelection bits that say which style a face is: italic, bold, regular, WWS, oblique.
const STYLE_BITS: u16 = 0x0001 | 0x0020 | 0x0040 | 0x0100 | 0x0200;
const USE_TYPO_METRICS: u16 = 0x0080;

fn patch_os2(
    data: &mut [u8],
    target: &Target,
    metrics: &VMetrics,
    matched: bool,
    average: Option<i16>,
    scale: f64,
) {
    if let Some(average) = average {
        set_i16(data, 2, average);
    } else if (scale - 1.0).abs() > 0.005 {
        let current = get_u16(data, 2) as i16;
        set_i16(data, 2, (current as f64 * scale).round() as i16);
    }
    set_u16(data, 4, target.weight);
    set_u16(data, 6, target.width);
    if let Some(slot) = data.get_mut(32..42) {
        slot.copy_from_slice(&target.panose);
    }
    let mut selection = get_u16(data, 62) & !STYLE_BITS | target.fs_selection & STYLE_BITS;
    if matched {
        selection = selection & !USE_TYPO_METRICS
            | if metrics.use_typo_metrics {
                USE_TYPO_METRICS
            } else {
                0
            };
    }
    set_u16(data, 62, selection);
    set_i16(data, 68, metrics.typo_ascender);
    set_i16(data, 70, metrics.typo_descender);
    set_i16(data, 72, metrics.typo_line_gap);
    set_u16(data, 74, metrics.win_ascent);
    set_u16(data, 76, metrics.win_descent);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upem_shrinks_as_glyphs_grow() {
        assert_eq!(scaled_upem(2048, 1.0), 2048);
        assert_eq!(scaled_upem(2048, 1.25), 1638);
        assert_eq!(scaled_upem(1000, 0.8), 1250);
    }

    #[test]
    fn metrics_scale_to_another_em() {
        let segoe = VMetrics {
            upem: 2048,
            hhea_ascender: 2210,
            hhea_descender: -514,
            hhea_line_gap: 0,
            typo_ascender: 1491,
            typo_descender: -431,
            typo_line_gap: 269,
            win_ascent: 2210,
            win_descent: 514,
            use_typo_metrics: false,
        };
        let inter = segoe.scaled_to(1000);
        assert_eq!(inter.win_ascent, 1079);
        assert_eq!(inter.hhea_descender, -251);
        let height = |m: &VMetrics| (m.win_ascent + m.win_descent) as f32 / m.upem as f32;
        assert!((height(&inter) - height(&segoe)).abs() < 0.01);
    }

    #[test]
    fn lone_points_are_dropped() {
        let mut path = BezPath::new();
        path.move_to((0.0, 0.0));
        path.close_path();
        path.move_to((0.0, 0.0));
        path.line_to((10.0, 0.0));
        path.line_to((10.0, 10.0));
        path.close_path();
        let cleaned = clean(&path);
        assert_eq!(cleaned.elements().len(), 4);
    }
}

#[cfg(test)]
mod ink {
    use super::*;
    use ab_glyph::{Font, ScaleFont};

    /// Total ink of a line of text drawn at one em size, to compare a redrawn face with its source.
    fn ink(bytes: &[u8], text: &str) -> f32 {
        let font = ab_glyph::FontRef::try_from_slice(bytes).unwrap();
        let em = 64.0;
        let px = em * font.height_unscaled() / font.units_per_em().unwrap();
        let scaled = font.as_scaled(px);
        let mut x = 0.0;
        let mut total = 0.0;
        for ch in text.chars() {
            let mut glyph = scaled.scaled_glyph(ch);
            glyph.position = ab_glyph::point(x, 100.0);
            x += scaled.h_advance(glyph.id);
            if let Some(outline) = font.outline_glyph(glyph) {
                outline.draw(|_, _, c| total += c);
            }
        }
        total
    }

    #[test]
    fn redrawn_variable_face_has_the_same_weight_as_its_source() {
        let path = crate::fonts::windows_fonts_dir().join("bahnschrift.ttf");
        // Windows Server, which CI runs on, may not have Bahnschrift.
        let Ok(source) = std::fs::read(&path) else {
            eprintln!("Bahnschrift is not installed here; skipping.");
            return;
        };
        let font = FontRef::new(&source).unwrap();
        let segoe = crate::fonts::windows_fonts_dir().join("segoeui.ttf");
        let segoe = std::fs::read(segoe).unwrap();
        let segoe = FontRef::new(&segoe).unwrap();
        let target = Target {
            names: NameSet::from_font(&segoe).unwrap(),
            weight: 400,
            width: 5,
            italic: false,
            fs_selection: 0x40,
            mac_style: 0,
            panose: [0; 10],
            metrics: VMetrics::from_font(&segoe).unwrap(),
            opsz: None,
        };
        // Bahnschrift's default is weight 400 at full width, so the redrawn face should match it.
        let position = Position {
            axes: vec![("wght".into(), 400.0), ("wdth".into(), 100.0)],
            slant: false,
        };
        let redrawn = build(
            &font,
            &position,
            &target,
            Tweaks {
                match_metrics: true,
                scale: 1.0,
            },
            "test",
        )
        .unwrap();
        let text = "Hamburgefonstiv 0123";
        let (a, b) = (ink(&source, text), ink(&redrawn, text));
        assert!((a - b).abs() / a < 0.02, "source {a}, redrawn {b}");
    }
}
