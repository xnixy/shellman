//! Frame timing for measuring the renderer: `SHELLMAN_TERMINAL_STATS=<file>` appends one line a
//! second while frames are drawn (frames, the CPU time of a frame, rows sent to the GPU).

use std::io::Write;
use std::time::{Duration, Instant};

pub struct Stats {
    file: Option<std::fs::File>,
    since: Instant,
    frames: u32,
    cpu_sum: f32,
    cpu_max: f32,
    prepare_sum: f32,
    prepare_max: f32,
    rows: usize,
}

impl Stats {
    pub fn from_env() -> Self {
        let file = std::env::var_os("SHELLMAN_TERMINAL_STATS")
            .and_then(|p| std::fs::OpenOptions::new().create(true).append(true).open(p).ok());
        Self { file, since: Instant::now(), frames: 0, cpu_sum: 0.0, cpu_max: 0.0, prepare_sum: 0.0, prepare_max: 0.0, rows: 0 }
    }

    pub fn on(&self) -> bool {
        self.file.is_some()
    }

    /// One frame: the previous frame's CPU time (eframe's), this one's snapshot and prepare
    /// time, and the rows it sent.
    pub fn frame(&mut self, cpu: Option<f32>, prepare: Duration, rows: usize) {
        let Some(file) = self.file.as_mut() else { return };
        let now = Instant::now();
        if self.frames == 0 {
            self.since = now;
        }
        self.frames += 1;
        let cpu = cpu.unwrap_or(0.0) * 1000.0;
        self.cpu_sum += cpu;
        self.cpu_max = self.cpu_max.max(cpu);
        let p = prepare.as_secs_f32() * 1000.0;
        self.prepare_sum += p;
        self.prepare_max = self.prepare_max.max(p);
        self.rows += rows;
        let span = now - self.since;
        if span >= Duration::from_secs(1) {
            let n = self.frames as f32;
            let _ = writeln!(
                file,
                "{:.1}s frames={} fps={:.0} frame_cpu_ms avg={:.2} max={:.2} prepare_ms avg={:.2} max={:.2} rows={}",
                span.as_secs_f32(),
                self.frames,
                n / span.as_secs_f32(),
                self.cpu_sum / n,
                self.cpu_max,
                self.prepare_sum / n,
                self.prepare_max,
                self.rows
            );
            *self = Self { file: self.file.take(), ..Self::from_env_empty() };
        }
    }

    fn from_env_empty() -> Self {
        Self { file: None, since: Instant::now(), frames: 0, cpu_sum: 0.0, cpu_max: 0.0, prepare_sum: 0.0, prepare_max: 0.0, rows: 0 }
    }
}
