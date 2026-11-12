//! What the details island knows about the selection, read on a worker so the window never
//! waits: a text file's first lines, an image's size in pixels, when an item was made, and for a
//! folder (or several items) how many items and how many bytes are inside, counted through every
//! level. A new selection cancels the count under way, and an answer for an older selection is
//! dropped. Drawing is in `details_ui`.

use super::Msg;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How many lines of a text file are shown, and how much of it is read for them.
const PREVIEW_LINES: usize = 14;
const PREVIEW_BYTES: usize = 8 * 1024;
/// How often a long count says how far it got.
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

/// One answer from the worker, for the request numbered `gen`.
#[derive(Clone, Debug, Default)]
pub(crate) struct Reading {
    pub(crate) gen: u64,
    /// The first lines of a text file.
    pub(crate) lines: Option<Vec<String>>,
    /// An image's width and height in pixels.
    pub(crate) pixels: Option<(u32, u32)>,
    /// When the item was made, as the listing writes dates.
    pub(crate) created: Option<String>,
    /// Inside a folder, or the whole selection: items and bytes, through every level.
    pub(crate) total: Option<Total>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Total {
    pub(crate) files: u64,
    pub(crate) folders: u64,
    pub(crate) bytes: u64,
    /// Still counting (this is how far it got).
    pub(crate) partial: bool,
}

/// The island's state between frames.
#[derive(Default)]
pub(crate) struct Details {
    /// What the last request was for, its number, and the flag that cancels its count.
    asked: Option<(Vec<PathBuf>, u64)>,
    cancel: Option<Arc<AtomicBool>>,
    pub(crate) reading: Reading,
}

impl Details {
    /// Ask about `items` when they are not what was last asked about. Cheap when nothing
    /// changed, so it is called every frame.
    pub(crate) fn want(&mut self, items: Vec<(PathBuf, bool)>, tx: &Sender<Msg>, ctx: &egui::Context) {
        let paths: Vec<PathBuf> = items.iter().map(|(p, _)| p.clone()).collect();
        if self.asked.as_ref().is_some_and(|(p, _)| *p == paths) {
            return;
        }
        if let Some(c) = self.cancel.take() {
            c.store(true, Ordering::Relaxed);
        }
        let gen = self.asked.as_ref().map_or(1, |(_, g)| g + 1);
        self.asked = Some((paths, gen));
        self.reading = Reading {
            gen,
            ..Default::default()
        };
        if items.is_empty() {
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.cancel = Some(cancel.clone());
        let (tx, ctx) = (tx.clone(), ctx.clone());
        std::thread::spawn(move || read(gen, &items, &cancel, &|r| {
            let _ = tx.send(Msg::Details(r));
            ctx.request_repaint();
        }));
    }

    /// An answer: kept only when it is for the current request.
    pub(crate) fn arrived(&mut self, r: Reading) {
        if self.asked.as_ref().is_some_and(|(_, g)| *g == r.gen) {
            self.reading = r;
        }
    }

    /// Stop any count and forget what was asked (the island was hidden).
    pub(crate) fn stop(&mut self) {
        if let Some(c) = self.cancel.take() {
            c.store(true, Ordering::Relaxed);
        }
        self.asked = None;
        self.reading = Reading::default();
    }
}

/// The worker's whole job for one request: what one file or folder is, or the total of several.
fn read(gen: u64, items: &[(PathBuf, bool)], cancel: &AtomicBool, send: &dyn Fn(Reading)) {
    let mut r = Reading {
        gen,
        ..Default::default()
    };
    if let [(path, is_dir)] = items {
        r.created = created(path);
        if !is_dir {
            r.pixels = image_size_of(path);
            if r.pixels.is_none() && is_text_name(path) {
                r.lines = text_preview(path);
            }
            send(r);
            return;
        }
        send(r.clone());
    }
    // Folders, or several items: count through everything, saying how far it got now and then.
    let mut total = Total::default();
    let mut last = Instant::now();
    let mut tick = |t: &Total| {
        if last.elapsed() >= PROGRESS_EVERY {
            last = Instant::now();
            send(Reading {
                total: Some(Total { partial: true, ..*t }),
                ..r.clone()
            });
        }
    };
    for (path, is_dir) in items {
        if *is_dir {
            if items.len() > 1 {
                total.folders += 1;
            }
            if !walk(path, &mut total, cancel, &mut tick) {
                return;
            }
        } else {
            total.files += 1;
            total.bytes += std::fs::metadata(path).map_or(0, |m| m.len());
        }
    }
    r.total = Some(total);
    send(r);
}

/// Count what is inside `dir`, through every level, into `total`. Links to folders are not
/// followed (they would count things twice, or forever). False when cancelled.
pub(crate) fn walk(dir: &Path, total: &mut Total, cancel: &AtomicBool, tick: &mut dyn FnMut(&Total)) -> bool {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            return false;
        }
        let Ok(list) = std::fs::read_dir(&d) else { continue };
        for item in list.flatten() {
            let Ok(ft) = item.file_type() else { continue };
            if ft.is_dir() {
                total.folders += 1;
                stack.push(item.path());
            } else if !ft.is_symlink() {
                total.files += 1;
                total.bytes += item.metadata().map_or(0, |m| m.len());
            }
        }
        tick(total);
    }
    true
}

fn created(p: &Path) -> Option<String> {
    let t = std::fs::metadata(p).ok()?.created().ok()?;
    let e = shellman_core::fsops::entry_for(p)?;
    // The listing has the modified time in local time already; the creation time is that,
    // moved by the difference between the two.
    let modified = e.modified?;
    let shift = t.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() as i64
        - modified.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() as i64;
    Some(shift_text(&e.modified_text, shift))
}

/// "YYYY-MM-DD HH:MM" moved by `secs` (the creation time from the modified time, which the
/// listing has already put in local time). Days are worked out by the calendar.
pub(crate) fn shift_text(text: &str, secs: i64) -> String {
    let parse = || -> Option<i64> {
        let (d, t) = text.split_once(' ')?;
        let mut dp = d.split('-').map(|x| x.parse::<i64>().ok());
        let (y, m, day) = (dp.next()??, dp.next()??, dp.next()??);
        let (h, min) = t.split_once(':')?;
        let mins = h.parse::<i64>().ok()? * 60 + min.parse::<i64>().ok()?;
        Some((days_from_civil(y, m, day) * 1440 + mins) * 60)
    };
    let Some(at) = parse() else { return text.to_owned() };
    let at = at + secs;
    let (days, rem) = (at.div_euclid(86_400), at.rem_euclid(86_400));
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}", rem / 3600, rem % 3600 / 60)
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + d - 1;
    era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// Files whose first lines are worth showing.
pub(crate) fn is_text_name(p: &Path) -> bool {
    let ext = p
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    matches!(
        ext.as_str(),
        "txt" | "md" | "log" | "ini" | "cfg" | "conf" | "json" | "toml" | "yaml" | "yml" | "xml"
            | "csv" | "rs" | "py" | "js" | "ts" | "c" | "h" | "cpp" | "cs" | "go" | "java" | "sh"
            | "ps1" | "bat" | "cmd" | "html" | "css" | "sql" | "lua" | "gitignore"
    )
}

fn text_preview(p: &Path) -> Option<Vec<String>> {
    use std::io::Read;
    let mut buf = vec![0; PREVIEW_BYTES];
    let n = std::fs::File::open(p).ok()?.read(&mut buf).ok()?;
    buf.truncate(n);
    preview_lines(&buf, PREVIEW_LINES)
}

/// The first `max` lines of `bytes` as text, tabs as spaces; None when it does not look like
/// text (a NUL byte). A cut-off last character is dropped.
pub(crate) fn preview_lines(bytes: &[u8], max: usize) -> Option<Vec<String>> {
    if bytes.contains(&0) {
        return None;
    }
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    let text = String::from_utf8_lossy(bytes);
    let text = text.trim_end_matches('\u{FFFD}');
    Some(
        text.lines()
            .take(max)
            .map(|l| l.replace('\t', "    ").trim_end().to_owned())
            .collect(),
    )
}

fn image_size_of(p: &Path) -> Option<(u32, u32)> {
    use std::io::Read;
    let mut buf = vec![0; 64 * 1024];
    let n = std::fs::File::open(p).ok()?.read(&mut buf).ok()?;
    image_size(&buf[..n])
}

/// An image's width and height from the start of its file: PNG, JPEG, GIF, BMP and WebP.
pub(crate) fn image_size(b: &[u8]) -> Option<(u32, u32)> {
    let be16 = |i: usize| Some(u16::from_be_bytes([*b.get(i)?, *b.get(i + 1)?]) as u32);
    let le16 = |i: usize| Some(u16::from_le_bytes([*b.get(i)?, *b.get(i + 1)?]) as u32);
    let be32 = |i: usize| Some(u32::from_be_bytes(b.get(i..i + 4)?.try_into().ok()?));
    let le32 = |i: usize| Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?));
    let le24 = |i: usize| Some(u32::from_le_bytes([*b.get(i)?, *b.get(i + 1)?, *b.get(i + 2)?, 0]));
    if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some((be32(16)?, be32(20)?));
    }
    if b.starts_with(b"GIF8") {
        return Some((le16(6)?, le16(8)?));
    }
    if b.starts_with(b"BM") {
        let h = le32(22)? as i32;
        return Some((le32(18)?, h.unsigned_abs()));
    }
    if b.starts_with(b"RIFF") && b.get(8..12) == Some(b"WEBP") {
        return match b.get(12..16)? {
            b"VP8X" => Some((
                1 + le24(24)?,
                1 + le24(27)?,
            )),
            b"VP8 " => Some((le16(26)? & 0x3FFF, le16(28)? & 0x3FFF)),
            b"VP8L" => {
                let v = le32(21)?;
                Some((1 + (v & 0x3FFF), 1 + ((v >> 14) & 0x3FFF)))
            }
            _ => None,
        };
    }
    if b.starts_with(&[0xFF, 0xD8]) {
        // Walk the markers to the first start-of-frame.
        let mut i = 2;
        while i + 9 < b.len() {
            if b[i] != 0xFF {
                i += 1;
                continue;
            }
            let marker = b[i + 1];
            if marker == 0xFF {
                i += 1;
                continue;
            }
            let len = be16(i + 2)? as usize;
            let sof = matches!(marker, 0xC0..=0xCF) && !matches!(marker, 0xC4 | 0xC8 | 0xCC);
            if sof {
                return Some((be16(i + 7)?, be16(i + 5)?));
            }
            i += 2 + len;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_sizes_come_from_the_header() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend(3840u32.to_be_bytes());
        png.extend(2160u32.to_be_bytes());
        assert_eq!(image_size(&png), Some((3840, 2160)));

        let gif = [b"GIF89a".as_slice(), &[0x40, 0x01, 0xF0, 0x00]].concat();
        assert_eq!(image_size(&gif), Some((320, 240)));

        let mut bmp = vec![0u8; 26];
        bmp[..2].copy_from_slice(b"BM");
        bmp[18..22].copy_from_slice(&640u32.to_le_bytes());
        bmp[22..26].copy_from_slice(&(-480i32).to_le_bytes());
        assert_eq!(image_size(&bmp), Some((640, 480)));

        // A JPEG with an APP0 segment before its frame.
        let jpg = [
            0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00, 0xFF, 0xC0, 0x00, 0x11, 0x08, 0x04,
            0x38, 0x07, 0x80, 0x03, 0, 0, 0, 0, 0,
        ];
        assert_eq!(image_size(&jpg), Some((1920, 1080)));

        let mut webp = b"RIFF\0\0\0\0WEBPVP8X".to_vec();
        webp.extend([0u8; 8]);
        webp.extend([0xFF, 0x0E, 0x00]); // width - 1 = 3839
        webp.extend([0x6F, 0x08, 0x00]); // height - 1 = 2159
        assert_eq!(image_size(&webp), Some((3840, 2160)));

        assert_eq!(image_size(b"not an image"), None);
        assert_eq!(image_size(b"\x89PNG\r\n\x1a\n"), None, "cut short");
    }

    #[test]
    fn text_previews_take_the_first_lines_and_refuse_binary() {
        let lines = preview_lines(b"\xEF\xBB\xBFone\n\ttwo  \r\nthree\nfour", 3).unwrap();
        assert_eq!(lines, ["one", "    two", "three"]);
        assert!(preview_lines(b"MZ\0\0binary", 3).is_none());
        // Cut in the middle of a character: the broken end is dropped.
        let cut = "héllo".as_bytes();
        assert_eq!(preview_lines(&cut[..2], 3).unwrap(), ["h"]);
    }

    #[test]
    fn created_dates_shift_across_days() {
        assert_eq!(shift_text("2026-11-12 12:24", -3600), "2026-11-12 11:24");
        assert_eq!(shift_text("2026-03-01 00:10", -900), "2026-02-28 23:55");
        assert_eq!(shift_text("2024-12-31 23:00", 7200), "2025-01-01 01:00");
        assert_eq!(shift_text("garbage", 60), "garbage");
    }

    #[test]
    fn a_folder_is_counted_through_every_level_and_can_be_cancelled() {
        let dir = std::env::temp_dir().join(format!("files-details-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("a").join("b")).unwrap();
        std::fs::write(dir.join("x.txt"), b"12345").unwrap();
        std::fs::write(dir.join("a").join("b").join("y.bin"), vec![0u8; 100]).unwrap();
        let mut total = Total::default();
        let mut ticks = 0;
        assert!(walk(&dir, &mut total, &AtomicBool::new(false), &mut |_| ticks += 1));
        assert_eq!((total.files, total.folders, total.bytes), (2, 2, 105));
        assert!(ticks >= 1);
        let mut t2 = Total::default();
        assert!(!walk(&dir, &mut t2, &AtomicBool::new(true), &mut |_| {}));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn answers_for_an_older_selection_are_dropped() {
        let mut d = Details::default();
        d.asked = Some((vec![PathBuf::from(r"C:\b")], 2));
        d.arrived(Reading {
            gen: 1,
            created: Some("old".into()),
            ..Default::default()
        });
        assert!(d.reading.created.is_none());
        d.arrived(Reading {
            gen: 2,
            created: Some("new".into()),
            ..Default::default()
        });
        assert_eq!(d.reading.created.as_deref(), Some("new"));
    }
}
