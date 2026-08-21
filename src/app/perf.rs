//! `[DEBUG-perf9]` Frame-time probe. Temporary — delete with the Windows lag bug.
//!
//! Splits a frame into *our CPU* (`ui_ms`), *egui + render CPU* (`cpu_ms`, from
//! `eframe`, vsync wait excluded) and *wall period* (`dt_ms`). A stall with a
//! tiny `ui_ms`/`cpu_ms` and a huge `dt_ms` happened outside this process's
//! CPU — present, driver or OS — which is the split the bug turns on.
//!
//! Env: `BLACKBIRD_PERF=1` on, `_OPEN=<log>` load at startup, `_FRAMES=N`
//! (900), `_SPIKE_MS=100`, `_OUT=<path>`, `_HOVER`/`_DRAG`/`_ZOOM=1` synthetic
//! input. Exit code 1 means the loop went red: a frame breached `_SPIKE_MS`.

use std::env;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::Instant;

struct Sample {
    t_ms: f64,
    dt_ms: f64,
    ui_ms: f64,
    cpu_ms: f64,
}

pub struct Perf {
    limit: usize,
    spike_ms: f64,
    out: PathBuf,
    hover: bool,
    drag: bool,
    zoom: bool,
    start: Instant,
    last: Instant,
    head: Option<String>,
    injected: usize,
    samples: Vec<Sample>,
}

fn flag(key: &str) -> bool {
    env::var(key).map(|v| v != "0").unwrap_or(false)
}

fn num<T: std::str::FromStr>(key: &str, default: T) -> T {
    env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// The log to open without going through the file dialog, so the probe runs
/// unattended.
pub fn open_path() -> Option<PathBuf> {
    env::var("BLACKBIRD_PERF_OPEN").ok().map(PathBuf::from)
}

impl Perf {
    pub fn from_env() -> Option<Self> {
        flag("BLACKBIRD_PERF").then(|| Self {
            limit: num("BLACKBIRD_PERF_FRAMES", 900),
            spike_ms: num("BLACKBIRD_PERF_SPIKE_MS", 100.0),
            out: num::<String>("BLACKBIRD_PERF_OUT", "blackbird-perf.log".into()).into(),
            hover: flag("BLACKBIRD_PERF_HOVER"),
            drag: flag("BLACKBIRD_PERF_DRAG"),
            zoom: flag("BLACKBIRD_PERF_ZOOM"),
            start: Instant::now(),
            last: Instant::now(),
            head: None,
            injected: 0,
            samples: Vec::new(),
        })
    }

    /// Synthetic pointer traffic, so the interactive path is exercised without
    /// a hand on the mouse. Pushed into the same event queue winit fills.
    pub fn inject(&mut self, ctx: &egui::Context, input: &mut egui::RawInput) {
        if !(self.hover || self.drag || self.zoom) {
            return;
        }
        let rect = input.screen_rect.unwrap_or_else(|| ctx.input(|i| i.viewport_rect()));
        if rect.width() < 1.0 {
            return;
        }

        let n = self.injected;
        self.injected += 1;
        let phase = (n as f32 * 0.09).sin();
        let pos = egui::pos2(
            rect.center().x + phase * rect.width() * 0.3,
            rect.center().y + (n as f32 * 0.05).sin() * rect.height() * 0.2,
        );
        input.events.push(egui::Event::PointerMoved(pos));

        if self.drag && n == 1 {
            input.events.push(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            });
        }
        if self.zoom && n % 20 == 0 {
            input.events.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Line,
                delta: egui::vec2(0.0, if n % 40 == 0 { 1.0 } else { -1.0 }),
                phase: egui::TouchPhase::Move,
                modifiers: Default::default(),
            });
        }
    }

    /// One frame's numbers. `t0` is when this frame's `ui()` began; `settled`
    /// is false while a load is in flight, which resets the window so the
    /// report covers steady state only.
    pub fn record(
        &mut self,
        ctx: &egui::Context,
        frame: &eframe::Frame,
        t0: Instant,
        settled: bool,
    ) {
        ctx.request_repaint();
        let now = Instant::now();
        let dt_ms = (now - self.last).as_secs_f64() * 1e3;
        self.last = now;

        // The first frame has no previous one to measure a period against, and
        // a load in flight is not steady state.
        if self.head.is_none() {
            self.head = Some(self.header(frame));
            self.start = now;
            return;
        }
        if !settled {
            self.start = now;
            self.samples.clear();
            return;
        }

        self.samples.push(Sample {
            t_ms: (now - self.start).as_secs_f64() * 1e3,
            dt_ms,
            ui_ms: t0.elapsed().as_secs_f64() * 1e3,
            cpu_ms: frame.info().cpu_usage.unwrap_or(0.0) as f64 * 1e3,
        });

        if self.samples.len() >= self.limit {
            self.finish();
        }
    }

    fn header(&self, frame: &eframe::Frame) -> String {
        let mut s = String::new();
        let _ = writeln!(
            s,
            "os={} arch={} profile={} version={}",
            env::consts::OS,
            env::consts::ARCH,
            if cfg!(debug_assertions) { "debug" } else { "release" },
            env!("CARGO_PKG_VERSION"),
        );
        if let Some(state) = frame.wgpu_render_state() {
            let info = state.adapter.get_info();
            let _ = writeln!(
                s,
                "backend={:?} adapter={:?} type={:?} driver={:?} {:?}",
                info.backend, info.name, info.device_type, info.driver, info.driver_info
            );
        }
        if let Some(cfg) = frame.wgpu_surface_config() {
            let _ = writeln!(
                s,
                "present_mode={:?} frame_latency={:?}",
                cfg.present_mode, cfg.desired_maximum_frame_latency
            );
        }
        s
    }

    fn finish(&mut self) {
        let pct = |mut v: Vec<f64>, p: f64| {
            v.sort_by(f64::total_cmp);
            v[((v.len() - 1) as f64 * p) as usize]
        };
        let col = |f: fn(&Sample) -> f64| self.samples.iter().map(f).collect::<Vec<_>>();
        let (dt, ui, cpu) = (
            col(|s| s.dt_ms),
            col(|s| s.ui_ms),
            col(|s| s.cpu_ms),
        );

        let wall_s = dt.iter().sum::<f64>() / 1e3;
        let spikes: Vec<&Sample> = self
            .samples
            .iter()
            .filter(|s| s.dt_ms >= self.spike_ms)
            .collect();
        let stalled_s = spikes.iter().map(|s| s.dt_ms).sum::<f64>() / 1e3;

        let mut r = format!("[DEBUG-perf9] blackbird frame report\n");
        r.push_str(self.head.as_deref().unwrap_or(""));
        let _ = writeln!(
            r,
            "hover={} drag={} zoom={}\nframes={} wall={wall_s:.2}s mean_fps={:.1}",
            self.hover,
            self.drag,
            self.zoom,
            self.samples.len(),
            self.samples.len() as f64 / wall_s.max(1e-9),
        );
        for (name, v) in [("dt_ms ", dt.clone()), ("ui_ms ", ui), ("cpu_ms", cpu)] {
            let _ = writeln!(
                r,
                "{name} p50={:7.2} p90={:7.2} p99={:7.2} max={:8.2}",
                pct(v.clone(), 0.50),
                pct(v.clone(), 0.90),
                pct(v.clone(), 0.99),
                pct(v, 1.0),
            );
        }
        let _ = writeln!(
            r,
            "spikes >={:.0}ms: {} frames, {stalled_s:.2}s stalled ({:.1}% of wall)",
            self.spike_ms,
            spikes.len(),
            stalled_s / wall_s.max(1e-9) * 100.0,
        );
        let mut worst = spikes;
        worst.sort_by(|a, b| b.dt_ms.total_cmp(&a.dt_ms));
        for s in worst.iter().take(10) {
            let _ = writeln!(
                r,
                "  t={:8.2}s dt={:8.2} ui={:6.2} cpu={:6.2}",
                s.t_ms / 1e3,
                s.dt_ms,
                s.ui_ms,
                s.cpu_ms
            );
        }
        let red = pct(dt, 1.0) >= self.spike_ms;
        let _ = writeln!(
            r,
            "VERDICT: {}",
            if red { "RED — spike reproduced" } else { "GREEN — no spike" }
        );

        let _ = std::fs::write(&self.out, &r);
        print!("{r}");
        std::process::exit(red as i32);
    }
}
