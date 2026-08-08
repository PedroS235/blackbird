Status: done
Type: task

# `Analysis` bundle, then the step response module

Second of three for `../spec.md`. Blocked by 01.

## Files

- `src/analysis/mod.rs` — new `Analysis` bundle
- `src/analysis/step_response.rs` — deleted and rewritten
- `src/loader.rs` — computes the bundle
- `src/app/log_store.rs` — stores the bundle
- `src/app/tabs/mod.rs` — `TabCtx.analysis`
- `src/app/tabs/filter_analysis/*` — read `ctx.analysis.spectral`

## Two commits

### 1. The bundle, alone

`struct Analysis { spectral: SpectralAnalysis, step: StepResponseAnalysis }` —
introduced wrapping *only* `spectral` at first, with `LogStore`, `TabCtx` and
the filter panels rethreaded onto it and the suite green either side. A pure
rename-and-rethread is trivially reviewable; mixed with new math it is not.

### 2. The analysis module

`src/analysis/step_response.rs` is deleted in the same commit that adds its
replacement. It is in git; leaving it invites accidental reuse of its
constants.

```rust
pub struct StepResponseAnalyzer {
    pub window_s: f64,          // 2.0
    pub hop_s: f64,             // 0.25
    pub lambda_k: f64,          // 0.01
    pub min_setpoint_dps: f64,  // 20.0
    pub response_ms: f64,       // 500.0
    pub tail_ms: f64,           // 100.0
}

pub struct AxisStepResponse {
    pub time_ms: Arc<[f64]>,
    pub traces: Vec<Vec<f64>>,
    pub mean: Vec<f64>,
}

pub struct StepResponseAnalysis { /* PerAxis<Option<AxisStepResponse>> */ }
```

Per axis, per window: mask on `max |setpoint| ≥ min_setpoint_dps`,
`WienerDeconvolver::impulse_response`, cumulative sum, truncate to
`response_ms`, divide by the mean of the last `tail_ms` (guarded against
near-zero), push. Mean is the pointwise average of every surviving trace.

An axis is `None` when it has no `setpoint` or no `gyro`, or when no window
survived the mask — the panel distinguishes these cases from an axis that was
never logged.

Computed at load time in `LogLoader`, alongside the spectral analysis, per
sublog. Roughly 80 traces × 500 samples × 3 axes ≈ 1 MB per sublog.

## Tests

- Synthetic round-trip: a setpoint pushed through a known second-order system
  yields a mean curve whose overshoot matches the system's analytic overshoot
- Flat setpoint yields an axis with no traces
- A log shorter than one window yields an empty result, not a panic
- A response whose tail is ~0 does not produce infinities
- Through `LogLoader` on `tests/fixtures/eight_logs_in_one.bbl` — whether this
  asserts real traces or the no-setpoint path depends on whether that fixture
  logs `setpoint`; verify before writing the assertion
