//! Turn a picture file into the one bitmap a monitor needs, using WIC.
//!
//! The picture is decoded, scaled once to the size its fit mode asks for (Fant when shrinking,
//! high-quality cubic when enlarging), converted to 32-bit BGR, and only the kept part is copied
//! straight into a GDI DIB section. That bitmap is all the window keeps. Runs on the worker
//! thread; GDI bitmaps are not tied to a thread, so the UI thread can use it as is.

use crate::fit::{self, Area, Fit, Plan};
use crate::surface::Image;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use windows::core::{Interface, PCWSTR};
use windows::Win32::Foundation::GENERIC_READ;
use windows::Win32::Graphics::Gdi::{CreateDIBSection, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppBGR, IWICBitmapSource, IWICImagingFactory,
    WICBitmapDitherTypeNone, WICBitmapInterpolationModeFant, WICBitmapInterpolationModeHighQualityCubic,
    WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnDemand, WICRect,
};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};

/// One factory per worker run.
pub fn factory() -> Option<IWICImagingFactory> {
    unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok() }
}

/// What the monitor being drawn is, in the layout of all monitors.
pub struct Target {
    pub mon: Area,
    /// The rectangle around all monitors, for `Fit::Span`.
    pub union: Area,
    pub fit: Fit,
}

/// Decode `path` and render it for `target`. None if the file cannot be read as a picture.
pub fn render(factory: &IWICImagingFactory, path: &Path, target: &Target) -> Option<Image> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        let decoder = factory
            .CreateDecoderFromFilename(PCWSTR(wide.as_ptr()), None, GENERIC_READ, WICDecodeMetadataCacheOnDemand)
            .ok()?;
        let frame = decoder.GetFrame(0).ok()?;
        let (mut iw, mut ih) = (0u32, 0u32);
        frame.GetSize(&mut iw, &mut ih).ok()?;
        let plan = plan_for(target, (iw as i32, ih as i32));

        let mut source: IWICBitmapSource = frame.cast().ok()?;
        if plan.scaled != (iw as i32, ih as i32) {
            let scaler = factory.CreateBitmapScaler().ok()?;
            let growing = plan.scaled.0 > iw as i32 || plan.scaled.1 > ih as i32;
            let mode = if growing { WICBitmapInterpolationModeHighQualityCubic } else { WICBitmapInterpolationModeFant };
            scaler.Initialize(&source, plan.scaled.0 as u32, plan.scaled.1 as u32, mode).ok()?;
            source = scaler.cast().ok()?;
        }
        let converter = factory.CreateFormatConverter().ok()?;
        converter
            .Initialize(&source, &GUID_WICPixelFormat32bppBGR, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom)
            .ok()?;

        let (w, h) = (plan.src.w, plan.src.h);
        let (bitmap, bits) = dib(w, h)?;
        let stride = w as u32 * 4;
        let rect = WICRect { X: plan.src.x, Y: plan.src.y, Width: w, Height: h };
        let pixels = std::slice::from_raw_parts_mut(bits, stride as usize * h as usize);
        if converter.CopyPixels(&rect, stride, pixels).is_err() {
            let _ = windows::Win32::Graphics::Gdi::DeleteObject(windows::Win32::Graphics::Gdi::HGDIOBJ(bitmap as *mut _));
            return None;
        }
        Some(Image { bitmap, dst: plan.dst, tile: plan.tile })
    }
}

fn plan_for(target: &Target, img: (i32, i32)) -> Plan {
    let Target { mon, union, fit } = *target;
    if fit == Fit::Span {
        let rel = Area::new(mon.x - union.x, mon.y - union.y, mon.w, mon.h);
        fit::plan_span(img, (union.w, union.h), rel)
    } else {
        fit::plan(fit, img, (mon.w, mon.h))
    }
}

/// A top-down 32-bit DIB section and a pointer to its pixels.
unsafe fn dib(w: i32, h: i32) -> Option<(isize, *mut u8)> {
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits = std::ptr::null_mut();
    let bitmap = CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
    (!bits.is_null()).then_some((bitmap.0 as isize, bits.cast()))
}
