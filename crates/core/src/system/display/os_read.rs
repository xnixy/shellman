//! Reading the display configuration from Windows: the active paths and modes (kept whole so a
//! change can be taken back), and everything the page shows about each monitor.

use super::geom::Rect;
use super::modes::{self, Mode};
use super::page::{Hdr, Monitor, Orientation, Topology};
use super::scale::Scale;
use crate::system::display_names::wide_str;
use windows::core::PCWSTR;
use windows::Win32::Devices::Display::*;
use windows::Win32::Foundation::{ERROR_SUCCESS, LUID};
use windows::Win32::Graphics::Gdi::{EnumDisplaySettingsExW, DEVMODEW, ENUM_CURRENT_SETTINGS, ENUM_DISPLAY_SETTINGS_FLAGS};

/// The whole active configuration, as QueryDisplayConfig returned it.
#[derive(Clone)]
pub struct Snapshot {
    pub paths: Vec<DISPLAYCONFIG_PATH_INFO>,
    pub modes: Vec<DISPLAYCONFIG_MODE_INFO>,
}

/// The active configuration and the topology the database says is current.
pub fn query() -> Option<(Snapshot, DISPLAYCONFIG_TOPOLOGY_ID)> {
    // SAFETY: arrays are sized by GetDisplayConfigBufferSizes and the counts written back; the
    // database query reports the topology too.
    unsafe {
        let (mut np, mut nm) = (0u32, 0u32);
        if GetDisplayConfigBufferSizes(QDC_DATABASE_CURRENT, &mut np, &mut nm) != ERROR_SUCCESS {
            return None;
        }
        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); np as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); nm as usize];
        let mut topo = DISPLAYCONFIG_TOPOLOGY_ID(0);
        // The database call keeps the topology; the active call is what a restore needs.
        let ok = QueryDisplayConfig(QDC_DATABASE_CURRENT, &mut np, paths.as_mut_ptr(), &mut nm, modes.as_mut_ptr(), Some(&mut topo));
        if ok != ERROR_SUCCESS {
            return None;
        }
        let (mut np, mut nm) = (0u32, 0u32);
        if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut np, &mut nm) != ERROR_SUCCESS {
            return None;
        }
        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); np as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); nm as usize];
        if QueryDisplayConfig(QDC_ONLY_ACTIVE_PATHS, &mut np, paths.as_mut_ptr(), &mut nm, modes.as_mut_ptr(), None) != ERROR_SUCCESS {
            return None;
        }
        paths.truncate(np as usize);
        modes.truncate(nm as usize);
        Some((Snapshot { paths, modes }, topo))
    }
}

pub fn topology_of(id: DISPLAYCONFIG_TOPOLOGY_ID) -> Topology {
    if id == DISPLAYCONFIG_TOPOLOGY_EXTEND {
        Topology::Extend
    } else if id == DISPLAYCONFIG_TOPOLOGY_CLONE {
        Topology::Duplicate
    } else {
        Topology::Other
    }
}

fn header(kind: DISPLAYCONFIG_DEVICE_INFO_TYPE, size: usize, adapter: LUID, id: u32) -> DISPLAYCONFIG_DEVICE_INFO_HEADER {
    DISPLAYCONFIG_DEVICE_INFO_HEADER { r#type: kind, size: size as u32, adapterId: adapter, id }
}

pub fn source_name(p: &DISPLAYCONFIG_PATH_INFO) -> Option<String> {
    let mut s = DISPLAYCONFIG_SOURCE_DEVICE_NAME::default();
    s.header = header(DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, size_of_val(&s), p.sourceInfo.adapterId, p.sourceInfo.id);
    // SAFETY: the packet carries its own size.
    (unsafe { DisplayConfigGetDeviceInfo(&mut s.header) } == 0).then(|| wide_str(&s.viewGdiDeviceName))
}

fn target_name(p: &DISPLAYCONFIG_PATH_INFO) -> Option<String> {
    let mut t = DISPLAYCONFIG_TARGET_DEVICE_NAME::default();
    t.header = header(DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME, size_of_val(&t), p.targetInfo.adapterId, p.targetInfo.id);
    // SAFETY: as above.
    let ok = unsafe { DisplayConfigGetDeviceInfo(&mut t.header) } == 0;
    ok.then(|| wide_str(&t.monitorFriendlyDeviceName)).filter(|n| !n.is_empty())
}

fn preferred(p: &DISPLAYCONFIG_PATH_INFO) -> Option<(u32, u32)> {
    let mut m = DISPLAYCONFIG_TARGET_PREFERRED_MODE::default();
    m.header = header(DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_PREFERRED_MODE, size_of_val(&m), p.targetInfo.adapterId, p.targetInfo.id);
    // SAFETY: as above.
    let ok = unsafe { DisplayConfigGetDeviceInfo(&mut m.header) } == 0;
    ok.then_some((m.width, m.height)).filter(|(w, h)| *w > 0 && *h > 0)
}

/// Advanced colour (HDR): documented, DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO. Bit 0 is
/// "supported", bit 1 "enabled"; a monitor that cannot do it reports no state.
fn hdr(p: &DISPLAYCONFIG_PATH_INFO) -> Option<Hdr> {
    let mut a = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO::default();
    a.header = header(DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO, size_of_val(&a), p.targetInfo.adapterId, p.targetInfo.id);
    // SAFETY: as above; the union is plain bits.
    unsafe {
        if DisplayConfigGetDeviceInfo(&mut a.header) != 0 {
            return None;
        }
        let v = a.Anonymous.value;
        (v & 1 != 0).then_some(Hdr { enabled: v & 2 != 0 })
    }
}

/// Custom type -3, see `scale`.
#[repr(C)]
struct ScaleGet {
    header: DISPLAYCONFIG_DEVICE_INFO_HEADER,
    min: i32,
    cur: i32,
    max: i32,
}

pub fn scale(p: &DISPLAYCONFIG_PATH_INFO) -> Option<Scale> {
    let mut g = ScaleGet { header: header(DISPLAYCONFIG_DEVICE_INFO_TYPE(-3), size_of::<ScaleGet>(), p.sourceInfo.adapterId, p.sourceInfo.id), min: 0, cur: 0, max: 0 };
    // SAFETY: the packet is the documented-by-convention layout and carries its own size.
    let ok = unsafe { DisplayConfigGetDeviceInfo(&mut g.header) } == 0;
    let s = Scale { min: g.min, cur: g.cur, max: g.max };
    (ok && s.sane()).then_some(s)
}

fn devmode(device: &[u16], index: windows::Win32::Graphics::Gdi::ENUM_DISPLAY_SETTINGS_MODE) -> Option<DEVMODEW> {
    let mut dm = DEVMODEW { dmSize: size_of::<DEVMODEW>() as u16, ..Default::default() };
    // SAFETY: a sized DEVMODEW and a NUL-terminated device name.
    unsafe { EnumDisplaySettingsExW(PCWSTR(device.as_ptr()), index, &mut dm, ENUM_DISPLAY_SETTINGS_FLAGS(0)) }
        .as_bool()
        .then_some(dm)
}

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn supported_modes(device: &[u16]) -> Vec<Mode> {
    let mut out = Vec::new();
    let mut i = 0u32;
    while let Some(dm) = devmode(device, windows::Win32::Graphics::Gdi::ENUM_DISPLAY_SETTINGS_MODE(i)) {
        // 32 bit colour only: lower depths repeat every resolution.
        if dm.dmBitsPerPel == 32 {
            out.push(Mode { w: dm.dmPelsWidth, h: dm.dmPelsHeight, hz: dm.dmDisplayFrequency });
        }
        i += 1;
    }
    out
}

/// Every active monitor, numbered by its GDI name, with what the page shows for it.
pub fn monitors() -> Option<(Vec<Monitor>, Topology, Snapshot)> {
    let (snap, topo) = query()?;
    let mut out: Vec<Monitor> = Vec::new();
    for p in &snap.paths {
        let Some(id) = source_name(p) else { continue };
        // Duplicated screens share a source: listed once.
        if out.iter().any(|m| m.id == id) {
            continue;
        }
        let wid = wide(&id);
        let Some(cur) = devmode(&wid, ENUM_CURRENT_SETTINGS) else { continue };
        // SAFETY: a display DEVMODE carries position and orientation in this arm of the union.
        let (pos, dmdo) = unsafe { (cur.Anonymous1.Anonymous2.dmPosition, cur.Anonymous1.Anonymous2.dmDisplayOrientation.0) };
        let orientation = Orientation::from_dmdo(dmdo);
        let number = id.trim_start_matches(|c: char| !c.is_ascii_digit()).parse().unwrap_or(0);
        let mut pref = preferred(p);
        if let (Some((w, h)), true) = (pref, orientation.is_portrait()) {
            pref = Some((h, w));
        }
        out.push(Monitor {
            number,
            name: target_name(p).unwrap_or_else(|| format!("Display {number}")),
            rect: Rect::new(pos.x, pos.y, cur.dmPelsWidth as i32, cur.dmPelsHeight as i32),
            orientation,
            hz: cur.dmDisplayFrequency,
            primary: pos.x == 0 && pos.y == 0,
            groups: modes::group(&supported_modes(&wid), pref),
            scale: scale(p),
            hdr: hdr(p),
            id,
        });
    }
    out.sort_by_key(|m| m.number);
    Some((out, topology_of(topo), snap))
}

/// Find a monitor's path by its GDI name.
pub fn path_of<'a>(snap: &'a Snapshot, id: &str) -> Option<&'a DISPLAYCONFIG_PATH_INFO> {
    snap.paths.iter().find(|p| source_name(p).as_deref() == Some(id))
}
