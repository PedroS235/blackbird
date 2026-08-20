# 01 — Decode `eRPM`, add the RPM channel

Status: done

`FlightData::rpm` is declared, hardcoded empty and never read. Make it real.

- `eRPM[n]` detected in `build_field_indices` alongside `motor`.
- `Channel::Rpm(index)` reaches it through the same accessor every other
  signal uses.
- `Metadata::motor_poles()` reads `motor_poles` from the raw header
  passthrough, falling back to 14 (Betaflight's default, and both fixtures').
- `Metadata::erpm_to_hz(erpm)` = `erpm * 100 / (poles / 2) / 60`.

Tests: field detection; the conversion including the missing-poles fallback.
