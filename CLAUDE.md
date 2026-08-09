# CLAUDE.md — Blackbird

This file gives Claude (and any contributor) full context on the Blackbird project.
Update the "Current status" and "Completed milestones" sections as work progresses.

---

## What is Blackbird?

A native desktop tool for FPV drone pilots to analyse Betaflight blackbox logs,
visualise flight data, and get AI-assisted PID and filter tuning guidance.

Built as a response to PIDToolbox (MATLAB-based, paid, heavy runtime dependency).
Blackbird is free, open source, ships as a single native binary, and has AI
analysis built in from day one.

Target audience: FPV pilots who fly Betaflight — freestyle, racing, cinematic.
They are technical but not necessarily software developers. The tool must speak
FPV language: looptime, P/D balance, prop wash, notch filters, RPM filtering.

---

## Design philosophy

- **Single binary** — no runtime, no installer, no Python env. `cargo build --release`
  produces one executable. Drop it anywhere and it works
- **FPV-native language** — UI labels, AI output, and diagnostics use Betaflight
  terminology. A pilot should never feel like they're reading generic signal
  processing documentation
- **AI from day one** — the data model is designed so that every computed metric
  is serialisable into an AI prompt context. The AI layer is not bolted on later
- **Offline capable** — core analysis works without internet. AI requires either
  an Anthropic API key (cloud) or a local model via Ollama
- **Parser abstraction** — the `blackbox-log` crate is wrapped in a thin internal
  module. Its types never leak into the rest of the codebase. Swappable
- **Clean separation** — analysis modules know nothing about UI. UI knows nothing
  about AI. AI knows nothing about the parser. Each layer tests independently

---

## Tech stack

| Concern | Crate / Tool |
|---|---|
| GUI framework | `egui` + `eframe` |
| Plotting | `egui_plot` |
| Blackbox parsing | `blackbox-log` |
| FFT | `rustfft` |
| Numerical arrays | `ndarray` |
| Async runtime | `tokio` |
| HTTP client (AI) | `reqwest` |
| Serialisation | `serde` + `serde_json` |

---

## Module structure

```
src/
├── main.rs                  ← entry point, initialise eframe
├── app.rs                   ← App struct, central state, file drop handling
│
├── parser/
│   ├── mod.rs               ← re-exports, wraps blackbox-log crate
│   ├── header.rs            ← HeaderData struct (PIDs, filters, rates, craft name)
│   └── timeseries.rs        ← GyroTimeseries, RcCommandTimeseries, MotorTimeseries
│
├── analysis/
│   ├── mod.rs               ← AnalysisResult struct, orchestrates the three analysers
│   ├── spectral.rs          ← FFT, Hann windowing, throttle-binned spectral analysis
│   ├── step_response.rs     ← Wiener deconvolution, windowing, averaging
│   └── filter_delay.rs      ← cross-correlation, delay estimation in ms
│
├── ai/
│   ├── mod.rs               ← LlmBackend trait
│   ├── anthropic.rs         ← Anthropic API client via reqwest
│   ├── ollama.rs            ← local model fallback
│   └── prompt.rs            ← prompt builder from AnalysisResult + HeaderData
│
└── ui/
    ├── mod.rs
    └── panels/
        ├── timeseries.rs    ← raw signal viewer (milestone 1)
        ├── log_info.rs      ← header viewer: craft, firmware, PIDs, filters
        ├── spectral.rs      ← FFT heatmap panel (milestone 2)
        ├── step_response.rs ← step response curve panel (milestone 2)
        ├── filter_delay.rs  ← filter delay readout (milestone 2)
        └── ai_panel.rs      ← streaming AI response panel (milestone 3)
```

---

## Key data structures

### HeaderData

Extracted from the ASCII header at the top of every blackbox log.
Contains everything the AI needs as context for its starting point.

```rust
pub struct HeaderData {
    pub craft_name: String,
    pub firmware_version: String,
    pub board: String,
    pub sample_rate_hz: f32,      // critical — all time/freq calculations depend on this

    // PID values — what the pilot had when they flew
    pub pid_roll:  [f32; 3],      // [P, I, D]
    pub pid_pitch: [f32; 3],
    pub pid_yaw:   [f32; 3],

    // Filter settings
    pub gyro_lpf_hz: f32,
    pub dterm_lpf_hz: f32,
    pub rpm_filter_enabled: bool,
    pub notch_filters: Vec<NotchFilter>,
}
```

### PlotState

Shared across all panels — ensures cursor and zoom are synchronised.

```rust
pub struct PlotState {
    pub time_range: RangeInclusive<f64>,
    pub zoom: f64,
    pub cursor_time: Option<f64>,   // shared vertical cursor across panels
}
```

### AnalysisResult

The single struct that drives both the UI and the AI prompt.
The AI never sees raw timeseries — only computed metrics.

```rust
pub struct AnalysisResult {
    pub spectral:      [SpectralResult; 3],     // per axis: roll, pitch, yaw
    pub step_response: StepResponseAnalysis,   // per axis, see below
    pub filter_delay:  FilterDelayResult,
}

pub struct SpectralResult {
    pub noise_floor_db: f32,
    pub peaks: Vec<FrequencyPeak>,              // { freq_hz, amplitude_db }
    pub throttle_map: ndarray::Array2<f32>,     // throttle_bin × freq → dB
}

// Implemented as `analysis::step_response::AxisStepResponse`. No settling
// time: a 500 ms window averaged from as few as forty traces cannot support
// a stable one.
pub struct AxisStepResponse {
    pub time_ms: Arc<[f64]>,                    // shared across traces
    pub sample: Vec<Vec<f64>>,                  // bounded, evenly spread subset
    pub count: usize,                           // surviving windows, ≥ sample.len()
    pub mean: Vec<f64>,                         // pointwise average of them all
    pub metrics: StepMetrics,                   // measured on `mean`
}

pub struct StepMetrics {
    pub overshoot_pct: f64,                     // peak over the steady state
    pub peak_ms: f64,
    pub delay_ms: f64,                          // to the 50% crossing
    pub spread_pct: RangeInclusive<f64>,        // IQR of the per-trace peak
}

pub struct FilterDelayResult {
    pub delay_ms: f32,
}
```

---

## Signal processing notes

### Spectral analysis

- Window function: **Hann** — must be applied before FFT to prevent spectral leakage
- Throttle binning: split log into 10 bins (0-10%, 10-20% … 90-100%)
- Output: dB per frequency bin per throttle bin → 2D heatmap
- Formula: `amplitude_db = 20.0 * log10(magnitude)`
- Input signal: `gyro_raw` (pre-filter gyro, not post-filter)
- Analysed over `FlightData::trimmed(trim_s)`, not the whole log — see below

### Step response

Wiener deconvolution, not step detection: real freestyle logs contain almost no
isolated, sustained stick steps, so a detector finds nothing or averages a
handful of unrepresentative events. Deconvolution uses every part of the flight
where the sticks moved — the same approach as PIDToolbox and Blackbox Explorer.

- Per overlapping window (1 s, hopping window/16 = 62.5 ms): multiply both
  signals by a **Hann window**, then FFT the setpoint and the gyro, zero-padded
  so the convolution is linear rather than circular
- The Hann window is not optional. A window is a slice of continuous flight;
  handed to the FFT as a rectangle, its edges are step discontinuities the craft
  never flew, and their leakage lands at `t = 0` as a spike that the cumulative
  sum turns into a head start. λ does not absorb this — PIDToolbox and
  PID-Analyzer both taper first
- `H = (G · conj(S)) / (|S|² + λ)`, `λ = 0.01 · mean(|S|²)` — λ regularises the
  frequencies where the setpoint carried no energy
- IFFT → impulse response; cumulative sum → step response; truncate to 500 ms
- Normalise each trace by the mean of its last 100 ms, so one drifting trace
  cannot shift the average
- Reject a trace whose steady state falls outside `0.5..=3.0` — including any
  negative one, which would otherwise be flipped upright into the mean
- Reject windows where `max |setpoint| < 52 deg/s`; no throttle gating
- Both analysers run over the log's middle: `FlightData::trimmed(trim_s)` is a
  view (no copy) that drops the first and last `trim_s`, 2 s by default. The
  ends are arming, the hand launch and the landing — ground resonance the
  spectral analysis would report as flight noise, and stick input answered by a
  craft that is not airborne. Trimming is skipped when it would take more than
  half the log, so a short bench log is still analysed whole. Trimmed time
  values stay relative to the untrimmed start, so the spectrogram lines up with
  the timeseries plots
- Defaults match PID-Analyzer (`framelen = 1.0`, `superpos = 16`,
  `np.hanning`). The panel exposes window, hop, minimum stick input, λ and the
  two ends of the band as knobs; `tail_ms`, `response_ms` and `max_traces`
  stay fields, because they change normalisation or memory rather than the
  view; load-time analysis always uses the defaults
- Input: `setpoint` (logged field only) and `gyroADC` (filtered), which is what
  the PID loop saw, so filter delay belongs in the measured response
- Traces are kept individually as well as averaged — the spread is diagnostic.
  Only a bounded, evenly spread sample of them is retained (`max_traces`, 200);
  the mean and the metrics always come from every surviving window
- Metrics (`StepMetrics`) are measured on the mean curve, so the number and the
  drawn curve can never disagree. Below ten responses the panel says so

### Filter delay

- Cross-correlate `gyro` (pre-filter) with `gyroADC` (post-filter)
- Cross-correlation via FFT multiply in frequency domain (efficient)
- Lag at maximum correlation × (1 / sample_rate_hz) × 1000 = delay in ms

---

## AI integration

### LlmBackend trait

```rust
pub trait LlmBackend: Send + Sync {
    async fn analyse(&self, context: &TuneContext) -> Result<AnalysisStream>;
}
```

Two implementations:

- `AnthropicBackend` — calls `api.anthropic.com/v1/messages`, streams response
- `OllamaBackend` — calls local Ollama instance, offline fallback

### TuneContext

What gets sent to the model. Structured, not raw timeseries.

```rust
pub struct TuneContext {
    pub header: HeaderData,         // current PID/filter values, craft info
    pub analysis: AnalysisResult,   // computed metrics
    pub pilot_notes: Option<String> // free text from the pilot ("prop wash on fast turns")
}
```

### Prompt design (prompt.rs)

The system prompt encodes PID tuning expertise:

- What each metric means in FPV context
- Known diagnostic relationships (overshoot > 10% → P/D imbalance, etc.)
- Valid Betaflight CLI syntax for suggested changes
- Output format: Diagnosis → Recommended changes → Betaflight CLI block

The CLI block must be copyable directly into Betaflight configurator.
This is the killer feature — not just diagnosis but ready-to-paste commands.

---

## File format notes

- `.bbl` — from onboard flash chip, **can contain multiple logs** in one file
- `.bfl` — from SD card, contains a single flight
- Both handled transparently by `blackbox-log` crate
- Log store: `Arc<Mutex<Vec<ParsedLog>>>` — shared between parser thread and UI

---

## Milestones

### Milestone 1 — Blackbox Viewer (current)

Goal: load a log, see the data. Validate the full parser → UI pipeline.

- [ ] Project setup — `cargo new blackbird`, add dependencies
- [ ] `parser/` — wrap `blackbox-log`, extract `HeaderData` and timeseries
- [ ] `app.rs` — file drag-and-drop, log store
- [ ] `ui/panels/log_info.rs` — header viewer
- [ ] `ui/panels/timeseries.rs` — raw gyro/rc/motor plot with egui_plot
- [ ] Axis toggles (show/hide roll, pitch, yaw)
- [ ] Multi-log switcher for `.bbl` files
- [ ] Graceful handling of corrupted frames

### Milestone 2 — Analysis

- [ ] `analysis/spectral.rs` — FFT + throttle heatmap
- [x] `analysis/step_response.rs` — Wiener deconvolution + averaging
- [ ] `analysis/filter_delay.rs` — cross-correlation delay
- [ ] Corresponding UI panels

### Milestone 3 — AI Integration

- [ ] `ai/prompt.rs` — TuneContext → prompt
- [ ] `ai/anthropic.rs` — streaming API client
- [ ] `ai/ollama.rs` — local fallback
- [ ] `ui/panels/ai_panel.rs` — streaming response + copyable CLI block
- [ ] Settings panel — API key, model selection, backend toggle

### Future / Backlog

- [ ] Multi-log overlay comparison
- [ ] Opt-in log submission for future ML training dataset
- [ ] ML model for direct PID suggestions (requires labelled dataset)
- [ ] Export: PNG charts, CSV data dump

---

## Completed milestones

_(none yet — project is in initial setup)_

---

## Design decisions already made

| Decision | Rationale |
|---|---|
| Single `src/` with modules, not a workspace | Project scope doesn't justify crate splitting. No shared library to publish |
| `blackbox-log` crate, not a custom parser | Format is gnarly (variable-length encoding, predictor compression). Already solved correctly in Rust |
| `blackbox-log` wrapped in thin internal module | Crate types must not leak. If we swap the crate, only `parser/` changes |
| `AnalysisResult` feeds both UI and AI | Single source of truth. AI reasons over computed metrics, not raw floats |
| `PlotState` lives in `app.rs`, passed to all panels | Future panels (spectral, step response) share the same time range and cursor |
| AI as trait with two backends | Anthropic for quality, Ollama for offline/privacy. Swappable at runtime |
| `prompt.rs` isolated from API plumbing | Prompt is a product decision iterated independently of transport code |

---

## Open questions

- Which Anthropic model for the AI panel? Likely claude-sonnet-4-5 for balance of
  quality and cost. Consider claude-haiku for faster streaming.
- Ollama: which local model performs best for PID tuning diagnosis?
  Needs evaluation once milestone 2 data is available.
- Settings persistence: `dirs` crate for platform config dir, or flat file next to binary?

---

## Agent skills

### Issue tracker

Local markdown — issues live under `.scratch/<feature-slug>/`. See `docs/agents/issue-tracker.md`.

### Domain docs

Single-context — `CONTEXT.md` + `docs/adr/` at repo root (created lazily). See `docs/agents/domain.md`.
