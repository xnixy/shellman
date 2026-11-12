//! Asking Windows itself what a font file is called. GDI and DirectWrite each name faces their own
//! way, and a replacement has to answer to both exactly as the Segoe UI face it stands in for.

use std::path::Path;
use windows::core::{w, BOOL, HSTRING, PCSTR, PCWSTR};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory6, IDWriteFontSet, IDWriteLocalizedStrings,
    DWRITE_FACTORY_TYPE_ISOLATED, DWRITE_FONT_PROPERTY_ID, DWRITE_FONT_PROPERTY_ID_FULL_NAME,
    DWRITE_FONT_PROPERTY_ID_STYLE, DWRITE_FONT_PROPERTY_ID_TYPOGRAPHIC_FAMILY_NAME,
    DWRITE_FONT_PROPERTY_ID_WEIGHT, DWRITE_FONT_PROPERTY_ID_WEIGHT_STRETCH_STYLE_FACE_NAME,
    DWRITE_FONT_PROPERTY_ID_WEIGHT_STRETCH_STYLE_FAMILY_NAME,
    DWRITE_FONT_PROPERTY_ID_WIN32_FAMILY_NAME,
};
use windows::Win32::Graphics::Gdi::{
    AddFontResourceExW, RemoveFontResourceExW, FR_PRIVATE, LOGFONTW,
};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

/// A face as GDI lists it: the family name apps pass to CreateFont, and its weight and slope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GdiFace {
    pub family: String,
    pub weight: i32,
    pub italic: bool,
}

/// A face as DirectWrite lists it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DwFace {
    /// The weight-stretch-style family, which most DirectWrite apps ask for.
    pub family: String,
    pub face: String,
    pub typographic: String,
    pub win32: String,
    pub full: String,
    pub weight: String,
    pub style: String,
}

type GetFontResourceInfo = unsafe extern "system" fn(
    filename: PCWSTR,
    size: *mut u32,
    buffer: *mut core::ffi::c_void,
    query: u32,
) -> BOOL;

/// Ask for the LOGFONT of every face in the file.
const QFR_LOGFONT: u32 = 2;

/// The faces GDI makes of a font file, through the same gdi32 call the Fonts control panel uses.
pub fn gdi_faces(path: &Path) -> Result<Vec<GdiFace>, String> {
    let query: GetFontResourceInfo = unsafe {
        let gdi = LoadLibraryW(w!("gdi32.dll")).map_err(|err| err.to_string())?;
        let found = GetProcAddress(gdi, PCSTR(c"GetFontResourceInfoW".as_ptr().cast()))
            .ok_or("This Windows has no GetFontResourceInfoW.")?;
        std::mem::transmute::<unsafe extern "system" fn() -> isize, GetFontResourceInfo>(found)
    };
    let full = std::path::absolute(path).map_err(|err| err.to_string())?;
    let name = HSTRING::from(full.as_os_str());
    // GDI only describes files it has loaded. Loading privately keeps it from every other app.
    let loaded = unsafe { AddFontResourceExW(&name, FR_PRIVATE, None) };
    let result = query_logfonts(query, &name, path);
    if loaded > 0 {
        unsafe {
            let _ = RemoveFontResourceExW(&name, FR_PRIVATE.0, None);
        }
    }
    result
}

fn query_logfonts(
    query: GetFontResourceInfo,
    name: &HSTRING,
    path: &Path,
) -> Result<Vec<GdiFace>, String> {
    let mut size = 0u32;
    let _ = unsafe {
        query(
            PCWSTR(name.as_ptr()),
            &mut size,
            std::ptr::null_mut(),
            QFR_LOGFONT,
        )
    };
    let count = size as usize / size_of::<LOGFONTW>();
    if count == 0 {
        return Err(format!("GDI cannot load {}.", path.display()));
    }
    let mut fonts = vec![LOGFONTW::default(); count];
    let mut size = (count * size_of::<LOGFONTW>()) as u32;
    let ok = unsafe {
        query(
            PCWSTR(name.as_ptr()),
            &mut size,
            fonts.as_mut_ptr().cast(),
            QFR_LOGFONT,
        )
    };
    if !ok.as_bool() {
        return Err(format!("GDI cannot read the faces of {}.", path.display()));
    }
    Ok(fonts
        .iter()
        .map(|font| GdiFace {
            family: wide_to_string(&font.lfFaceName),
            weight: font.lfWeight,
            italic: font.lfItalic != 0,
        })
        .collect())
}

/// The faces DirectWrite makes of a font file, a variable font giving one per named instance.
pub fn dwrite_faces(path: &Path) -> Result<Vec<DwFace>, String> {
    unsafe {
        let factory: IDWriteFactory6 =
            DWriteCreateFactory(DWRITE_FACTORY_TYPE_ISOLATED).map_err(|err| err.to_string())?;
        let builder = factory
            .CreateFontSetBuilder()
            .map_err(|err| err.to_string())?;
        let full = std::path::absolute(path).map_err(|err| err.to_string())?;
        builder
            .AddFontFile(&HSTRING::from(full.as_os_str()))
            .map_err(|err| format!("DirectWrite cannot load {}: {err}", path.display()))?;
        let set = builder.CreateFontSet().map_err(|err| err.to_string())?;
        let mut faces = Vec::new();
        for index in 0..set.GetFontCount() {
            let get = |id| property(&set, index, id);
            faces.push(DwFace {
                family: get(DWRITE_FONT_PROPERTY_ID_WEIGHT_STRETCH_STYLE_FAMILY_NAME),
                face: get(DWRITE_FONT_PROPERTY_ID_WEIGHT_STRETCH_STYLE_FACE_NAME),
                typographic: get(DWRITE_FONT_PROPERTY_ID_TYPOGRAPHIC_FAMILY_NAME),
                win32: get(DWRITE_FONT_PROPERTY_ID_WIN32_FAMILY_NAME),
                full: get(DWRITE_FONT_PROPERTY_ID_FULL_NAME),
                weight: get(DWRITE_FONT_PROPERTY_ID_WEIGHT),
                style: get(DWRITE_FONT_PROPERTY_ID_STYLE),
            });
        }
        if faces.is_empty() {
            return Err(format!("DirectWrite found no faces in {}.", path.display()));
        }
        Ok(faces)
    }
}

fn property(set: &IDWriteFontSet, index: u32, id: DWRITE_FONT_PROPERTY_ID) -> String {
    let mut exists = BOOL(0);
    let mut values: Option<IDWriteLocalizedStrings> = None;
    unsafe {
        if set
            .GetPropertyValues3(index, id, &mut exists, &mut values)
            .is_err()
            || !exists.as_bool()
        {
            return String::new();
        }
    }
    values.map(|values| english(&values)).unwrap_or_default()
}

fn english(values: &IDWriteLocalizedStrings) -> String {
    unsafe {
        let mut index = 0u32;
        let mut exists = BOOL(0);
        if values
            .FindLocaleName(w!("en-us"), &mut index, &mut exists)
            .is_err()
            || !exists.as_bool()
        {
            index = 0;
        }
        if values.GetCount() == 0 {
            return String::new();
        }
        let Ok(len) = values.GetStringLength(index) else {
            return String::new();
        };
        let mut buffer = vec![0u16; len as usize + 1];
        if values.GetString(index, &mut buffer).is_err() {
            return String::new();
        }
        wide_to_string(&buffer)
    }
}

fn wide_to_string(wide: &[u16]) -> String {
    let len = wide.iter().position(|ch| *ch == 0).unwrap_or(wide.len());
    String::from_utf16_lossy(&wide[..len])
}

/// One way the replacement would answer differently from the face it replaces.
pub fn compare(
    expected_gdi: &[GdiFace],
    got_gdi: &[GdiFace],
    expected_dw: &[DwFace],
    got_dw: &[DwFace],
) -> Vec<String> {
    let mut problems = Vec::new();
    for want in expected_gdi {
        let found = got_gdi.iter().any(|have| {
            have.family.eq_ignore_ascii_case(&want.family)
                && have.italic == want.italic
                && (have.weight - want.weight).abs() <= 50
        });
        if !found {
            problems.push(format!(
                "GDI does not see \"{}\" weight {}{}",
                want.family,
                want.weight,
                if want.italic { " italic" } else { "" }
            ));
        }
    }
    for want in expected_dw {
        let found = got_dw.iter().any(|have| {
            have.family.eq_ignore_ascii_case(&want.family)
                && have.face.eq_ignore_ascii_case(&want.face)
                && have.weight == want.weight
                && have.style == want.style
        });
        if !found {
            problems.push(format!(
                "DirectWrite does not see \"{} {}\"",
                want.family, want.face
            ));
        }
    }
    problems
}
