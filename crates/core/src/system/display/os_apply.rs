//! Changing the configuration. Modes, orientation and positions go through ChangeDisplaySettingsEx
//! (each device staged with CDS_UPDATEREGISTRY | CDS_NORESET, then one final call applies them
//! all together); the layout of a plan is checked first with CDS_TEST and, when only positions
//! move, SetDisplayConfig(SDC_VALIDATE). Going back uses the saved QueryDisplayConfig arrays.

use super::os_read::{self, wide, Snapshot};
use super::page::{Change, Monitor, Topology};
use windows::core::PCWSTR;
use windows::Win32::Devices::Display::*;
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::Graphics::Gdi::*;

fn devmode(c: &Change) -> DEVMODEW {
    let mut dm = DEVMODEW { dmSize: size_of::<DEVMODEW>() as u16, ..Default::default() };
    dm.dmFields = DM_POSITION | DM_PELSWIDTH | DM_PELSHEIGHT | DM_DISPLAYFREQUENCY | DM_DISPLAYORIENTATION;
    dm.dmPelsWidth = c.w;
    dm.dmPelsHeight = c.h;
    dm.dmDisplayFrequency = c.hz;
    // The display arm of the union.
    dm.Anonymous1.Anonymous2.dmPosition.x = c.x;
    dm.Anonymous1.Anonymous2.dmPosition.y = c.y;
    dm.Anonymous1.Anonymous2.dmDisplayOrientation = DEVMODE_DISPLAY_ORIENTATION(c.orientation.dmdo());
    dm
}

fn cds(device: Option<&str>, dm: Option<&DEVMODEW>, flags: CDS_TYPE) -> DISP_CHANGE {
    let name = device.map(wide);
    let name = name.as_ref().map_or(PCWSTR::null(), |n| PCWSTR(n.as_ptr()));
    // SAFETY: a NUL-terminated name (or null for "all") and a DEVMODEW that lives through the call.
    unsafe { ChangeDisplaySettingsExW(name, dm.map(|d| d as *const _), None, flags, None) }
}

fn why(code: DISP_CHANGE) -> &'static str {
    match code.0 {
        -1 => "the display driver could not be told",
        -2 => "the mode is not supported",
        -3 => "the settings are invalid",
        -4 => "the display driver failed",
        -5 => "a restart is needed first",
        -6 => "the flags are invalid",
        _ => "Windows refused it",
    }
}

/// Ask Windows whether each changed device takes its mode; refuse with its reason if not.
/// Position-only changes are checked as a whole with SDC_VALIDATE.
pub fn validate(monitors: &[Monitor], plan: &[Change], snap: &Snapshot) -> Result<(), String> {
    let changed: Vec<(&Monitor, &Change)> = monitors.iter().zip(plan).filter(|(m, c)| c.differs_from(m)).collect();
    if changed.is_empty() {
        return Err("Nothing to apply.".into());
    }
    let mut positions_only = true;
    for (m, c) in &changed {
        if (c.w, c.h, c.hz, c.orientation) != (m.rect.w as u32, m.rect.h as u32, m.hz, m.orientation) {
            positions_only = false;
            let r = cds(Some(&c.id), Some(&devmode(c)), CDS_TEST);
            if r != DISP_CHANGE_SUCCESSFUL {
                return Err(format!("{} cannot use that mode: {}.", m.name, why(r)));
            }
        }
    }
    if positions_only {
        let mut modes = snap.modes.clone();
        for (_, c) in &changed {
            let Some(p) = os_read::path_of(snap, &c.id) else { continue };
            // SAFETY: legacy (not virtual-mode-aware) paths index the source mode directly.
            let idx = unsafe { p.sourceInfo.Anonymous.modeInfoIdx } as usize;
            if let Some(mi) = modes.get_mut(idx).filter(|mi| mi.infoType == DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE) {
                // The type says this is a source mode.
                mi.Anonymous.sourceMode.position.x = c.x;
                mi.Anonymous.sourceMode.position.y = c.y;
            }
        }
        // SAFETY: arrays from QueryDisplayConfig; validate only, nothing changes.
        let r = unsafe { SetDisplayConfig(Some(&snap.paths), Some(&modes), SDC_VALIDATE | SDC_USE_SUPPLIED_DISPLAY_CONFIG | SDC_ALLOW_CHANGES) };
        if r != ERROR_SUCCESS.0 as i32 {
            return Err("Windows does not accept that arrangement.".into());
        }
    }
    Ok(())
}

/// Stage every changed device and apply them together. On any failure the old configuration is
/// put back before the error is returned.
pub fn apply(monitors: &[Monitor], plan: &[Change], snap: &Snapshot) -> Result<(), String> {
    let mut changed: Vec<(&Monitor, &Change)> = monitors.iter().zip(plan).filter(|(m, c)| c.differs_from(m)).collect();
    // The new primary first: it is the one that must sit at 0,0.
    let was_primary = monitors.iter().find(|m| m.primary).map(|m| m.id.as_str());
    let now_primary = plan.iter().find(|c| c.x == 0 && c.y == 0).map(|c| c.id.as_str());
    changed.sort_by_key(|(_, c)| Some(c.id.as_str()) != now_primary);
    let fail = |msg: String| {
        restore(snap);
        Err(msg)
    };
    for (m, c) in &changed {
        let mut flags = CDS_UPDATEREGISTRY | CDS_NORESET;
        if now_primary == Some(c.id.as_str()) && was_primary != now_primary {
            flags |= CDS_SET_PRIMARY;
        }
        let r = cds(Some(&c.id), Some(&devmode(c)), flags);
        if r != DISP_CHANGE_SUCCESSFUL {
            return fail(format!("{} could not be changed: {}.", m.name, why(r)));
        }
    }
    let r = cds(None, None, CDS_TYPE(0));
    if r != DISP_CHANGE_SUCCESSFUL {
        return fail(format!("The new settings could not be applied: {}.", why(r)));
    }
    Ok(())
}

/// Put a saved configuration back and remember it as the user's.
pub fn restore(snap: &Snapshot) -> bool {
    // SAFETY: arrays from QueryDisplayConfig.
    let r = unsafe {
        SetDisplayConfig(
            Some(&snap.paths),
            Some(&snap.modes),
            SDC_APPLY | SDC_USE_SUPPLIED_DISPLAY_CONFIG | SDC_ALLOW_CHANGES | SDC_SAVE_TO_DATABASE,
        )
    };
    r == ERROR_SUCCESS.0 as i32
}

fn topology_flag(t: Topology) -> Option<SET_DISPLAY_CONFIG_FLAGS> {
    match t {
        Topology::Extend => Some(SDC_TOPOLOGY_EXTEND),
        Topology::Duplicate => Some(SDC_TOPOLOGY_CLONE),
        Topology::Other => None,
    }
}

/// Extend or duplicate: checked with SDC_VALIDATE, then applied.
pub fn set_topology(t: Topology, apply: bool) -> Result<(), String> {
    let flag = topology_flag(t).ok_or("That layout is not offered.")?;
    let mode = if apply { SDC_APPLY } else { SDC_VALIDATE };
    // SAFETY: topology flags take no arrays.
    let r = unsafe { SetDisplayConfig(None, None, mode | flag) };
    if r == ERROR_SUCCESS.0 as i32 {
        Ok(())
    } else {
        Err("Windows does not accept that layout.".into())
    }
}

/// Custom type -4, see `scale`.
#[repr(C)]
struct ScaleSet {
    header: DISPLAYCONFIG_DEVICE_INFO_HEADER,
    rel: i32,
}

pub fn set_scale(snap: &Snapshot, id: &str, percent: u32) -> Result<(), String> {
    let p = os_read::path_of(snap, id).ok_or("That display is gone.")?;
    let s = os_read::scale(p).ok_or("This display's scale cannot be read.")?;
    let rel = s.rel_for(percent).ok_or("This display does not take that scale.")?;
    let set = ScaleSet {
        header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
            r#type: DISPLAYCONFIG_DEVICE_INFO_TYPE(-4),
            size: size_of::<ScaleSet>() as u32,
            adapterId: p.sourceInfo.adapterId,
            id: p.sourceInfo.id,
        },
        rel,
    };
    // SAFETY: the packet carries its own size.
    let r = unsafe { DisplayConfigSetDeviceInfo(&set.header) };
    if r == 0 {
        Ok(())
    } else {
        Err(format!("Windows refused the scale (error {r})."))
    }
}

/// HDR on or off: DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE, bit 0 is "enable".
pub fn set_hdr(snap: &Snapshot, id: &str, on: bool) -> Result<(), String> {
    let p = os_read::path_of(snap, id).ok_or("That display is gone.")?;
    let mut s = DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE::default();
    s.header = DISPLAYCONFIG_DEVICE_INFO_HEADER {
        r#type: DISPLAYCONFIG_DEVICE_INFO_SET_ADVANCED_COLOR_STATE,
        size: size_of::<DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE>() as u32,
        adapterId: p.targetInfo.adapterId,
        id: p.targetInfo.id,
    };
    s.Anonymous.value = on as u32;
    // SAFETY: the packet carries its own size.
    let r = unsafe { DisplayConfigSetDeviceInfo(&s.header) };
    if r == 0 {
        Ok(())
    } else {
        Err(format!("Windows refused the HDR change (error {r})."))
    }
}
