//! Volume, network and battery readings for the bar's status icons.

pub use super::audio::{set_volume, toggle_mute, volume, Volume};
use windows::Win32::Networking::NetworkListManager::{
    INetworkListManager, NetworkListManager, NLM_CONNECTIVITY_IPV4_INTERNET,
    NLM_CONNECTIVITY_IPV6_INTERNET,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Network {
    Offline,
    Local,
    Internet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Battery {
    pub percent: u8,
    pub charging: bool,
}

/// COM must be initialised on whichever thread calls us. S_FALSE (already initialised) and
/// RPC_E_CHANGED_MODE (thread already MTA) both still leave COM usable, so the result is ignored.
fn init_com() {
    // SAFETY: plain FFI call with no pointers.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
}

/// Connectivity as the Windows network list manager sees it. It is a cached local answer, not a
/// probe, so it does not block on the network.
pub fn network() -> Network {
    init_com();
    // SAFETY: COM calls with valid arguments.
    unsafe {
        let Ok(nlm) =
            CoCreateInstance::<_, INetworkListManager>(&NetworkListManager, None, CLSCTX_ALL)
        else {
            return Network::Offline;
        };
        let Ok(c) = nlm.GetConnectivity() else {
            return Network::Offline;
        };
        let internet = NLM_CONNECTIVITY_IPV4_INTERNET.0 | NLM_CONNECTIVITY_IPV6_INTERNET.0;
        if c.0 & internet != 0 {
            Network::Internet
        } else if c.0 != 0 {
            Network::Local
        } else {
            Network::Offline
        }
    }
}

/// `None` on machines without a battery (flag 128) or when the state is unknown (255).
pub fn battery() -> Option<Battery> {
    let mut s = SYSTEM_POWER_STATUS::default();
    // SAFETY: `s` is a valid out-struct.
    unsafe { GetSystemPowerStatus(&mut s).ok()? };
    if s.BatteryFlag == 128 || s.BatteryFlag == 255 || s.BatteryLifePercent > 100 {
        return None;
    }
    Some(Battery { percent: s.BatteryLifePercent, charging: s.BatteryFlag & 8 != 0 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readings_do_not_panic() {
        let _ = network();
        let _ = battery();
    }
}
