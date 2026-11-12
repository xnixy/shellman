//! The machine's totals that are not counters: memory, counts, uptime, the CPU's rated speed, the
//! GPUs' names and memory sizes, and the network adapters that are up.

use super::Memory;
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE};
use windows::Win32::NetworkManagement::IpHelper::{
    GetAdaptersAddresses, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER, GAA_FLAG_SKIP_MULTICAST,
    IP_ADAPTER_ADDRESSES_LH,
};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows::Win32::Networking::WinSock::{AF_INET, AF_UNSPEC, SOCKADDR_IN};
use windows::Win32::System::Power::{CallNtPowerInformation, ProcessorInformation, PROCESSOR_POWER_INFORMATION};
use windows::Win32::System::ProcessStatus::{GetPerformanceInfo, PERFORMANCE_INFORMATION};
use windows::Win32::System::SystemInformation::{GetTickCount64, GlobalMemoryStatusEx, MEMORYSTATUSEX};

/// Memory, and the process, thread and handle counts.
pub fn memory() -> (Memory, [u32; 3]) {
    let mut m = Memory::default();
    let mut status = MEMORYSTATUSEX { dwLength: size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
    if unsafe { GlobalMemoryStatusEx(&mut status) }.is_ok() {
        m.total = status.ullTotalPhys;
        m.available = status.ullAvailPhys;
    }
    let mut p = PERFORMANCE_INFORMATION { cb: size_of::<PERFORMANCE_INFORMATION>() as u32, ..Default::default() };
    let mut counts = [0; 3];
    if unsafe { GetPerformanceInfo(&mut p, p.cb) }.is_ok() {
        let page = p.PageSize as u64;
        m.committed = p.CommitTotal as u64 * page;
        m.commit_limit = p.CommitLimit as u64 * page;
        m.cached = p.SystemCache as u64 * page;
        counts = [p.ProcessCount, p.ThreadCount, p.HandleCount];
    }
    (m, counts)
}

pub fn uptime_secs() -> u64 {
    let ms = unsafe { GetTickCount64() };
    ms / 1000
}

/// The CPU's rated speed in MHz (the first core's), which `% Processor Performance` scales.
pub fn rated_mhz(cores: usize) -> Option<f32> {
    let mut info = vec![PROCESSOR_POWER_INFORMATION::default(); cores.max(1)];
    let bytes = (info.len() * size_of::<PROCESSOR_POWER_INFORMATION>()) as u32;
    let status = unsafe {
        CallNtPowerInformation(ProcessorInformation, None, 0, Some(info.as_mut_ptr().cast()), bytes)
    };
    (status.is_ok() && info[0].MaxMhz > 0).then(|| info[0].MaxMhz as f32)
}

/// The CPU's name, as the firmware gives it: "AMD Ryzen 9 9950X3D 16-Core Processor".
pub fn cpu_name() -> Option<String> {
    use windows::core::w;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
    let mut buf = [0u16; 128];
    let mut size = (buf.len() * 2) as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            w!(r"HARDWARE\DESCRIPTION\System\CentralProcessor\0"),
            w!("ProcessorNameString"),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if status.is_err() {
        return None;
    }
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..end]).trim().to_owned())
}

/// A GPU as DXGI knows it.
#[derive(Clone, Debug)]
pub struct Adapter {
    pub luid: u64,
    pub name: String,
    pub dedicated: u64,
    pub shared: u64,
}

/// The hardware GPUs (the software renderer left out).
pub fn adapters() -> Vec<Adapter> {
    let Ok(factory) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else {
        return Vec::new();
    };
    let mut out: Vec<Adapter> = Vec::new();
    for i in 0.. {
        let Ok(adapter) = (unsafe { factory.EnumAdapters1(i) }) else {
            break;
        };
        let Ok(desc) = (unsafe { adapter.GetDesc1() }) else {
            continue;
        };
        if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
            continue;
        }
        let luid = super::gpu::luid_value(desc.AdapterLuid.HighPart, desc.AdapterLuid.LowPart);
        if out.iter().any(|a| a.luid == luid) {
            continue;
        }
        let end = desc.Description.iter().position(|&c| c == 0).unwrap_or(desc.Description.len());
        out.push(Adapter {
            luid,
            name: String::from_utf16_lossy(&desc.Description[..end]),
            dedicated: desc.DedicatedVideoMemory as u64,
            shared: desc.SharedSystemMemory as u64,
        });
    }
    out
}

/// A network adapter that is up.
#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    pub name: String,
    pub description: String,
    pub address: String,
}

/// The adapters that are up, loopback and tunnels left out.
pub fn links() -> Vec<Link> {
    const LOOPBACK: u32 = 24;
    const TUNNEL: u32 = 131;
    let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
    let mut size = 32 * 1024u32;
    let mut buf: Vec<u64> = Vec::new();
    for _ in 0..3 {
        buf = vec![0; size as usize / 8 + 1];
        let status = unsafe {
            GetAdaptersAddresses(AF_UNSPEC.0 as u32, flags, None, Some(buf.as_mut_ptr().cast()), &mut size)
        };
        match status {
            0 => break,
            // ERROR_BUFFER_OVERFLOW: `size` now says what it needs.
            111 => continue,
            _ => return Vec::new(),
        }
    }
    let mut out = Vec::new();
    let mut at = buf.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
    while !at.is_null() {
        // SAFETY: a linked list inside `buf`, filled by the call.
        let a = unsafe { &*at };
        at = a.Next;
        if a.OperStatus != IfOperStatusUp || a.IfType == LOOPBACK || a.IfType == TUNNEL {
            continue;
        }
        let text = |p: windows::core::PWSTR| unsafe { p.to_string() }.unwrap_or_default();
        let mut address = String::new();
        let mut u = a.FirstUnicastAddress;
        while !u.is_null() {
            let ua = unsafe { &*u };
            u = ua.Next;
            let sa = ua.Address.lpSockaddr;
            if !sa.is_null() && unsafe { (*sa).sa_family } == AF_INET {
                let v4 = unsafe { &*(sa as *const SOCKADDR_IN) };
                let b = unsafe { v4.sin_addr.S_un.S_un_b };
                address = format!("{}.{}.{}.{}", b.s_b1, b.s_b2, b.s_b3, b.s_b4);
                break;
            }
        }
        out.push(Link { name: text(a.FriendlyName), description: text(a.Description), address });
    }
    out
}

/// An adapter's description the way the `Network Interface` counters name it: PDH swaps the
/// characters its paths reserve (`(` `)` `#` `/` `\`).
pub fn counter_name(description: &str) -> String {
    description
        .chars()
        .map(|c| match c {
            '(' => '[',
            ')' => ']',
            '#' | '/' | '\\' => '_',
            c => c,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counter_names_swap_reserved_characters() {
        assert_eq!(
            counter_name("Intel(R) Ethernet Controller #2 I225-V/x"),
            "Intel[R] Ethernet Controller _2 I225-V_x"
        );
    }

    #[test]
    fn the_machine_is_read() {
        let (m, counts) = memory();
        assert!(m.total > 0 && m.available <= m.total && m.committed > 0);
        assert!(counts.iter().all(|&c| c > 0));
        assert!(uptime_secs() > 0);
    }
}
