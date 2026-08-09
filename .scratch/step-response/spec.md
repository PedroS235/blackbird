# Step response via Wiener deconvolution

Status: done

Replaces `src/analysis/step_response.rs` — a prototype that detected discrete
stick steps and averaged windows around them. It was never wired to the UI, and
its premise is wrong for real flying: freestyle logs contain almost no isolated,
sustained steps, so the detector either found nothing or averaged a handful of
unrepresentative events.

The new implementation deconvolves gyro from setpoint, the way PIDToolbox and
Blackbox Explorer do. Every part of the log where the sticks moved contributes.

## Method

For each overlapping window of the log:

1. Taper both signals with a Hann window, then FFT the setpoint (`S`) and the
   filtered gyro (`G`), zero-padded to avoid circular wrap-around.
2. `H = (G · conj(S)) / (|S|² + λ)` — Wiener deconvolution, λ regularising the
   frequencies where the setpoint carried no energy.
3. IFFT `H` → impulse response. Cumulative sum → step response.
4. Truncate to 500 ms, normalise so the steady-state tail sits at 1.0.

Windows that fail the quality mask are discarded. The survivors are kept
individually *and* averaged — the spread across traces is itself diagnostic.

## Decisions

| Decision | Value | Rationale |
|---|---|---|
| Algorithm | Wiener deconvolution | Uses all flight data, not just detectable steps |
| Input | `setpoint[axis]`, logged field only | Reconstructing from `rcCommand` needs rates, which `Metadata` does not parse |
| Missing setpoint | Render an explanation naming the field | Never fall back to `rcCommand` — wrong units, silently |
| Response signal | `gyro` (filtered, `gyroADC`) | What the PID loop saw; filter delay belongs in the measured response |
| Window | 1 s (was 2 s — see `.scratch/step-response-parity/`) | Long enough for low frequencies, short enough that the tune is constant across it; PID-Analyzer's `framelen` |
| Window function | Hann, applied to both signals (added by `step-response-parity`) | A rectangular cut leaks its edges into `t = 0` and the cumulative sum turns that into a head start. λ does not absorb it |
| Hop | window/16 (62.5 ms, was 0.25 s) | Dense stack for the all-traces view; PID-Analyzer's `superpos` |
| λ | `k · mean(|S|²)`, `k = 0.01` | Scale-relative, so it behaves the same on a cruise and on a flip |
| Window rejection | `max |setpoint| < 52 deg/s` (was 20) | No stick input means the deconvolution is noise; 20 admitted windows the reference tools reject |
| Steady-state acceptance | `0.5..=3.0` (replaced `min_steady_state = 1e-3`) | A trace settling outside it did not come from a craft; the old guard was sign-blind and accepted inverted traces |
| Throttle gating | none | Throttle-binning is the noise analysis's job; here it would discard most of a cruise log |
| Per-trace normalisation | Divide by mean of the last 100 ms of the response | Otherwise one drifting trace shifts the mean and overshoot becomes meaningless |
| Response length | 500 ms | Covers rise, overshoot and settle at FPV timescales |
| Axes | All three, stacked | Consistent with every other panel |
| Metrics (overshoot, rise, settle) | Not this pass | Land them once the curve has been eyeballed against real logs |

Every threshold above is a field on the analyser struct with these as defaults,
following `GyroNoiseAnalyzer`.

## Shape

- `signal/deconv.rs` — the deconvolution primitive. `signal/` owns "how to
  transform numbers"; `rustfft` must not appear in `analysis/`.
- `analysis/step_response.rs` — rewritten from scratch: windowing, masking,
  normalisation, averaging.
- `Analysis { spectral, step }` — a bundle computed at load time in `LogLoader`,
  stored in `LogStore`, handed to tabs as `TabCtx.analysis`. Without it every
  future analyser adds a field to three structs.

Traces are stored as `Vec<Vec<f64>>` plus one shared `Arc<[f64]>` time axis in
ms, mirroring `Psd`'s `freq_hz`. At most 100 evenly-spaced traces are *drawn*;
the mean is always computed from all of them.

## UI

PID Analysis → Step Response, currently a hardcoded-disabled button and a
"coming soon" label.

- Three stacked plots, one per axis.
- Individual responses in the axis colour at low alpha, mean in the same colour
  at full opacity and thicker.
- Checkbox "show individual responses", default **on** — the spread is the
  information.
- Trace count readout: "mean of N responses".
- A log without `setpoint` gets an explanation naming the field, not a silently
  disabled button. Presence is checked per axis, so a partly-logged craft still
  gets the axes it has.

## Issues

- `01-wiener-deconvolution-primitive.md` — `signal/deconv.rs` + unit tests
- `02-analysis-bundle-and-step-response.md` — `Analysis` bundle refactor, then
  the rewritten analysis module wired into `LogLoader`
- `03-step-response-panel.md` — the subtab

## Out of scope

- Rates parsing / setpoint reconstruction from `rcCommand`
- Overshoot, rise time and settling time metrics
- Comparing step responses across logs
