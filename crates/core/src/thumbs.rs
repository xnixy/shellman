//! Thumbnails and real icons from the Windows shell, the same ones Explorer shows: photo and video
//! previews, and each program's own icon.
//!
//! Requests come from rows as they are drawn, so only what is on screen is asked for. Workers take
//! the newest request first, so after a fast scroll the rows in view load before the ones that
//! scrolled past.

use egui::{ColorImage, TextureHandle, TextureOptions};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use windows::core::HSTRING;
use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::{
    DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, DIB_RGB_COLORS, HBITMAP, HGDIOBJ,
};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Shell::{
    IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK, SIIGBF_RESIZETOFIT,
};

/// Sizes asked of the shell, in pixels: a list row's 24 point tile and a grid tile's 88 point
/// preview, each enough at 250% scale.
const SMALL_PX: i32 = 64;
const LARGE_PX: i32 = 224;
/// The jumbo size the shell offers, for program icons shown large (see `Thumbs::icons`).
const JUMBO_PX: i32 = 256;
const WORKERS: usize = 2;
/// Past this many, the cache is cleared rather than grown.
const MAX_CACHED: usize = 4000;

enum Slot {
    Pending,
    Ready(TextureHandle),
    /// The shell had nothing better than a generic icon; the colored tile stays.
    Missing,
}

/// A file and whether its large image is wanted.
type Key = (PathBuf, bool);
type Queue = Arc<(Mutex<Vec<Key>>, Condvar)>;

pub struct Thumbs {
    cache: HashMap<Key, Slot>,
    queue: Queue,
    rx: Receiver<(Key, Option<ColorImage>)>,
}

/// Files whose shell image says more than a glyph would: previews, and programs' own icons.
pub fn wants_thumb(name: &str) -> bool {
    let ext = Path::new(name)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    matches!(
        ext.as_str(),
        "png"
            | "jpg"
            | "jpeg"
            | "gif"
            | "webp"
            | "bmp"
            | "heic"
            | "avif"
            | "tif"
            | "tiff"
            | "ico"
            | "mp4"
            | "mkv"
            | "mov"
            | "avi"
            | "webm"
            | "wmv"
            | "m4v"
            | "exe"
            | "lnk"
            | "url"
            | "msi"
            | "appref-ms"
            | "pdf"
    )
}

/// Programs and icons, whose shell image is a transparent icon rather than a photo: drawn small
/// and unframed, where a preview is fitted into a larger box.
pub fn is_program(name: &str) -> bool {
    let ext = Path::new(name)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    matches!(
        ext.as_str(),
        "exe" | "lnk" | "url" | "msi" | "appref-ms" | "ico"
    )
}

impl Thumbs {
    pub fn new(ctx: egui::Context) -> Self {
        Self::build(ctx, false)
    }

    /// For showing programs' icons large and crisp: every image is asked for at 256 px and
    /// cropped to its opaque bounds, so an old 32 px icon the shell centred in a big transparent
    /// canvas comes back as the 32 px it really is. The texture's size is then the icon's true
    /// pixel size.
    pub fn icons(ctx: egui::Context) -> Self {
        Self::build(ctx, true)
    }

    fn build(ctx: egui::Context, trim: bool) -> Self {
        let queue: Queue = Arc::new((Mutex::new(Vec::new()), Condvar::new()));
        let (tx, rx) = channel();
        for _ in 0..WORKERS {
            let (queue, tx, ctx) = (queue.clone(), tx.clone(), ctx.clone());
            std::thread::spawn(move || worker(queue, tx, ctx, trim));
        }
        Self {
            cache: HashMap::new(),
            queue,
            rx,
        }
    }

    /// The image for a file if it is ready, small for a row or large for a tile. Otherwise asks
    /// for it, once.
    pub fn get(&mut self, path: &Path, large: bool) -> Option<&TextureHandle> {
        let key = (path.to_path_buf(), large);
        if !self.cache.contains_key(&key) {
            if self.cache.len() >= MAX_CACHED {
                self.cache.retain(|_, s| matches!(s, Slot::Pending));
            }
            self.cache.insert(key.clone(), Slot::Pending);
            let (lock, cv) = &*self.queue;
            lock.lock().unwrap().push(key.clone());
            cv.notify_one();
        }
        match self.cache.get(&key) {
            Some(Slot::Ready(t)) => Some(t),
            _ => None,
        }
    }

    /// Turn finished images into textures.
    pub fn poll(&mut self, ctx: &egui::Context) {
        while let Ok((key, image)) = self.rx.try_recv() {
            let slot = match image {
                Some(img) => {
                    let name = format!("thumb:{}:{}", key.1, key.0.display());
                    Slot::Ready(ctx.load_texture(name, img, TextureOptions::LINEAR))
                }
                None => Slot::Missing,
            };
            self.cache.insert(key, slot);
        }
    }

    /// Drop requests nobody is waiting for any more, after leaving a folder.
    pub fn cancel_pending(&mut self) {
        let (lock, _) = &*self.queue;
        let dropped: Vec<Key> = std::mem::take(&mut *lock.lock().unwrap());
        for key in dropped {
            if matches!(self.cache.get(&key), Some(Slot::Pending)) {
                self.cache.remove(&key);
            }
        }
    }

    /// Forget images for files in `dir`, so changed files are asked for again.
    pub fn invalidate_dir(&mut self, dir: &Path) {
        self.cache
            .retain(|(p, _), s| matches!(s, Slot::Pending) || p.parent() != Some(dir));
    }
}

fn worker(queue: Queue, tx: Sender<(Key, Option<ColorImage>)>, ctx: egui::Context, trim: bool) {
    // Shell thumbnail providers expect a single-threaded apartment.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    loop {
        let key = {
            let (lock, cv) = &*queue;
            let mut q = lock.lock().unwrap();
            loop {
                if let Some(p) = q.pop() {
                    break p;
                }
                q = cv.wait(q).unwrap();
            }
        };
        let px = match (trim, key.1) {
            (true, _) => JUMBO_PX,
            (false, true) => LARGE_PX,
            (false, false) => SMALL_PX,
        };
        let image = shell_image(&key.0, px).map(|i| if trim { crop_to_opaque(i) } else { i });
        if tx.send((key, image)).is_err() {
            return;
        }
        ctx.request_repaint();
    }
}

/// The shell's image for a file: a preview where there is one, else the file's own icon.
fn shell_image(path: &Path, px: i32) -> Option<ColorImage> {
    unsafe {
        let factory: IShellItemImageFactory =
            SHCreateItemFromParsingName(&HSTRING::from(path.as_os_str()), None).ok()?;
        let hbmp = factory
            .GetImage(
                SIZE { cx: px, cy: px },
                SIIGBF_RESIZETOFIT | SIIGBF_BIGGERSIZEOK,
            )
            .ok()?;
        let image = to_color_image(hbmp);
        let _ = DeleteObject(HGDIOBJ(hbmp.0));
        image
    }
}

/// The image cut down to the box around its visible pixels (alpha above a faint threshold).
/// Unchanged when it has no transparency at all, or nothing visible.
fn crop_to_opaque(img: ColorImage) -> ColorImage {
    let [w, h] = img.size;
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for y in 0..h {
        for x in 0..w {
            if img.pixels[y * w + x].a() > 12 {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x + 1);
                y1 = y1.max(y + 1);
            }
        }
    }
    if x1 <= x0 || (x0, y0, x1, y1) == (0, 0, w, h) {
        return img;
    }
    let mut pixels = Vec::with_capacity((x1 - x0) * (y1 - y0));
    for y in y0..y1 {
        pixels.extend_from_slice(&img.pixels[y * w + x0..y * w + x1]);
    }
    ColorImage { size: [x1 - x0, y1 - y0], pixels }
}

/// Copy a 32-bit bitmap out as RGBA. Shell bitmaps are premultiplied BGRA, and photo previews
/// often leave alpha at zero, which means opaque.
unsafe fn to_color_image(hbmp: HBITMAP) -> Option<ColorImage> {
    let mut bm = BITMAP::default();
    if GetObjectW(
        HGDIOBJ(hbmp.0),
        size_of::<BITMAP>() as i32,
        Some((&raw mut bm).cast()),
    ) == 0
    {
        return None;
    }
    let (w, h) = (bm.bmWidth, bm.bmHeight.abs());
    if w <= 0 || h <= 0 {
        return None;
    }
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            // Negative height asks for rows top to bottom.
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut buf = vec![0u8; (w * h * 4) as usize];
    let dc = GetDC(None);
    let lines = GetDIBits(
        dc,
        hbmp,
        0,
        h as u32,
        Some(buf.as_mut_ptr().cast()),
        &mut info,
        DIB_RGB_COLORS,
    );
    ReleaseDC(None, dc);
    if lines == 0 {
        return None;
    }
    let opaque = buf.chunks_exact(4).all(|p| p[3] == 0);
    for p in buf.chunks_exact_mut(4) {
        p.swap(0, 2);
        if opaque {
            p[3] = 255;
        }
    }
    Some(ColorImage::from_rgba_premultiplied(
        [w as usize, h as usize],
        &buf,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_previewable_files() {
        assert!(wants_thumb("Photo.JPG"));
        assert!(wants_thumb("setup.exe"));
        assert!(!wants_thumb("notes.txt"));
        assert!(!wants_thumb("Makefile"));
    }

    #[test]
    fn crop_keeps_only_the_visible_box() {
        use egui::Color32;
        let mut img = ColorImage::new([8, 8], Color32::TRANSPARENT);
        for (x, y) in [(2, 3), (4, 5)] {
            img.pixels[y * 8 + x] = Color32::from_rgb(200, 0, 0);
        }
        let cropped = crop_to_opaque(img);
        assert_eq!(cropped.size, [3, 3]);
        let blank = crop_to_opaque(ColorImage::new([4, 4], Color32::TRANSPARENT));
        assert_eq!(blank.size, [4, 4]);
    }

    #[test]
    fn shell_gives_an_icon_for_an_exe() {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        let img = shell_image(Path::new(r"C:\Windows\System32\notepad.exe"), 48)
            .expect("notepad has an icon");
        assert!(img.size[0] > 0 && img.size[1] > 0);
        assert!(img.pixels.iter().any(|p| p.a() > 0));
    }
}
