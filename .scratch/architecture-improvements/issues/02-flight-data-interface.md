Status: done
Type: task

# Give FlightData a real interface

`src/parser/flight_data.rs` is 20 public fields and zero methods — a struct,
not a module. Every consumer re-derives the same things from those fields.

## Files

- `src/parser/flight_data.rs` — the struct
- `src/analysis/spectral.rs` — `GyroNoiseAnalyzer::analyze`
- `src/app/mainview.rs` — 5 plot functions

## Problem

Duplication forced on callers by the bare-field interface:

- `let t0 = fd.time_us.first().copied().unwrap_or(0)` — 6 copies
  (`mainview.rs:184,240,298,359,458`, `spectral.rs:80`)
- relative-seconds conversion (`(t - t0) as f64 / 1e6`) — spelled out in both
  `spectral.rs:81` and `mainview.rs:185`
- `fd.rc_command[3]` — magic index for throttle, no name anywhere
- `fd.raw_gyro[i].as_deref()` — `Option<Vec<f64>>` unwrapping at every call site
- presence probes: `fd.vbat.is_some() || fd.current.is_some()`,
  `fd.debug[0..3].iter().any(Option::is_some)`

Nothing is hidden, so nothing can change: the field layout *is* the interface.

## Solution

Private fields (`pub(super)` so `parser/mod.rs` keeps its struct literal),
plus a public interface:

- `channel(Channel) -> Option<&[f64]>` — one accessor, `Channel` enum names
  every stream (`RawGyro(axis)`, `Gyro(axis)`, `Setpoint`, `RcCommand`,
  `Motor`, `Vbat`, `Rssi`, `Debug`…)
- named conveniences over it: `throttle()`, `gyro_raw(axis)`, `gyro(axis)`
- time: `start_us()`, `duration_s()`, `time_s()`, `time_us()`
- presence: `has_power()`, `has_rssi()`, `has_debug_axes()`
- `with_channel(Channel, Vec<f64>)` — construction for tests and future
  non-parser sources

## Seams under test (agreed)

1. `FlightData` public API — constructed in-test, no fixtures
2. `GyroNoiseAnalyzer::analyze` — the real consumer, proving the seam holds

## Comments

Built test-first 2026-08-08, five red→green slices (throttle → gyro accessors →
time helpers → presence queries → the analyzer seam). 12 new tests, suite at
48 passing.

The analyzer test earned its keep on the first run: a 200 Hz sine on
`RawGyro(0)` produced *no* peaks, because `with_time` set the timestamps but
left `sample_rate` at its default 0 Hz, so every frequency bin came out at 0
and the 30 Hz peak-search floor discarded everything. Fixed by deriving
`SampleRateEstimate::from_timestamps` inside `with_time` — the two can no
longer disagree. That failure mode was reachable through the old bare-field
struct too.

Fields are now `pub(super)`, so `parser/mod.rs` keeps its struct literal and
the rest of the crate goes through the interface. Ported both consumers:
`spectral.rs::analyze` dropped 10 lines to 4; `mainview.rs` lost 5 copies of
the `t0` idiom and 4 of the duration idiom.

`rpm` has no `Channel` variant yet — nothing writes it (`parser/mod.rs` fills
`Vec::new()`), so there was no behaviour to test. Add it when RPM telemetry
lands.
