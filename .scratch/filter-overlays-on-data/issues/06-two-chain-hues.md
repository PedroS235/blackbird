# 06 — One hue per loop in `colors.rs`

Status: done

- Replace the single `filter_color` with a gyro hue and a D-term hue, both from
  the installed palette so light mode is not drawn in dark-theme accents.
- Within a chain, separation is width and alpha: total at full weight,
  per-stage curves thin and dimmed, fill and dwell lane at low alpha.
- `harmonic_key` is untouched — motor hue and order style stay its business.

Tests: the two hues differ in both themes; nothing outside `colors.rs` names a
filter colour literal.
