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
├── lib.rs                   ← the library half: everything but the UI
├── loader.rs                ← paths in, parsed-and-analysed logs out
├── version.rs               ← is a newer release out, and which asset is ours
│
├── parser/
│   ├── mod.rs               ← wraps blackbox-log, field detection, header decode
│   ├── metadata.rs          ← Metadata + FilterConfig, RateConfig, pole count
│   ├── flight_data.rs       ← FlightData, Channel, Axis, PerAxis, Trimmed
│   └── sample_rate.rs       ← sample rate estimated from the timestamps
│
├── signal/                  ← FFT, Welch passes, deconvolution, downsampling
│
├── analysis/
│   ├── mod.rs               ← Analysis struct, orchestrates the analysers
│   ├── filter_response.rs   ← what a stage does to a frequency: notch V, LPF rolloff
│   ├── overlays.rs          ← filter geometry: responses, bands, harmonic groups
│   ├── spectral.rs          ← PSD, peaks, throttle/time-binned maps
│   ├── step_response.rs     ← Wiener deconvolution, windowing, averaging
│   └── filter_delay.rs      ← cross-correlation, delay estimation in ms (planned)
│
├── ai/                      ← planned; see the AI integration section
│
└── app/
    ├── mod.rs               ← App struct, central state, load events
    ├── colors.rs            ← axis, compare-slot and overlay palettes
    ├── log_store.rs         ← LogId, FlightKey, the read-only FlightCatalog
    ├── sidepanel.rs         ← file list and selection
    ├── update.rs            ← the check thread, and the strip that offers it
    ├── ui/                  ← widgets shared across tabs
    │   ├── compare.rs       ← compare chips and picker
    │   ├── harmonic_key.rs  ← hue per motor, style per order, and its legend
    │   ├── heatmap.rs       ← heatmap rendering
    │   ├── overlay_menu.rs  ← which overlay families a panel is drawing
    │   └── timeseries_plot.rs
    └── tabs/
        ├── timeseries/      ← gyro, power/battery, RSSI
        ├── filter_analysis/ ← PSD, Frequency, Vs Reference, Spectrogram
        └── pid_analysis/    ← step response, gyro vs setpoint
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
- **Two curves on one plot share one dB reference.** A `Psd` is dB relative to
  a level, and `SpectralView` hands out both halves of that: `peak_db()` is
  this pass's loudest bin, `psd_relative_to(db)` normalises against someone
  else's. The filtered gyro is normalised against the *raw* peak, never its
  own — its own would lift it by exactly what the filters took off the loudest
  bin, which is 10 dB on the pitch of a hover log and draws the filtered trace
  above the raw one at frequencies no filter touches. Any second signal added
  to that plot takes the same reference. `FrequencyPeak::attenuated_db` is the
  difference of these two curves, so it is only the real attenuation while
  they share a scale

### Filter overlay geometry

`analysis::overlays` computes what each filter actually occupies in frequency,
at load time, stored on `Analysis`. It is not a pure function the panel calls
per frame: the geometry depends on the analysed window, which is fixed at
load, and storing it puts the feature behind the loader integration seam.

- One `FilterOverlay { label, family, shape }`. `OverlayFamily` carries the
  gyro/D-term loop, so a panel selects gyro overlays by matching the type
  rather than by `label.starts_with("Gyro")`
- `OverlayShape::Response` — what a filter took off, per frequency, from
  `analysis::filter_response`. A notch is a V and a lowpass is a rolloff;
  drawn as a line or a band, both read as "everything here is gone", which is
  the one thing neither does. Static notches and every lowpass stage draw this
- `OverlayShape::Traced` — the same, where it had to be measured per axis: the
  dynamic notch, whose centre Betaflight logs one of per axis. Read from
  `debug[0..3]`, gated on `Metadata::logs_dyn_notch_trace()` (debug mode
  `FFT_FREQ`) — the one rule, shared with the Spectrogram sub-tab's overlay
- **A filter that moved is averaged over the settings it moved through**,
  weighted by how long it spent at each and averaged **in power, not in
  decibels**. A frequency notched hard for a tenth of the flight and untouched
  for the rest kept nine tenths of its energy, which averaging decibels would
  report as a cut it never got. So a dynamic notch pinned at one frequency
  draws a deep narrow V and a roaming one draws a broad shallow trough; a
  dynamic LPF is averaged over the cutoffs the *throttle* actually produced,
  through Betaflight's own `dynLpfCutoffFreq` curve
- `OverlayShape::Band` — only what a filter is *allowed* to do: the dynamic
  notch's configured bounds, and a dynamic LPF whose log has no throttle
- `OverlayShape::Line` — only a notch whose cutoff cannot give a Q, so its
  shape cannot be derived. Everything sizeable draws its response
- `OverlayShape::Harmonics` — one band per motor per order, from `eRPM`, over
  the frequencies that motor *spent the window at*: the 5th to 95th percentile
  of its running eRPM, not its two extremes. The full excursion of a freestyle
  log runs from idle to full song, and three orders of that wash over most of
  the spectrum — a band covering everything can never say a peak is *not* motor
  noise. Samples are uniform in time, so a rank over them is already
  time-weighted. Order count comes from `RpmFilterConfig::harmonics`, clamped
  to Betaflight's own maximum of three; a zero-weight order is flagged
  unfiltered. Stopped-motor samples are excluded, so no band runs down to 0 Hz
- **Hue is the motor, line style is the harmonic order** — solid, dashed,
  dotted for the fundamental and its two multiples, one scheme in
  `ui::harmonic_key` that the PSD's spans and the Spectrogram's curves both
  read. Order is derivable from the frequency axis and spends a colour on a
  fact the pilot can already see; which of four motors is loud is the actual
  diagnosis — a bent shaft, a chipped prop, a dying bearing — and nothing else
  on the plot says it. Four hues, cycled so a hex still draws. A tracked but
  zero-weight order keeps its identity and is dimmed
- The PSD draws each band as **the spectrum's own curve, recoloured** across
  those frequencies — not a span. Twelve spans are twenty-four vertical edges,
  and a bracket reads as a boundary rather than as noise; every point of a
  recoloured run is measured data, saying *this part of your spectrum is motor
  3's second harmonic*. A peak with no coloured run over it is a peak no motor
  explains, which is what narrowing the bands was for. Drawn thicker than the
  raw trace, widened by an FFT bin either side so a motor that held one
  frequency still draws a segment, and skipped where the band falls off the end
  of the spectrum. No in-plot labels: a legend row above the plots keys them
  while the family is on
- The Spectrogram draws the same identities as **curves of frequency against
  time** — the PIDToolbox view. Built per frame from `eRPM` rather than stored:
  the panel already holds the flight data, and the series borrows the samples
  as logged with a `scale` applied after decimation, which min-max decimation
  commutes with. Clipped to the heatmap's own frequency range, so a third
  harmonic above Nyquist cannot stretch the plot's bounds
- Responses are modelled at **`Metadata::filter_rate_hz`, the PID loop rate**,
  not the logging rate — a log written every second frame would otherwise show
  every stage rolling off far earlier than it does. They are the discrete
  filters Betaflight runs, so a stage's real corner sits somewhat below its
  configured one once that cutoff is a sizeable fraction of the loop rate.
  That gap is a true thing about the tune, and the curve keeps it
- `eRPM` → Hz is `erpm * 100 / (poles / 2) / 60`. `motor_poles` comes from the
  raw header passthrough, defaulting to Betaflight's 14
- Overlay visibility is UI state (`ui::overlay_menu::OverlayVisibility`), a
  shared type with a separate instance per sub-tab, every family off by
  default, toggled from an inline wrapped row above the plots. Toggling one never recomputes anything. Detected peaks get a switch
  there too, and default off with the rest — they are not a filter and so not
  an `OverlayFamily`, but to a pilot they are one more thing drawn over the
  curve
- The menu is passed **the families the panel can draw**, not all of them: the
  Spectrogram lists Harmonics and Dyn notch and nothing else, and takes no
  peaks switch, because a greyed toggle there would blame the log for a shape
  the panel was never going to draw. Which makes the dynamic notch trace —
  drawn unconditionally before — opt-in like everything else, the accepted cost
  of one rule for every overlay on the panel

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

### Logging

`tracing` throughout, one `tracing_subscriber` installed by `logging::init_logging`
from `main`. A pilot's bug report is a paste of this output, so it is written to
be read by someone who is not holding the code.

- **`RUST_LOG` wins whenever it is set** — `RUST_LOG=blackbird::parser=trace`
  to chase one module, `RUST_LOG=debug` to hear eframe and wgpu too. Unset, the
  build decides: `blackbird=debug` in a dev build, `blackbird=info` in a
  release one. Only this crate is on by default; wgpu logs per frame and would
  bury the lines that matter
- **`info` is the flight's story, `debug` is the mechanism.** One `info` per
  file opened, per sublog parsed (frames, sample rate, duration, parse ms), per
  file finished with its total; `debug` for header contents, field count, per-
  analyser timings and per-axis results. So the default release log reads as a
  timeline of what the pilot did
- **A field Betaflight never logged is a `warn`, once, at parse time**
  (`parser::warn_about_missing_fields`): no `gyroUnfilt` means no pre-filter
  spectrum, no `setpoint` means no step response, no `eRPM` means no harmonic
  bands. From the panel these look like bugs rather than a `blackbox_fields`
  setting, and the log is where that distinction can be made cheaply
- **`warn` for what one flight lost, `error` only for what the pilot sees.**
  A corrupt sublog in a `.bbl` is a `warn` and the other seven still load;
  `App::notify` is the only `error` path, so every error line has a matching
  notification on screen and vice versa
- **Nothing logs per frame.** Every call site is a load, a parse, an analyser
  run, or a click — `ui`/`tabs` stay silent, because a line per frame at 120 Hz
  is not a log, it is a leak
- **`BLACKBIRD_LOG_FILE` writes the same output to a file.** The Windows build
  targets the `windows` subsystem, which has no console attached, so stdout
  there goes nowhere and `RUST_LOG` alone produces nothing a pilot can paste
- The update check is the one exception to all of this: every failure there is
  `debug` and nothing reaches the UI (see above)

### Update check

One unauthenticated `GET api.github.com/repos/PedroS235/blackbird/releases/latest`
on startup, on a plain `std::thread` — `blackbird::version` does the compare,
`app::update` owns the thread and the strip.

- It **offers**, it never self-updates. The release assets are bare, unsigned
  binaries: replacing one in place would mean defeating Gatekeeper on macOS,
  renaming a running `.exe` on Windows, and guessing whether a Linux install
  came from a package manager — with no checksum or signature to verify the
  download against. Self-update is its own milestone, after signing
- **It resolves the asset for the platform it is running on**: `env::consts::OS`
  and `ARCH` are mapped to `release.yml`'s own names — note `aarch64` → `arm64`
  — and the name is looked up in the release's asset list. No asset for this
  platform (32-bit, BSD, anything outside the matrix) falls back to the release
  page. `asset_name` and `newer` take `os`/`arch` as parameters, so every
  target's name is a unit test on one machine
- **Every failure is silence.** Offline, rate-limited, renamed repo, a tag that
  is not a version, a release older than this build: `debug!` and nothing in the
  UI. The pilot opened the app to read a log
- `ureq` + rustls, not `reqwest`: one blocking GET does not justify a tokio
  runtime, and rustls keeps the binary self-contained — native-tls would
  dynamically link OpenSSL and break the drop-it-anywhere promise on any distro
  with a different libssl. Nothing links OpenSSL now, so `release.yml` no longer
  installs `libssl-dev`
- The strip is a `Panel::top` above everything, dismissible for the session
  only. Dismissal is the absence of state, not a flag — the offer is dropped, so
  it returns on the next launch. Persisting a "skip this version" would need the
  settings store that does not exist yet
- **`Cargo.toml`'s version is the single source of truth**, and the `verify` job
  in `release.yml` fails a tag that disagrees with it before any of the six
  builds start. So **tagging a release needs a version-bump commit first**.
  Without that gate the checker silently lies: the tree sat at 0.1.0 through six
  releases, and every user would have been told forever that an update was
  waiting

### Releases

Pushing a `v*` tag runs `release.yml`: `verify` (tag against `Cargo.toml`) →
six target builds → `release`. Releasing is therefore: bump `Cargo.toml`,
commit, tag, push. Nothing else is done by hand.

- **The release notes and the changelog are generated at release time**, by
  git-cliff from the commits, off a `fetch-depth: 0` checkout — a shallow clone
  has no tag graph and cliff would emit an empty section
- `CHANGELOG.md` is **not tracked** (it is in `.gitignore`) and ships as a
  release asset. It is derived from the commits, so a copy in the tree only ever
  drifts from the tag it claims to describe, and regenerating it by hand is a
  step that gets forgotten
- The notes are written by `gh release edit --notes-file`, not by the upload
  action, which left the body empty across three runs with `body_path` and
  `body` both set. The upload action ships the binaries; the notes are a
  separate, verifiable write
- The notes body overrides only cliff.toml's `body`, dropping the
  `## [x.y.z] - date` heading — GitHub already titles the release with the tag.
  Groups, parsers and filters still come from `cliff.toml`, so the file and the
  notes can never disagree about what a `fix(psd):` is
- Which means **the commit subject is the release note**. A subject without a
  conventional prefix is dropped from the notes entirely (`filter_commits`), and
  the scope in `feat(psd):` is what the pilot reads on the Releases page

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
| Overlays default to off, behind an inline toggle row | The panel opens as a clean spectrum, so every mark over the curve is one the pilot asked for. The toggles are laid out inline rather than in a dropdown: a button that opens a menu announces nothing, and with every family off there is no mark on the plot to hint that more exists. One wrapped row is the whole cost |
| Harmonic identity is hue-per-motor plus style-per-order | Order is derivable — the second harmonic is at twice the first and the pilot can see that. Which motor is loud is not derivable and is the diagnosis. Colouring by order spent the only distinguishing channel on the less useful of the two facts, and left four motors indistinguishable |
| A harmonic is drawn as a recoloured stretch of the spectrum, not a band | Twelve spans were twenty-four vertical edges over the curve the pilot came to read, and a bracket says "boundary" where the question is "how loud". Recolouring the real curve makes every drawn point measured data, and leaves a peak no motor explains with no colour over it |
| Harmonic bands are a percentile, not an extent | On any real freestyle log the full min..max runs idle to full song, so three orders of it overlap into a wash that covers most of the spectrum. Every peak lands inside one, so every peak looks motor-explained and the overlay says nothing |
| Spectrogram harmonics are built per frame, not stored | The panel already holds the flight data, and the dynamic notch trace set the precedent. The series borrows eRPM as logged and carries a `scale` applied after decimation — min-max decimation commutes with a positive scale, so nothing is copied per frame |
| Raw and filtered PSDs share the raw peak as their dB reference | Each normalised to its own peak is two scales on one plot, and the quieter curve is lifted by whatever the filters removed at the loudest bin — a hover log drew filtered 10 dB above raw on pitch. Roll and throttle-ramp logs hid it: their loudest bin survives filtering, so the two references happened to agree |
| One colour module (`app/colors.rs`) for axes, compare slots and overlays | Axis colour is Betaflight red/green/blue in every single-log tab; slot colour exists only where comparison lives. Both must read the installed palette, so light mode is not drawn in dark-theme accents |
| The harmonic mark's *hue and style together* live in `ui/harmonic_key.rs`, not `colors.rs` | `colors.rs` stays the palette — which hue is motor 3, in this theme. But a harmonic mark's identity is a hue *and* a line style, and a line style is not a colour: splitting the pair across two modules is exactly how the PSD's spans and the spectrogram's curves would come to disagree. `harmonic_key` asks `colors` for the hue and owns nothing else about the palette |
| One `tracing` subscriber, `RUST_LOG`-overridable, crate-only by default | A bug report is a paste of this output. Dependencies logging per frame bury the parse and analysis lines it exists to show, and a pilot can still widen it without a rebuild |
| A missing log field warns at parse time, not where it is used | `gyroUnfilt` off means every spectral panel is empty for a reason no panel can name. Said once, at load, in the one place that knows what was in the file |
| Update check offers a download, never self-replaces | Assets are unsigned bare binaries and the install source is unknowable — a self-updater would fight Gatekeeper, a running `.exe`, and package managers, with nothing to verify the download against |
| `ureq` + rustls for the update check, not `reqwest` | One blocking GET does not justify pulling in a tokio runtime, and rustls keeps the single binary free of a libssl dynamic link |
| Changelog generated at release time, not tracked | It is derived from the commits. A generated file in the tree drifts from the tag it describes, and regenerating it by hand was a step that got skipped — the 0.7.0 release shipped with its section still headed `[unreleased]` |
| Tag-vs-`Cargo.toml` gate as the first CI job | The version the binary reports is what the update check compares; drift makes it lie to every user. Fails in seconds instead of after six matrix builds |
| Frames are presented vsynced, `BLACKBIRD_PRESENT` overrides | Windows DX12 honours `Immediate`, and presenting uncapped starved the compositor until the whole desktop froze — measured over 900 frames per arm, DX12-uncapped was the only red one: Vulkan uncapped and DX12 vsynced were both clean. Linux never showed it because Wayland/RADV has no `Immediate` to honour and had been silently clamping to 59.8 fps all along. A log analyser gains nothing from frames the monitor cannot show |
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
