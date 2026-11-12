//! The real [`Source`]: the system process list, the window list and the PDH counters, read once
//! per tick and turned into shares and rates against the tick before.

use super::cpu::{self, Times};
use super::gpu::{self, EngineUse};
use super::info::{self, Static};
use super::pdh::Query;
use super::system::{self, Adapter, Link};
use super::{nt, wins, Disk, Gpu, Key, Net, Proc, Sample, Source, System, Window};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use windows::Win32::System::Threading::GetActiveProcessorCount;

/// How often the network adapters' list (names, addresses) is read again.
const LINKS_EVERY: Duration = Duration::from_secs(10);

/// The counters, added once.
struct Counters {
    query: Query,
    engine: Option<usize>,
    gpu_dedicated: Option<usize>,
    gpu_shared: Option<usize>,
    disk_idle: Option<usize>,
    disk_read: Option<usize>,
    disk_write: Option<usize>,
    net_send: Option<usize>,
    net_receive: Option<usize>,
    performance: Option<usize>,
}

impl Counters {
    fn open() -> Option<Self> {
        let mut query = Query::open()?;
        let mut add = |p: &str| query.add(p);
        let c = Counters {
            engine: add(r"\GPU Engine(*)\Utilization Percentage"),
            gpu_dedicated: add(r"\GPU Adapter Memory(*)\Dedicated Usage"),
            gpu_shared: add(r"\GPU Adapter Memory(*)\Shared Usage"),
            disk_idle: add(r"\PhysicalDisk(*)\% Idle Time"),
            disk_read: add(r"\PhysicalDisk(*)\Disk Read Bytes/sec"),
            disk_write: add(r"\PhysicalDisk(*)\Disk Write Bytes/sec"),
            net_send: add(r"\Network Interface(*)\Bytes Sent/sec"),
            net_receive: add(r"\Network Interface(*)\Bytes Received/sec"),
            performance: add(r"\Processor Information(_Total)\% Processor Performance"),
            query,
        };
        Some(c)
    }

    /// Every instance of a wildcard counter, `_Total` left out.
    fn instances(&mut self, at: Option<usize>) -> Vec<(String, f64)> {
        let mut out = Vec::new();
        if let Some(at) = at {
            self.query.each(at, |name, v| {
                if name != "_Total" {
                    out.push((name.to_owned(), v));
                }
            });
        }
        out
    }
}

pub struct NativeSource {
    reader: nt::Reader,
    cores: usize,
    previous: HashMap<Key, (i64, u64)>,
    previous_at: Option<Instant>,
    previous_cores: Vec<Times>,
    known: HashMap<Key, Static>,
    counters: Option<Counters>,
    adapters: Vec<Adapter>,
    links: Vec<Link>,
    links_at: Option<Instant>,
    rated_mhz: Option<f32>,
}

impl Default for NativeSource {
    fn default() -> Self {
        Self::new()
    }
}

impl NativeSource {
    pub fn new() -> Self {
        // Every group's processors (0xFFFF is ALL_PROCESSOR_GROUPS).
        let cores = (unsafe { GetActiveProcessorCount(0xFFFF) } as usize).max(1);
        Self {
            reader: nt::Reader::default(),
            cores,
            previous: HashMap::new(),
            previous_at: None,
            previous_cores: Vec::new(),
            known: HashMap::new(),
            counters: Counters::open(),
            adapters: system::adapters(),
            links: Vec::new(),
            links_at: None,
            rated_mhz: system::rated_mhz(cores),
        }
    }

    /// GPU use per process and per adapter, and each adapter's memory in use.
    fn gpus(&mut self) -> (HashMap<u32, f32>, Vec<Gpu>) {
        let Some(c) = self.counters.as_mut() else {
            return (HashMap::new(), Vec::new());
        };
        let readings: Vec<EngineUse> = c
            .instances(c.engine)
            .into_iter()
            .filter_map(|(name, v)| {
                let (pid, adapter, engine) = gpu::parse_engine(&name)?;
                Some(EngineUse { pid, adapter, engine, percent: v as f32 })
            })
            .collect();
        let (per_pid, per_adapter) = gpu::usage(&readings);
        let mut memory: HashMap<u64, (u64, u64)> = HashMap::new();
        for (name, v) in c.instances(c.gpu_dedicated) {
            if let Some(a) = gpu::parse_adapter(&name) {
                memory.entry(a).or_default().0 += v as u64;
            }
        }
        for (name, v) in c.instances(c.gpu_shared) {
            if let Some(a) = gpu::parse_adapter(&name) {
                memory.entry(a).or_default().1 += v as u64;
            }
        }
        let gpus = self
            .adapters
            .iter()
            .map(|a| {
                let (dedicated, shared) = memory.get(&a.luid).copied().unwrap_or_default();
                Gpu {
                    name: a.name.clone(),
                    utilisation: per_adapter.get(&a.luid).copied().unwrap_or(0.0),
                    dedicated_used: dedicated,
                    dedicated_total: a.dedicated,
                    shared_used: shared,
                    shared_total: a.shared,
                }
            })
            .collect();
        (per_pid, gpus)
    }

    fn disks(&mut self) -> Vec<Disk> {
        let Some(c) = self.counters.as_mut() else {
            return Vec::new();
        };
        let reads: HashMap<String, f64> = c.instances(c.disk_read).into_iter().collect();
        let writes: HashMap<String, f64> = c.instances(c.disk_write).into_iter().collect();
        let mut disks: Vec<Disk> = c
            .instances(c.disk_idle)
            .into_iter()
            .map(|(name, idle)| Disk {
                active: (100.0 - idle).clamp(0.0, 100.0) as f32,
                read: reads.get(&name).copied().unwrap_or(0.0),
                write: writes.get(&name).copied().unwrap_or(0.0),
                name,
            })
            .collect();
        disks.sort_by(|a, b| a.name.cmp(&b.name));
        disks
    }

    fn nets(&mut self, now: Instant) -> Vec<Net> {
        if self.links_at.is_none_or(|t| now.duration_since(t) >= LINKS_EVERY) {
            self.links = system::links();
            self.links_at = Some(now);
        }
        let Some(c) = self.counters.as_mut() else {
            return Vec::new();
        };
        let send: HashMap<String, f64> = c.instances(c.net_send).into_iter().collect();
        let receive: HashMap<String, f64> = c.instances(c.net_receive).into_iter().collect();
        self.links
            .iter()
            .map(|l| {
                let key = system::counter_name(&l.description);
                Net {
                    name: l.name.clone(),
                    description: l.description.clone(),
                    address: l.address.clone(),
                    send: send.get(&key).copied().unwrap_or(0.0),
                    receive: receive.get(&key).copied().unwrap_or(0.0),
                }
            })
            .collect()
    }
}

impl Source for NativeSource {
    fn sample(&mut self) -> Sample {
        let started = Instant::now();
        let raws = self.reader.processes();
        let core_times = self.reader.cores(self.cores);
        let mut windows: HashMap<u32, Vec<Window>> = HashMap::new();
        for (pid, w) in wins::app_windows() {
            windows.entry(pid).or_default().push(w);
        }
        if let Some(c) = &self.counters {
            c.query.collect();
        }
        let (gpu_by_pid, gpus) = self.gpus();
        let has_gpu_counters = self.counters.as_ref().is_some_and(|c| c.engine.is_some());

        let elapsed = self.previous_at.map(|t| started.duration_since(t)).unwrap_or_default();
        let elapsed_100ns = (elapsed.as_nanos() / 100) as i64;
        let secs = elapsed.as_secs_f64();
        let mut previous = HashMap::with_capacity(raws.len());
        let mut procs = Vec::with_capacity(raws.len());
        for raw in raws {
            let key = Key { pid: raw.pid, created: raw.created };
            let known = self.known.entry(key).or_insert_with(|| info::read_static(key, &raw.name));
            let (cpu, disk) = match self.previous.get(&key) {
                Some(&(t, io)) => (
                    cpu::process_percent(t, raw.cpu_time, elapsed_100ns, self.cores as u32),
                    cpu::rate(io, raw.io_bytes, secs),
                ),
                None => (0.0, 0.0),
            };
            previous.insert(key, (raw.cpu_time, raw.io_bytes));
            procs.push(Proc {
                key,
                parent: raw.parent,
                name: raw.name.into(),
                exe: known.exe.clone(),
                description: known.description.clone(),
                command_line: known.command_line.clone(),
                cpu,
                memory: raw.private_ws,
                disk,
                gpu: has_gpu_counters.then(|| gpu_by_pid.get(&raw.pid).copied().unwrap_or(0.0)),
                threads: raw.threads,
                handles: raw.handles,
                session: raw.session,
                suspended: raw.suspended,
                windows: windows.remove(&raw.pid).unwrap_or_default(),
                access: known.access,
            });
        }
        self.known.retain(|k, _| previous.contains_key(k));
        self.previous = previous;
        self.previous_at = Some(started);

        let (cpu_total, cores) = cpu::cores_percent(&self.previous_cores, &core_times);
        self.previous_cores = core_times;
        let (memory, [processes, threads, handles]) = system::memory();
        let performance = self.counters.as_ref().and_then(|c| c.query.value(c.performance?));
        let disks = self.disks();
        let nets = self.nets(started);
        let system = System {
            cpu: cpu_total,
            cores,
            mhz: self.rated_mhz.zip(performance).map(|(m, p)| m * p as f32 / 100.0),
            uptime_secs: system::uptime_secs(),
            processes,
            threads,
            handles,
            memory,
            disks,
            nets,
            gpus,
        };
        Sample { procs, system, took: started.elapsed() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_samples_give_shares() {
        let mut s = NativeSource::new();
        let first = s.sample();
        assert!(first.procs.len() > 10);
        std::thread::sleep(Duration::from_millis(300));
        let second = s.sample();
        assert!(second.procs.iter().all(|p| (0.0..=100.0).contains(&p.cpu)));
        assert_eq!(second.system.cores.len(), s.cores.min(64));
        assert!(second.system.memory.total > 0);
        let me = second.procs.iter().find(|p| p.key.pid == std::process::id()).unwrap();
        assert!(me.exe.is_some() && me.memory > 0);
    }

    /// What one tick costs on this machine: `cargo test -p shellman-core --release cost -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn cost() {
        let t = Instant::now();
        let mut s = NativeSource::new();
        println!("open: {:?}", t.elapsed());
        for i in 0..6 {
            let sample = s.sample();
            println!(
                "tick {i}: {:?}, {} processes, {} gpus {:?}, {} disks, {} nets, cpu {:.1}%, mhz {:?}",
                sample.took,
                sample.procs.len(),
                sample.system.gpus.len(),
                sample.system.gpus.iter().map(|g| (g.name.clone(), g.utilisation)).collect::<Vec<_>>(),
                sample.system.disks.len(),
                sample.system.nets.len(),
                sample.system.cpu,
                sample.system.mhz
            );
            std::thread::sleep(Duration::from_millis(1000));
        }
        // CPU time of this thread, not wall time, from its cycle count (at about the rated speed).
        let ghz = s.rated_mhz.unwrap_or(3000.0) as f64 / 1000.0;
        #[link(name = "kernel32", kind = "raw-dylib")]
        extern "system" {
            fn QueryThreadCycleTime(thread: isize, cycles: *mut u64) -> i32;
            fn GetCurrentThread() -> isize;
        }
        let cpu = || {
            let mut cycles = 0u64;
            unsafe { QueryThreadCycleTime(GetCurrentThread(), &mut cycles) };
            Duration::from_nanos((cycles as f64 / ghz) as u64)
        };
        for _ in 0..5 {
            let t0 = cpu();
            s.reader.processes();
            let a = cpu();
            wins::app_windows();
            let b = cpu();
            s.counters.as_ref().unwrap().query.collect();
            let c = cpu();
            s.gpus();
            let d = cpu();
            let _ = s.disks().len() + s.nets(Instant::now()).len();
            system::memory();
            let e = cpu();
            let whole = cpu();
            s.sample();
            let f = cpu();
            println!("cpu: nt {:?}, wins {:?}, collect {:?}, gpus {:?}, rest {:?}; a whole sample {:?}", a - t0, b - a, c - b, d - c, e - d, f - whole);
            std::thread::sleep(Duration::from_millis(1000));
        }
    }
}
