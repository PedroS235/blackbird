# 03 — D-term stages: present, unanchored, own reference

Status: done

The PSD plots gyro power. D-term stages never touched it, so they must not be
drawn as if they had.

- `Notch(Dterm)` and `Lowpass(Dterm)` draw gain against frequency, hanging from
  a reference line at unity gain, in the D-term hue.
- That line is `response_anchor_db`'s value repurposed and labelled — it is the
  D-term chain's 0 dB, drawn only while a D-term family is visible.
- No fill, no anchoring to the raw curve, no contribution to the gyro chain
  total.

Tests: a D-term overlay's drawn y is independent of the raw spectrum; the
reference line is absent when both D-term families are hidden.
