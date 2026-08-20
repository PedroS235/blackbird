# 06 — Overlay colours into the colour module

Status: todo

`PEAK_MARKER_COLOR` and `FILTER_MARKER_COLOR` are hardcoded per panel and do
not follow the theme — the exact defect `app/colors.rs` exists to prevent.

- `peak_color`, `filter_color`, `harmonic_color(order)` in `colors.rs`.
- Harmonic orders get a fixed hue sequence, as the compare slots do, so an
  order keeps its identity across a theme switch.
- All join the existing contrast-against-background and mutual-distinctness
  tests, in both palettes.
