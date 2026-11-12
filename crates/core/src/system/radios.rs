//! Connectivity, followed live: the Wi-Fi and Bluetooth radios (on, off, or absent), the Wi-Fi
//! network's name, the network the machine is on, and Focus.
//!
//! Radios come from `Windows.Devices.Radios`. A desktop without a Wi-Fi or Bluetooth adapter
//! simply has no such radio, which reads as `None`. Everything is heard through events (a
//! radio's state, WLAN connection changes, network list changes, Focus); the worker sleeps in
//! between. WinRT's async calls are waited on here, on the worker, never on the UI thread.

use super::live::{self, Mailbox, Wake};
use super::netlist::NetList;
use super::status::Network;
use super::wifi::Wlan;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use windows::core::IInspectable;
use windows::Devices::Radios::{Radio, RadioAccessStatus, RadioKind, RadioState};
use windows::Foundation::TypedEventHandler;
use windows::UI::Shell::FocusSessionManager;

/// A radio's switch, as the control centre shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Switch {
    On,
    Off,
    /// Off in hardware (a key or switch on the machine) or by policy: it cannot be turned on
    /// from here.
    Disabled,
}

impl Switch {
    pub fn is_on(self) -> bool {
        self == Switch::On
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    WiFi,
    Bluetooth,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Connectivity {
    /// None when the machine has no Wi-Fi radio (or Windows would not let us see it).
    pub wifi: Option<Switch>,
    /// The connected Wi-Fi network's name.
    pub wifi_name: Option<String>,
    pub bluetooth: Option<Switch>,
    pub network: Option<Network>,
    /// The connected network's profile name.
    pub network_name: Option<String>,
    /// Whether Focus is on; None where Windows offers no Focus to apps.
    pub focus: Option<bool>,
}

enum Cmd {
    Refresh,
    Set(Kind, bool),
    Focus(bool),
    Stop,
}

pub struct ConnectivityWatch {
    tx: Sender<Cmd>,
    mailbox: Arc<Mailbox<Connectivity>>,
}

impl ConnectivityWatch {
    pub fn start(wake: Wake) -> Self {
        let (tx, rx) = channel();
        let mailbox = Arc::new(Mailbox::default());
        let (to_self, out) = (tx.clone(), mailbox.clone());
        live::spawn("connectivity-watch", move || run(rx, to_self, out, wake));
        let _ = tx.send(Cmd::Refresh);
        Self { tx, mailbox }
    }

    pub fn take(&self) -> Option<Connectivity> {
        self.mailbox.take()
    }

    /// Turn a radio on or off. Answered by the radio's own state-changed event.
    pub fn set(&self, kind: Kind, on: bool) {
        let _ = self.tx.send(Cmd::Set(kind, on));
    }

    pub fn set_focus(&self, on: bool) {
        let _ = self.tx.send(Cmd::Focus(on));
    }
}

impl Drop for ConnectivityWatch {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Stop);
    }
}

/// What the worker holds while it runs, with the event registrations to undo.
struct Sources {
    radios: Vec<(Radio, i64)>,
    wlan: Option<Wlan>,
    netlist: Option<NetList>,
    focus: Option<(FocusSessionManager, i64)>,
}

impl Sources {
    fn open(tx: &Sender<Cmd>) -> Self {
        let refresh = |tx: &Sender<Cmd>| {
            let tx = tx.clone();
            move || {
                let _ = tx.send(Cmd::Refresh);
            }
        };
        let radios = radios()
            .into_iter()
            .filter_map(|radio| {
                let on_change = refresh(tx);
                let handler = TypedEventHandler::<Radio, IInspectable>::new(move |_, _| {
                    on_change();
                    Ok(())
                });
                let token = radio.StateChanged(&handler).ok()?;
                Some((radio, token))
            })
            .collect();
        let wlan = Wlan::open().map(|mut w| {
            w.listen(Box::new(refresh(tx)));
            w
        });
        let netlist = NetList::open().map(|mut n| {
            n.listen(Box::new(refresh(tx)));
            n
        });
        let focus = focus_manager().and_then(|m| {
            let on_change = refresh(tx);
            let handler = TypedEventHandler::<FocusSessionManager, IInspectable>::new(move |_, _| {
                on_change();
                Ok(())
            });
            let token = m.IsFocusActiveChanged(&handler).ok()?;
            Some((m, token))
        });
        Self { radios, wlan, netlist, focus }
    }

    fn read(&self) -> Connectivity {
        let switch = |kind: RadioKind| {
            self.radios
                .iter()
                .find(|(r, _)| r.Kind().ok() == Some(kind))
                .map(|(r, _)| to_switch(r.State().unwrap_or(RadioState::Unknown)))
        };
        Connectivity {
            wifi: switch(RadioKind::WiFi),
            wifi_name: self.wlan.as_ref().and_then(Wlan::network_name),
            bluetooth: switch(RadioKind::Bluetooth),
            network: self.netlist.as_ref().map(NetList::connectivity),
            network_name: self.netlist.as_ref().and_then(NetList::network_name),
            focus: self.focus.as_ref().and_then(|(m, _)| m.IsFocusActive().ok()),
        }
    }

    fn set(&self, kind: Kind, on: bool) {
        let want = match kind {
            Kind::WiFi => RadioKind::WiFi,
            Kind::Bluetooth => RadioKind::Bluetooth,
        };
        let state = if on { RadioState::On } else { RadioState::Off };
        for (radio, _) in self.radios.iter().filter(|(r, _)| r.Kind().ok() == Some(want)) {
            // Waits here on the worker; the state-changed event then refreshes the panel.
            let _ = radio.SetStateAsync(state).and_then(|op| op.get());
        }
    }

    fn set_focus(&self, on: bool) {
        if let Some((m, _)) = &self.focus {
            if on {
                let _ = m.TryStartFocusSession();
            } else {
                let _ = m.DeactivateFocus();
            }
        }
    }

    fn close(self) {
        for (radio, token) in self.radios {
            let _ = radio.RemoveStateChanged(token);
        }
        if let Some((m, token)) = self.focus {
            let _ = m.RemoveIsFocusActiveChanged(token);
        }
    }
}

fn run(rx: Receiver<Cmd>, to_self: Sender<Cmd>, out: Arc<Mailbox<Connectivity>>, wake: Wake) {
    live::init_mta();
    let sources = Sources::open(&to_self);
    drop(to_self);
    while let Ok(first) = rx.recv() {
        let mut batch = vec![first];
        batch.extend(rx.try_iter());
        if batch.iter().any(|c| matches!(c, Cmd::Stop)) {
            break;
        }
        for cmd in &batch {
            match *cmd {
                Cmd::Set(kind, on) => sources.set(kind, on),
                Cmd::Focus(on) => sources.set_focus(on),
                _ => {}
            }
        }
        out.put(sources.read());
        wake();
    }
    sources.close();
}

/// Every radio Windows lets this app see. Unpackaged desktop apps are normally allowed; if
/// access is refused, there are simply no radios to show.
fn radios() -> Vec<Radio> {
    let allowed = Radio::RequestAccessAsync()
        .and_then(|op| op.get())
        .is_ok_and(|a| a == RadioAccessStatus::Allowed);
    if !allowed {
        return Vec::new();
    }
    Radio::GetRadiosAsync()
        .and_then(|op| op.get())
        .map(|list| list.into_iter().collect())
        .unwrap_or_default()
}

fn to_switch(state: RadioState) -> Switch {
    match state {
        RadioState::On => Switch::On,
        RadioState::Off => Switch::Off,
        _ => Switch::Disabled,
    }
}

/// Windows 11's Focus, where Windows offers it to apps and something answers for it. With
/// Explorer stopped the manager is there but its state is not ("class not registered"), so it
/// counts as absent.
fn focus_manager() -> Option<FocusSessionManager> {
    if !FocusSessionManager::IsSupported().unwrap_or(false) {
        return None;
    }
    FocusSessionManager::GetDefault().ok().filter(|m| m.IsFocusActive().is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn radio_states_map_to_switches() {
        assert_eq!(to_switch(RadioState::On), Switch::On);
        assert_eq!(to_switch(RadioState::Off), Switch::Off);
        assert_eq!(to_switch(RadioState::Disabled), Switch::Disabled);
        assert_eq!(to_switch(RadioState::Unknown), Switch::Disabled);
        assert!(Switch::On.is_on() && !Switch::Off.is_on());
    }

    /// What this machine has; run with `--ignored --nocapture` to see it. Reads only.
    #[test]
    #[ignore]
    fn probe() {
        live::init_mta();
        let (tx, _rx) = channel();
        let sources = Sources::open(&tx);
        for (r, _) in &sources.radios {
            println!("radio {:?} {:?} {:?}", r.Name(), r.Kind(), r.State());
        }
        println!("{:?}", sources.read());
        println!("focus supported: {:?}", FocusSessionManager::IsSupported());
        let m = FocusSessionManager::GetDefault();
        println!("focus manager: {:?}", m.as_ref().map(|_| ()));
        if let Ok(m) = m {
            println!("focus active: {:?}", m.IsFocusActive());
            let h = TypedEventHandler::<FocusSessionManager, IInspectable>::new(|_, _| Ok(()));
            println!("focus event: {:?}", m.IsFocusActiveChanged(&h));
        }
        sources.close();
    }
}
