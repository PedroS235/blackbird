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
│   ├── metadata.rs          ← Metadata struct (filters, rates, craft name, raw headers)
│   └── timeseries.rs        ← GyroTimeseries, RcCommandTimeseries, MotorTimeseries
│
├── analysis/
│   ├── mod.rs               ← Analysis struct, orchestrates the analysers
│   ├── overlays.rs          ← filter geometry: bands, harmonic groups, traced centre
│   ├── spectral.rs          ← FFT, Hann windowing, throttle-binned spectral analysis
│   ├── step_response.rs     ← Wiener deconvolution, windowing, averaging
│   └── filter_delay.rs      ← cross-correlation, delay estimation in ms
│
├── ai/
│   ├── mod.rs               ← LlmBackend trait
│   ├── anthropic.rs         ← Anthropic API client via reqwest
│   ├── ollama.rs            ← local model fallback
│   └── prompt.rs            ← prompt builder from Analysis + Metadata
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

### Metadata

Extracted from the ASCII header at the top of every blackbox log. Lives in
`parser::metadata`. **PIDs are not parsed** — they sit unread in
`raw_headers`, which is also where `motor_poles` is read from.

```rust
pub struct Metadata {
    pub file_name: String,
    pub craft_name: String,
    pub firmware: String,
    pub board: String,
    pub looptime_us: Option<u32>,
    pub duration: Duration,
    pub filters: FilterConfig,      // see below
    pub rates: Option<RateConfig>,  // None where the log records no rate curve
    pub debug_mode: String,         // e.g. "FFT_FREQ"
    pub raw_headers: HashMap<String, String>,
}

pub struct FilterConfig {
    pub gyro_lpf1: Option<LowpassConfig>,        // static, or a dynamic min..max
    pub gyro_lpf2: Option<StaticLowpassConfig>,
    pub dterm_lpf1: Option<DtermLowpass1Config>,
    pub dterm_lpf2: Option<StaticLowpassConfig>,
    pub gyro_notches: Vec<NotchConfig>,          // { center_hz, cutoff_hz }
    pub dterm_notches: Vec<NotchConfig>,
    pub dyn_notch: Option<DynNotchConfig>,       // { min_hz, max_hz, count, q }
    pub rpm_filter: Option<RpmFilterConfig>,     // { harmonics, min_hz, q, weights, … }
}
```

The sample rate is not here — it is measured from the timestamps and lives on
`FlightData`, so a log whose looptime header lies is still analysed correctly.

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

Implemented as `analysis::Analysis`; a new analyser adds a field here rather
than another `Vec` to the loader, the log store and the tab context.

```rust
pub struct Analysis {
    pub spectral: SpectralAnalysis,
    pub step:     StepResponseAnalysis,
    // filter delay lands here
}

pub struct SpectralAnalysis {
    axes: PerAxis<Option<AxisSpectral>>,   // None where the axis had no gyroUnfilt
    pub overlays: Vec<FilterOverlay>,      // filter geometry, see below
}

pub struct AxisSpectral {
    pub raw_psd: Psd,
    pub filtered_psd: Option<Psd>,
    pub raw_spectrum: Spectrum,
    pub filtered_spectrum: Option<Spectrum>,
    pub throttle_map: Option<BinnedSpectrum>,   // throttle bin × freq → dB
    pub time_map: Option<BinnedSpectrum>,       // the spectrogram
    pub peaks: Vec<FrequencyPeak>,
    pub noise_floor_db: f64,                    // computed, still undisplayed
}

pub struct FrequencyPeak {
    pub freq_hz: f64,
    pub amplitude_db: f64,
    pub harmonic_of: Option<usize>,             // index of its fundamental
    pub attenuated_db: Option<f64>,             // raw − filtered at this bin
    pub dyn_notch_reach: Option<DynNotchReach>, // Inside / BelowMin / AboveMax
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

### Filter overlay geometry

`analysis::overlays` computes what each filter actually occupies in frequency,
at load time, stored on `Analysis`. It is not a pure function the panel calls
per frame: the geometry depends on the analysed window, which is fixed at
load, and storing it puts the feature behind the loader integration seam.

- One `FilterOverlay { label, family, shape }`. `OverlayFamily` carries the
  gyro/D-term loop, so a panel selects gyro overlays by matching the type
  rather than by `label.starts_with("Gyro")`
- `OverlayShape::Line` — a fixed lowpass corner, the only filter with no width
- `OverlayShape::Band` — a notch's −3 dB width (`centre / Q`, with Q derived
  from centre and cutoff as Betaflight's `filterGetNotchQ` does), a dynamic
  lowpass's swept range, or the dynamic notch's configured range. A dynamic
  filter drawn at one nominal centre is a guess at a frequency it never sat at
- `OverlayShape::Harmonics` — one band per motor per order, from `eRPM`, over
  the frequencies that motor actually reached. Order count comes from
  `RpmFilterConfig::harmonics`; a zero-weight order is flagged unfiltered.
  Stopped-motor samples are excluded, so no band runs down to 0 Hz
- `OverlayShape::Traced` — where the dynamic notch tracker actually sat, as a
  histogram over frequency, per axis. Read from `debug[0..3]`, gated on
  `Metadata::logs_dyn_notch_trace()` (debug mode `FFT_FREQ`) — the one rule,
  shared with the Spectrogram sub-tab's overlay
- `eRPM` → Hz is `erpm * 100 / (poles / 2) / 60`. `motor_poles` comes from the
  raw header passthrough, defaulting to Betaflight's 14
- Overlay visibility is UI state (`ui::overlay_menu::OverlayVisibility`), a
  shared type with a separate instance per sub-tab, every family off by
  default. Toggling one never recomputes anything

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
  Only a bounded, evenly spread sample of them is retained (`max_traces`, 40);
  the mean and the metrics always come from every surviving window. The band is
  off by default: `spread_pct` is the same claim as a number, and 200 traces per
  axis per sublog parked hundreds of megabytes behind an unticked checkbox
- Metrics (`StepMetrics`) are measured on the mean curve, so the number and the
  drawn curve can never disagree. Below ten responses the panel says so
- Up to four flights are compared in this panel — the sidepanel keeps single
  selection, and comparison is a Step Response concept rather than an app-wide
  mode. The base flight is always the sidepanel's selection and always colour
  slot 0; colour is the slot, never the flight, and the chips carry identity.
  Metrics are prose at one flight and a colour-swatched grid at two or more.
  Which axes draw is the union across the compared flights, and every flight
  that cannot fill one says why, by name

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
    pub header: Metadata,           // current filter values, craft info
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
- [x] `parser/` — wrap `blackbox-log`, extract `Metadata` and timeseries
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
| Flights are named by `LogId`, never by index | `LogStore::remove` shifts every later index, and panel state the store cannot see would then redraw a different file under the old label |
| Panels reach other flights through a read-only catalog on `TabCtx` | A panel handed `&LogStore` could `select` or `remove` mid-frame while the sidepanel iterates it |
| Overlay geometry computed at load and stored on `Analysis` | It depends on the analysed window, which a visibility toggle does not change — and storing it puts the feature behind the existing loader integration seam instead of needing a new one |
| Overlays default to off, behind one dropdown | The panel opens as a clean spectrum. Every mark over the curve is one the pilot asked for, and a closed dropdown costs none of the vertical space three stacked axes need |
| One colour module (`app/colors.rs`) for axes, compare slots and overlays | Axis colour is Betaflight red/green/blue in every single-log tab; slot colour exists only where comparison lives. Both must read the installed palette, so light mode is not drawn in dark-theme accents |
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
