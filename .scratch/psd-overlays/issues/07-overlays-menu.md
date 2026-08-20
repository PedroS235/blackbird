# 07 — The overlays menu and per-family rendering

Status: todo

- A dropdown above the plots, one section per family, every family off by
  default. Costs no vertical space closed.
- `OverlayVisibility` is a shared type with a separate instance per sub-tab —
  the PSD and Frequency sub-tabs once shared a field and toggling one toggled
  the other.
- A family the log cannot fill greys out with a stated reason, following the
  tab bar's law.
- Bands draw as `egui_plot::Span`; a zero-weight harmonic draws as an outline
  with no fill.
