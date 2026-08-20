# 02 — Replace `FilterMarker` with the overlay type

Status: done

`FilterMarker` is a line: label, centre, optional cutoff. Every filter that has
width is drawn as a guess at a midpoint, and the PSD panel selects gyro-only
markers by `label.starts_with("Gyro")`.

- `FilterOverlay { label, family, shape }`; `OverlayFamily` carries the
  gyro/D-term loop, so family membership stops being a label convention.
- `OverlayShape::{Line, Band, Harmonics, Traced}`.
- A notch emits a band of width `centre / Q`; a dynamic lowpass emits its
  dynamic min..max instead of collapsing to the ceiling; the dynamic notch
  emits its configured range as a band.
- Static notches only when enabled (`centre > 0`) — already filtered in the
  parser, keep it true here.

Tests: the notch bandwidth derivation, the dynamic lowpass band.
