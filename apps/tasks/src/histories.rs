//! The last minute of every graph, filled from each sample: the summary tiles and the
//! Performance page share them.

use shellman_core::procs::history::Ring;
use shellman_core::procs::System;
use std::collections::HashMap;

#[derive(Default)]
pub struct Histories {
    pub cpu: Ring,
    /// Memory in use, percent of the total.
    pub memory: Ring,
    pub gpu: Ring,
    /// The busiest disk's active time.
    pub disk: Ring,
    /// Bytes per second over every adapter.
    pub net: Ring,
    pub cores: Vec<Ring>,
    /// Per disk: active time, read and write rates.
    pub disks: HashMap<String, [Ring; 3]>,
    /// Per adapter: send and receive rates.
    pub nets: HashMap<String, [Ring; 2]>,
    /// Per GPU: utilisation and dedicated memory in use (percent of its total).
    pub gpus: HashMap<String, [Ring; 2]>,
}

fn percent(part: u64, whole: u64) -> f32 {
    if whole == 0 {
        0.0
    } else {
        (part as f64 / whole as f64 * 100.0) as f32
    }
}

impl Histories {
    pub fn push(&mut self, s: &System) {
        self.cpu.push(s.cpu);
        self.memory.push(percent(s.memory.used(), s.memory.total));
        self.gpu.push(s.gpu().unwrap_or(0.0));
        self.disk.push(s.disk_active());
        self.net.push(s.net_rate() as f32);
        if self.cores.len() != s.cores.len() {
            self.cores = vec![Ring::default(); s.cores.len()];
        }
        for (ring, v) in self.cores.iter_mut().zip(&s.cores) {
            ring.push(*v);
        }
        // Devices that went away are forgotten; new ones start empty.
        self.disks.retain(|k, _| s.disks.iter().any(|d| &d.name == k));
        for d in &s.disks {
            let r = self.disks.entry(d.name.clone()).or_default();
            r[0].push(d.active);
            r[1].push(d.read as f32);
            r[2].push(d.write as f32);
        }
        self.nets.retain(|k, _| s.nets.iter().any(|n| &n.name == k));
        for n in &s.nets {
            let r = self.nets.entry(n.name.clone()).or_default();
            r[0].push(n.send as f32);
            r[1].push(n.receive as f32);
        }
        self.gpus.retain(|k, _| s.gpus.iter().any(|g| &g.name == k));
        for g in &s.gpus {
            let r = self.gpus.entry(g.name.clone()).or_default();
            r[0].push(g.utilisation);
            r[1].push(percent(g.dedicated_used, g.dedicated_total));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shellman_core::procs::{Disk, Memory};

    #[test]
    fn follows_the_machine() {
        let mut h = Histories::default();
        let mut s = System {
            cpu: 12.0,
            cores: vec![10.0, 14.0],
            memory: Memory { total: 100, available: 25, ..Default::default() },
            disks: vec![Disk { name: "0 C:".into(), active: 40.0, read: 5.0, write: 6.0 }],
            ..Default::default()
        };
        h.push(&s);
        assert_eq!(h.cpu.last(), Some(12.0));
        assert_eq!(h.memory.last(), Some(75.0));
        assert_eq!(h.cores.len(), 2);
        assert_eq!(h.disk.last(), Some(40.0));
        assert_eq!(h.disks["0 C:"][2].last(), Some(6.0));
        s.disks.clear();
        h.push(&s);
        assert!(h.disks.is_empty(), "a removed disk is forgotten");
        assert_eq!(h.gpu.last(), Some(0.0), "no GPU reads as idle");
    }
}
