//! The network the machine is on, as the Windows network list manager sees it, and a sink that
//! hears when connectivity changes. Its answers are cached locally, never probes, so reading
//! them does not wait on the network.

use super::status::Network;
use windows::core::{implement, Interface, Result};
use windows::Win32::Networking::NetworkListManager::{
    INetwork, INetworkListManager, INetworkListManagerEvents, INetworkListManagerEvents_Impl,
    NetworkListManager, NLM_CONNECTIVITY, NLM_CONNECTIVITY_IPV4_INTERNET,
    NLM_CONNECTIVITY_IPV6_INTERNET, NLM_ENUM_NETWORK_CONNECTED,
};
use windows::Win32::System::Com::{
    CoCreateInstance, IConnectionPoint, IConnectionPointContainer, CLSCTX_ALL,
};

/// The network list manager, with an events sink attached while it lives.
pub struct NetList {
    nlm: INetworkListManager,
    advised: Option<(IConnectionPoint, u32)>,
}

impl NetList {
    pub fn open() -> Option<Self> {
        // SAFETY: COM call with valid arguments.
        let nlm = unsafe { CoCreateInstance(&NetworkListManager, None, CLSCTX_ALL).ok()? };
        Some(Self { nlm, advised: None })
    }

    /// Call `on_change` whenever connectivity changes, until this is dropped.
    pub fn listen(&mut self, on_change: Box<dyn Fn() + Send + Sync>) {
        let sink: INetworkListManagerEvents = Sink { on_change }.into();
        // SAFETY: COM calls with valid arguments; unadvised in `drop`.
        let advised = unsafe {
            self.nlm
                .cast::<IConnectionPointContainer>()
                .and_then(|c| c.FindConnectionPoint(&INetworkListManagerEvents::IID))
                .and_then(|point| point.Advise(&sink).map(|cookie| (point, cookie)))
        };
        self.advised = advised.ok();
    }

    pub fn connectivity(&self) -> Network {
        // SAFETY: valid interface.
        let Ok(c) = (unsafe { self.nlm.GetConnectivity() }) else {
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

    /// The name of the first connected network (its profile name, such as "Network 2" or a
    /// Wi-Fi network's name).
    pub fn network_name(&self) -> Option<String> {
        // SAFETY: COM calls with valid arguments.
        unsafe {
            let list = self.nlm.GetNetworks(NLM_ENUM_NETWORK_CONNECTED).ok()?;
            let mut one: [Option<INetwork>; 1] = [None];
            let mut fetched = 0u32;
            list.Next(&mut one, Some(&mut fetched)).ok()?;
            let network = one[0].take().filter(|_| fetched == 1)?;
            let name = network.GetName().ok()?.to_string();
            (!name.is_empty()).then_some(name)
        }
    }
}

impl Drop for NetList {
    fn drop(&mut self) {
        if let Some((point, cookie)) = self.advised.take() {
            // SAFETY: the cookie Advise returned.
            unsafe {
                let _ = point.Unadvise(cookie);
            }
        }
    }
}

#[implement(INetworkListManagerEvents)]
struct Sink {
    on_change: Box<dyn Fn() + Send + Sync>,
}

impl INetworkListManagerEvents_Impl for Sink_Impl {
    fn ConnectivityChanged(&self, _now: NLM_CONNECTIVITY) -> Result<()> {
        (self.on_change)();
        Ok(())
    }
}
