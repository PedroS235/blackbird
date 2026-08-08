Status: done
Type: task

# Wiener deconvolution primitive in `signal/`

First of three for `../spec.md`. Pure math, no callers yet.

## Files

- `src/signal/deconv.rs` — new
- `src/signal/mod.rs` — declare the module

## Problem

`SignalAnalyzer` plans forward FFTs only. The step response needs an inverse
FFT and a regularised spectral division, and neither belongs in `analysis/`,
which is policy over `signal/` primitives everywhere else in the codebase.

## Solution

`WienerDeconvolver`, constructed once per window length and reused across every
window of a log:

```rust
pub struct WienerDeconvolver { /* len, padded len, lambda_k, forward, inverse */ }

impl WienerDeconvolver {
    pub fn new(len: usize, lambda_k: f64) -> Self;

    /// Recovers `h` from `output ≈ h * input`, both `len` samples long.
    pub fn impulse_response(&self, input: &[f64], output: &[f64]) -> Vec<f64>;
}
```

- FFT length is `(2 * len).next_power_of_two()` with both signals zero-padded.
  Linear convolution, not circular — without the padding the tail of the window
  wraps onto the head and corrupts the first milliseconds of the response,
  which is precisely the region the panel is about.
- No taper. Windowing both signals would bias the recovered gain; zero-padding
  removes wrap-around without touching the amplitudes.
- `λ = lambda_k · mean(|S|²)` over the padded spectrum, so the regularisation
  tracks how hard the pilot was moving the sticks.
- The denominator is floored, so an all-zero input yields zeros rather than
  `NaN`. Callers reject those windows anyway, but a primitive that emits `NaN`
  poisons an average silently.

Cumulative sum, truncation and normalisation are policy and stay in issue 02.

## Tests

- Recovers a known short FIR from broadband input within tolerance
- Recovers a known first-order lag: the cumsum of the result matches
  `1 - exp(-t/τ)`
- All-zero input produces finite output
- Larger `lambda_k` attenuates the recovered response (regularisation biases
  towards zero, as it should)
