//! Bounded native redraw workload: local sliders and window resizing, no hardware writes.
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

pub struct Benchmark {
    path: PathBuf,
    started: Instant,
    previous: Option<Instant>,
    intervals: Vec<f64>,
    frames: u32,
}
impl Benchmark {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            started: Instant::now(),
            previous: None,
            intervals: vec![],
            frames: 0,
        }
    }
    pub fn timed_out(&self) -> bool {
        self.started.elapsed() > Duration::from_secs(12)
    }
    pub fn count(&self) -> u32 {
        self.frames
    }
    pub fn frame(&mut self, at: Instant) -> bool {
        self.frames += 1;
        if self.started.elapsed() > Duration::from_secs(2) {
            if let Some(previous) = self.previous {
                self.intervals
                    .push(at.saturating_duration_since(previous).as_secs_f64() * 1000.0);
            }
            self.previous = Some(at);
        }
        self.intervals.len() >= 240 || self.started.elapsed() > Duration::from_secs(12)
    }
    pub fn save(&self, renderer: &str, adapter: &str) -> std::io::Result<()> {
        let mut intervals = self.intervals.clone();
        intervals.sort_by(f64::total_cmp);
        let percentile = |p: f64| {
            intervals
                .get(((intervals.len().saturating_sub(1)) as f64 * p).round() as usize)
                .copied()
        };
        let report = serde_json::json!({
            "renderer": renderer, "adapter": adapter,
            "build": if cfg!(debug_assertions) { "debug" } else { "release" },
            "workload": "native redraws with local slider changes, scroll position updates and alternating window sizes; excludes first 2 seconds",
            "frames": self.frames, "samples": intervals.len(),
            "p50_redraw_interval_ms": percentile(0.5), "p95_redraw_interval_ms": percentile(0.95),
            "max_redraw_interval_ms": intervals.last(),
            "elapsed_ms": self.started.elapsed().as_millis(),
        });
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&self.path, serde_json::to_vec_pretty(&report)?)
    }
}
