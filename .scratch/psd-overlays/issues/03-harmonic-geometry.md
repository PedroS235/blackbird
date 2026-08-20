# 03 — Harmonic group geometry

Status: done

Per motor per harmonic order, the minimum and maximum frequency that motor
reached over the analysed (trimmed) window, plus whether that order's RPM
filter weight is non-zero.

- Order count comes from `RpmFilterConfig::harmonics`, not a constant. With
  eRPM but no RPM filter, only the fundamental is drawn, unfiltered.
- Samples where the motor is stopped are excluded — a band running from 0 Hz
  describes the ground, not the flight.
- Absent `eRPM` produces no harmonics overlay at all, which is what greys the
  menu entry out.

Tests: the zero-weight flag; end to end against both fixtures.
