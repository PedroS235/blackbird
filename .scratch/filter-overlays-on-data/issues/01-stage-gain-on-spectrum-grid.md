# 01 — Every stage's gain, precomputed on the spectrum's grid

Status: done

The chain total is a per-frame product, so each stage's contribution has to be
a plain array of power gains on one shared grid — the PSD's own, so the fill's
two edges share x with no resampling.

- `filter_response::cascade(stages: &[&[(Stage, f64)]], freq_hz, fs) -> Vec<f64>`:
  at each frequency, the product over stages of that stage's expected power
  gain (the weight-sum over its settings). A static stage is one `(stage, 1.0)`
  pair, so `weighted` becomes the one-element case and `of` still builds on it.
- Document at the call site that a product of expected gains treats stages as
  independent, which two throttle-tracking dynamic stages are not.
- Each `FilterOverlay` carries its own gain on that grid — per axis where the
  shape is `Traced`, since the dynamic notch has one centre per axis.
- Grid comes from the axis's `raw_psd.freq_hz` (`Arc<[f64]>`, already shared).

Tests: cascade of two stages equals the product of their gains in power; a
one-element cascade equals `weighted`; a stage whose gain is unity everywhere
leaves the total unchanged.
