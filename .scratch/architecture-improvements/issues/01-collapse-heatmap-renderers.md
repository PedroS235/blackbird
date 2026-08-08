Status: done
Type: task

# Collapse the two heatmap renderers into one deep module

From the 2026-08-05 architecture review (candidate 1 of 4; candidates 2 and 3
were implemented directly — see git log for `LogStore` and `MainViewState`).

## Files

- `src/app/mainview.rs` — `show_binned_heatmap`, `show_time_heatmap`, `heat_color`

## Problem

Two ~60-line functions duplicate the same pixel-buffer-to-texture math,
differing only in which axis is transposed (freq × throttle vs freq × time) —
a shallow pair, not one module.

## Solution

One `Heatmap` module, shaped like the existing `TimeseriesPlot` in
`src/app/ui/timeseries_plot.rs` — takes a `BinnedSpectrum` and an
orientation, owns the texture id, the dB-to-color mapping, and the
transpose. Two call sites (throttle map in `show_vs_reference_tab`, time map
in `show_spectrogram_tab`) justify the seam as real, not hypothetical.

## Wins

- Interface shrinks to one struct + `.show(ui)`
- Locality: fix a color-scale bug once, not twice
- Leverage: the spectrogram's tracked-frequency-line overlay becomes one
  optional field, not a bespoke branch

## Comments

Grilled and implemented 2026-08-06. `src/app/ui/heatmap.rs`: `Heatmap<'a>`
struct (plain fields, `.show(&self, ui)`, mirrors `TimeseriesPlot`),
`HeatmapOrientation` enum (`VsThrottle`/`VsTime`, bakes in axis labels),
`OverlaySeries<'a>` for the tracked-frequency line. One shared pixel-fill
loop and one `PlotImage` construction, parameterized on orientation rather
than matched into two loop bodies. `heat_color` moved in as a private fn.
Both call sites in `mainview.rs` (`show_vs_reference_tab`,
`show_spectrogram_tab`) now build a `Heatmap` literal and call `.show(ui)`;
`show_binned_heatmap`/`show_time_heatmap`/free-fn `heat_color` deleted.
