# 04 — Traced dynamic notch centre

Status: todo

`debug[0..3]` under debug mode `FFT_FREQ` is the tracker's live centre per
axis. Reduce it to a histogram over frequency and hang it off the dynamic
notch overlay as `OverlayShape::Traced(PerAxis<Option<TracedCenter>>)`.

- Gated on the same rule the Spectrogram sub-tab uses: debug mode is
  `FFT_FREQ` and the log has debug axes. Shared, not reinvented.
- Binned over the configured range where there is one — that is the band the
  tracker was allowed, so a tracker pinned at one end is visible as such.
- A log flown in another debug mode still gets the configured range band.

Tests: the loader test — present on the `FFT_FREQ` fixture, absent on the
other, with the configured range produced either way.
