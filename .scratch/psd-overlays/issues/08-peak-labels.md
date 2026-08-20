# 08 — Peak label cull, attenuation, out-of-range recolour

Status: todo

- At most the three strongest peaks by amplitude carry a label; the rest are
  unlabelled lines.
- Each peak is one mark, not a labelled line plus floating text saying the
  same thing.
- A labelled peak states its post-filter attenuation — a value the analysis
  already computes and nothing displays.
- Out-of-range peaks take the palette warning colour, and one prose line under
  the plot states the count and the bound they exceeded.

Tests: plot id stability and overlays-default-off in the PSD panel.
