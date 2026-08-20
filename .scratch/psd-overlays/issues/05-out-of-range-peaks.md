# 05 — Out-of-range peak classification

Status: done

A peak outside the dynamic notch's configured range is noise the tracker can
never touch. Decided in analysis, not in the panel.

- `FrequencyPeak::dyn_notch_reach: Option<DynNotchReach>` — `None` with no
  dynamic notch configured, else `Inside`/`BelowMin`/`AboveMax`.
- `AxisSpectral::peaks_outside_dyn_notch()` for the prose count.

Tests: classification against a constructed filter config.
