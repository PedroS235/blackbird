# 05 — Name a dynamic stage by the range it really used

Status: done

- `Gyro LPF1 (dyn, 180–420 Hz)`, from the p5–p95 of the realised cutoffs, the
  same percentile rule the harmonic bands use.
- With no throttle logged there is no realised range: fall back to the
  configured min..max and say `config` in the label, so the two are never
  confused.
- In that no-throttle case `OverlayShape::Band` is replaced by an envelope —
  two real rolloff curves at the configured extremes. `Band` is then unused for
  the LPF; the dynamic notch's bounds move to the dwell lane (04), so the
  variant can go.
- `OverlayShape::Line` stays as-is.

Tests: the label carries realised percentiles when throttle exists and the
configured range with `config` when it does not; the envelope is two curves,
not a span.
