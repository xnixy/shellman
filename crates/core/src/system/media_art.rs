//! Album art: a picture from a media session, decoded with WIC and cut to a small square.

use windows::core::Interface;
use windows::Storage::Streams::{DataReader, IRandomAccessStreamReference};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppRGBA, IWICBitmapSource, IWICImagingFactory,
    WICBitmapDitherTypeNone, WICBitmapInterpolationModeFant, WICBitmapPaletteTypeCustom,
    WICDecodeMetadataCacheOnDemand, WICRect,
};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::Shell::SHCreateMemStream;

/// Square RGBA pixels, ready for a texture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Art {
    pub side: u32,
    pub rgba: Vec<u8>,
}

/// Pictures larger than this are not worth reading for a thumbnail.
const MAX_BYTES: u64 = 8 << 20;

/// Read and decode `thumb` to `side` x `side` pixels, cut from its middle. Waits on WinRT, so
/// only call it on a worker.
pub fn load(thumb: &IRandomAccessStreamReference, side: u32) -> Option<Art> {
    let stream = thumb.OpenReadAsync().ok()?.get().ok()?;
    let size = stream.Size().ok()?;
    if size == 0 || size > MAX_BYTES {
        return None;
    }
    let reader = DataReader::CreateDataReader(&stream).ok()?;
    let got = reader.LoadAsync(size as u32).ok()?.get().ok()?;
    let mut bytes = vec![0u8; got as usize];
    reader.ReadBytes(&mut bytes).ok()?;
    decode(&bytes, side)
}

/// Decode an encoded picture to a centred square of `side` pixels.
pub fn decode(bytes: &[u8], side: u32) -> Option<Art> {
    // SAFETY: COM calls with valid arguments; the pixel buffer is sized for the copy.
    unsafe {
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
        let stream = SHCreateMemStream(Some(bytes))?;
        let decoder = factory
            .CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)
            .ok()?;
        let frame = decoder.GetFrame(0).ok()?;
        let (mut w, mut h) = (0u32, 0u32);
        frame.GetSize(&mut w, &mut h).ok()?;
        let (sw, sh) = cover(w, h, side)?;
        let scaler = factory.CreateBitmapScaler().ok()?;
        scaler.Initialize(&frame, sw, sh, WICBitmapInterpolationModeFant).ok()?;
        let converter = factory.CreateFormatConverter().ok()?;
        let source: IWICBitmapSource = scaler.cast().ok()?;
        converter
            .Initialize(&source, &GUID_WICPixelFormat32bppRGBA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom)
            .ok()?;
        let rect = WICRect {
            X: ((sw - side) / 2) as i32,
            Y: ((sh - side) / 2) as i32,
            Width: side as i32,
            Height: side as i32,
        };
        let mut rgba = vec![0u8; (side * side * 4) as usize];
        converter.CopyPixels(&rect, side * 4, &mut rgba).ok()?;
        Some(Art { side, rgba })
    }
}

/// The size to scale a `w` x `h` picture to so it just covers a `side` square.
fn cover(w: u32, h: u32, side: u32) -> Option<(u32, u32)> {
    if w == 0 || h == 0 || side == 0 {
        return None;
    }
    let scale = side as f64 / w.min(h) as f64;
    let fit = |v: u32| ((v as f64 * scale).round() as u32).max(side);
    Some((fit(w), fit(h)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn covers_the_square() {
        assert_eq!(cover(1280, 720, 96), Some((171, 96)));
        assert_eq!(cover(300, 300, 96), Some((96, 96)));
        assert_eq!(cover(50, 100, 96), Some((96, 192)));
        assert_eq!(cover(0, 10, 96), None);
    }

    #[test]
    fn garbage_is_not_a_picture() {
        crate::system::live::init_mta();
        assert_eq!(decode(b"not a picture", 32), None);
    }
}
