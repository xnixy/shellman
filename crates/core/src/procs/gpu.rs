//! GPU use from the `GPU Engine` counters, the way Task Manager adds them up.
//!
//! Each counter instance is one process on one engine of one adapter, named like
//! `pid_1234_luid_0x00000000_0x0000D1B8_phys_0_eng_3_engtype_VideoDecode`. An engine's use is the
//! sum over the processes on it; an adapter's is its busiest engine's; a process's is the busiest
//! engine it has work on.

use std::collections::HashMap;

/// One counter reading.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EngineUse {
    pub pid: u32,
    pub adapter: u64,
    pub engine: u32,
    pub percent: f32,
}

fn hex(s: &str) -> Option<u32> {
    u32::from_str_radix(s.strip_prefix("0x").or_else(|| s.strip_prefix("0X"))?, 16).ok()
}

/// An adapter's LUID from `luid_0xHIGH_0xLOW`, high part first, as one number.
fn luid(high: &str, low: &str) -> Option<u64> {
    Some(((hex(high)? as u64) << 32) | hex(low)? as u64)
}

/// The LUID as a number, from its two halves (as DXGI gives them).
pub fn luid_value(high: i32, low: u32) -> u64 {
    ((high as u32 as u64) << 32) | low as u64
}

/// The process, adapter and engine of a `GPU Engine` instance.
pub fn parse_engine(name: &str) -> Option<(u32, u64, u32)> {
    let parts: Vec<&str> = name.split('_').collect();
    let at = |key: &str| parts.iter().position(|p| *p == key);
    let pid = parts.get(at("pid")? + 1)?.parse().ok()?;
    let l = at("luid")?;
    let adapter = luid(parts.get(l + 1)?, parts.get(l + 2)?)?;
    let engine = parts.get(at("eng")? + 1)?.parse().ok()?;
    Some((pid, adapter, engine))
}

/// The adapter of a `GPU Adapter Memory` or `GPU Process Memory` instance.
pub fn parse_adapter(name: &str) -> Option<u64> {
    let parts: Vec<&str> = name.split('_').collect();
    let l = parts.iter().position(|p| *p == "luid")?;
    luid(parts.get(l + 1)?, parts.get(l + 2)?)
}

/// Each process's and each adapter's use, 0 to 100.
pub fn usage(readings: &[EngineUse]) -> (HashMap<u32, f32>, HashMap<u64, f32>) {
    // A process can have several counters on one engine (one per context); they add up.
    let mut per_process_engine: HashMap<(u32, u64, u32), f32> = HashMap::new();
    let mut per_engine: HashMap<(u64, u32), f32> = HashMap::new();
    for r in readings {
        let v = if r.percent.is_finite() { r.percent.max(0.0) } else { 0.0 };
        *per_process_engine.entry((r.pid, r.adapter, r.engine)).or_default() += v;
        *per_engine.entry((r.adapter, r.engine)).or_default() += v;
    }
    let mut processes: HashMap<u32, f32> = HashMap::new();
    for ((pid, _, _), v) in per_process_engine {
        let e = processes.entry(pid).or_default();
        *e = e.max(v.min(100.0));
    }
    let mut adapters: HashMap<u64, f32> = HashMap::new();
    for ((adapter, _), v) in per_engine {
        let e = adapters.entry(adapter).or_default();
        *e = e.max(v.min(100.0));
    }
    (processes, adapters)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_names() {
        let n = "pid_1234_luid_0x00000000_0x0000D1B8_phys_0_eng_3_engtype_VideoDecode";
        assert_eq!(parse_engine(n), Some((1234, 0xD1B8, 3)));
        assert_eq!(parse_engine("pid_7_luid_0x00000001_0x00000002_phys_0_eng_0_engtype_"), Some((7, (1 << 32) | 2, 0)));
        assert_eq!(parse_engine("_Total"), None);
        assert_eq!(parse_engine("pid_x_luid_0x0_0x0_phys_0_eng_0"), None);
        assert_eq!(parse_adapter("luid_0x00000000_0x0000D1B8_phys_0"), Some(0xD1B8));
        assert_eq!(luid_value(0, 0xD1B8), 0xD1B8);
        assert_eq!(luid_value(1, 2), (1 << 32) | 2);
    }

    #[test]
    fn adapters_take_their_busiest_engine() {
        let r = |pid, engine, percent| EngineUse { pid, adapter: 1, engine, percent };
        let readings = [r(10, 0, 30.0), r(11, 0, 25.0), r(10, 3, 40.0), r(10, 3, 5.0), r(12, 1, f32::NAN)];
        let (procs, adapters) = usage(&readings);
        // Engine 0 has 30 + 25; engine 3 has 45.
        assert_eq!(adapters[&1], 55.0);
        // Process 10: 30 on engine 0, 45 on engine 3.
        assert_eq!(procs[&10], 45.0);
        assert_eq!(procs[&11], 25.0);
        assert_eq!(procs[&12], 0.0);
    }

    #[test]
    fn use_never_passes_a_hundred() {
        let r = |pid| EngineUse { pid, adapter: 2, engine: 0, percent: 70.0 };
        let (_, adapters) = usage(&[r(1), r(2)]);
        assert_eq!(adapters[&2], 100.0);
    }
}
