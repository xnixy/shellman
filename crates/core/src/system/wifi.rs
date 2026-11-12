//! The Wi-Fi network this machine is connected to, through the WLAN service.
//!
//! On a machine without a Wi-Fi adapter the service may not run at all; then there is simply no
//! network name, never an error.

use windows::Win32::Foundation::HANDLE;
use windows::Win32::NetworkManagement::WiFi::{
    wlan_interface_state_connected, wlan_intf_opcode_current_connection, WlanCloseHandle,
    WlanEnumInterfaces, WlanFreeMemory, WlanOpenHandle, WlanQueryInterface,
    WlanRegisterNotification, L2_NOTIFICATION_DATA, WLAN_CONNECTION_ATTRIBUTES,
    WLAN_INTERFACE_INFO_LIST, WLAN_NOTIFICATION_SOURCE_ACM, WLAN_NOTIFICATION_SOURCE_NONE,
};

/// An open WLAN client handle, closed on drop. While `on_change` is registered it is called
/// (on a WLAN service thread) whenever a connection starts, ends or fails.
pub struct Wlan {
    handle: HANDLE,
    /// The callback's context, owned here and freed after the handle is closed.
    context: Option<*mut Box<dyn Fn() + Send + Sync>>,
}

// SAFETY: the handle is a plain token; the context is only touched by the WLAN service's
// threads (read-only) and by `drop`, after the service has stopped calling.
unsafe impl Send for Wlan {}

impl Wlan {
    /// None when the WLAN service is not running (typically: no Wi-Fi adapter).
    pub fn open() -> Option<Self> {
        let (mut version, mut handle) = (0u32, HANDLE::default());
        // SAFETY: valid out-pointers; version 2 is Vista and later.
        let status = unsafe { WlanOpenHandle(2, None, &mut version, &mut handle) };
        (status == 0).then_some(Self { handle, context: None })
    }

    /// Call `on_change` whenever the connection changes, until this handle is dropped.
    pub fn listen(&mut self, on_change: Box<dyn Fn() + Send + Sync>) {
        if self.context.is_some() {
            return;
        }
        let context = Box::into_raw(Box::new(on_change));
        // SAFETY: the context stays alive until the handle is closed in `drop`.
        let status = unsafe {
            WlanRegisterNotification(
                self.handle,
                WLAN_NOTIFICATION_SOURCE_ACM,
                true,
                Some(notified),
                Some(context as *const _),
                None,
                None,
            )
        };
        if status == 0 {
            self.context = Some(context);
        } else {
            // SAFETY: not registered, so nothing else holds it.
            drop(unsafe { Box::from_raw(context) });
        }
    }

    /// The SSID of the first connected Wi-Fi interface.
    pub fn network_name(&self) -> Option<String> {
        // SAFETY: lists and attributes are allocated by the service and freed with
        // WlanFreeMemory; lengths are bounded by the structures' own arrays.
        unsafe {
            let mut list: *mut WLAN_INTERFACE_INFO_LIST = std::ptr::null_mut();
            if WlanEnumInterfaces(self.handle, None, &mut list) != 0 || list.is_null() {
                return None;
            }
            let count = (*list).dwNumberOfItems as usize;
            let first = (*list).InterfaceInfo.as_ptr();
            let mut name = None;
            for i in 0..count {
                let info = &*first.add(i);
                if info.isState != wlan_interface_state_connected {
                    continue;
                }
                let mut size = 0u32;
                let mut data: *mut core::ffi::c_void = std::ptr::null_mut();
                let status = WlanQueryInterface(
                    self.handle,
                    &info.InterfaceGuid,
                    wlan_intf_opcode_current_connection,
                    None,
                    &mut size,
                    &mut data,
                    None,
                );
                if status != 0 || data.is_null() {
                    continue;
                }
                let attrs = &*(data as *const WLAN_CONNECTION_ATTRIBUTES);
                let ssid = &attrs.wlanAssociationAttributes.dot11Ssid;
                let len = (ssid.uSSIDLength as usize).min(ssid.ucSSID.len());
                name = Some(String::from_utf8_lossy(&ssid.ucSSID[..len]).into_owned());
                WlanFreeMemory(data);
                if name.as_deref().is_some_and(|n| !n.is_empty()) {
                    break;
                }
            }
            WlanFreeMemory(list as *const _);
            name.filter(|n| !n.is_empty())
        }
    }
}

impl Drop for Wlan {
    fn drop(&mut self) {
        // SAFETY: unregister first, then close (which waits for running callbacks), and only
        // then free the context.
        unsafe {
            if self.context.is_some() {
                let _ = WlanRegisterNotification(
                    self.handle,
                    WLAN_NOTIFICATION_SOURCE_NONE,
                    true,
                    None,
                    None,
                    None,
                    None,
                );
            }
            let _ = WlanCloseHandle(self.handle, None);
            if let Some(context) = self.context.take() {
                drop(Box::from_raw(context));
            }
        }
    }
}

unsafe extern "system" fn notified(_data: *mut L2_NOTIFICATION_DATA, context: *mut core::ffi::c_void) {
    if let Some(on_change) = (context as *const Box<dyn Fn() + Send + Sync>).as_ref() {
        on_change();
    }
}
