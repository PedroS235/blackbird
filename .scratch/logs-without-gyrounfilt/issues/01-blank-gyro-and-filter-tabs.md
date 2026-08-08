Status: todo
Type: task

# A log without `gyroUnfilt` opens on a blank panel and never says why

From the code review of `architecture-improvements/04-tab-modules`. The bug
predates that ticket — the refactor only made the assumption legible.

## Files

- `src/parser/mod.rs:403` — `gyroUnfilt` is the only source of `raw_gyro`
- `src/parser/flight_data.rs` — `gyro_raw`, `gyro`
- `src/analysis/spectral.rs` — `SpectralAnalysis::analyze`
- `src/app/tabs/timeseries/gyro.rs` — draws from `gyro_raw` only
- `src/app/tabs/timeseries/mod.rs` — `Available::has`, `resolve`

## Problem Statement

`raw_gyro` is populated only from the `gyroUnfilt` field, which Betaflight
logs only when the pilot has enabled the pre-filter gyro debug. A log with
`gyroADC` but no `gyroUnfilt` is perfectly ordinary, and today it opens like
this:

- **Gyro tab** — `gyro::show` hits `let Some(raw) = fd.gyro_raw(axis) else {
  continue }` for all three axes and draws nothing. No plots, no message.
- **Filter Analysis** — `SpectralAnalysis::analyze` skips axes without
  `gyro_raw`, so every sub-tab is empty for the same reason.
- **PID Analysis** — works, because it reads `gyro`.

The Gyro tab is also the app's designated safe harbour: `Available::has`
hardcodes `Gyro => true`, and `resolve()` sends the user there when the log
lacks power or RSSI data. So the pilot is actively redirected onto the blank
panel, with the Timeseries tab bar showing Power & Battery and Receiver RSSI
greyed out and Gyro selected and empty.

The unit test `gyro_is_always_available` is about tab *selectability*, which
remains true. It is not asserting that the panel renders anything.

## Open Questions

Three shapes, and the choice is a product decision, not a cleanup:

1. **Fall back to the filtered trace.** `gyro::show` draws `gyro` when
   `gyro_raw` is absent, labelled so the pilot knows which they are looking
   at. Cheapest, and keeps Gyro a genuine safe harbour. Does nothing for
   Filter Analysis, where the raw signal is the point.
2. **Make availability data-driven.** No tab is unconditionally available;
   `resolve()` picks the first tab the log can actually render. Honest, but
   needs an answer for a log that can render nothing.
3. **Say why.** Keep the current selection rules and render an explanation
   naming `gyroUnfilt` and how to enable it (`set debug_mode = GYRO_SCALED`
   or the equivalent for the firmware). Speaks FPV, costs the least state.

1 and 3 compose. Worth deciding whether Filter Analysis gets the same
treatment, since its emptiness has the same cause and a different remedy.

## Out of Scope

- Synthesising a raw trace from the filtered one.

## Comments

Recorded 2026-08-08 from the review of ticket 04. Confirmed against
`parser/mod.rs:403` (only writer of `raw_gyro`) and `analysis/spectral.rs`
(skips axes without it).
