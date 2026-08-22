# 04 — Keep the dwell, draw it on the plot floor

Status: done

`Dwell` is computed for every dynamic stage and dropped. Keep it on the
overlay and give it pixels.

- A dynamic stage's overlay carries its `Dwell` (bin centres + time fractions).
- Drawn as a filled histogram in a short lane along the bottom of the plot, in
  the owning chain's hue: a pinned notch is a spike, a roaming one a plateau.
- The lane holds its height under zoom — it is a strip of the plot, not a
  series in the data's dB.
- The dynamic notch's configured bounds, where there is no trace to average,
  become a marker in this same lane rather than a `Band` over the spectrum.

Tests: dwell weights sum to one; a single-setting dwell draws one bin; the lane
is absent for a log with no dynamic stage.
