Status: done
Type: task

# Converge the tab bars and the two heatmap panels

From the code review of `04-tab-modules`. That ticket split the tabs into
modules; it did not converge what the split then put side by side.

## Files

- `src/app/tabs/mod.rs` — `show_tab_bar`, `stacked_plot_height`
- `src/app/tabs/timeseries/mod.rs` — tab bar, `Available`/`resolve`
- `src/app/tabs/filter_analysis/mod.rs` — tab bar
- `src/app/tabs/filter_analysis/vs_reference.rs`
- `src/app/tabs/filter_analysis/spectrogram.rs`
- `src/app/tabs/filter_analysis/frequency.rs`
- `src/app/tabs/pid_analysis/mod.rs` — tab bar

## Problem Statement

**Four tab bars, two idioms.** `tabs/mod.rs` and `filter_analysis/mod.rs` use
`ui.selectable_label`; `timeseries/mod.rs` and `pid_analysis/mod.rs` use
`egui::Button::selectable` + `add_enabled`. They render differently, and only
the second can express a disabled tab. Which idiom a new tab bar gets is
currently decided by which file you copied from.

**Two heatmap panels, one shape.** `vs_reference.rs` and `spectrogram.rs`
each carry an identical `floor_db: -60.0` field, an identical sensitivity
slider (`-120.0..=-5.0`, same label, same suffix), and a `Heatmap { .. }`
block differing only in orientation, source map, and overlay. Changing the
sensitivity range means editing two files that never reference each other.

**Two pre-existing height quirks**, preserved verbatim by the refactor and
worth fixing while in here:

- `frequency.rs` calls `stacked_plot_height(ui, 3)` *before* adding its
  slider, so the three plots are sized against height the slider then takes.
  `vs_reference.rs` and `spectrogram.rs` measure after their slider, which is
  the correct order. Original: `HEAD~3:src/app/mainview.rs:553`.
- Every caller passes a literal `3` even though axes without data are
  skipped, so a log with only roll renders one plot at a third height above
  two thirds of empty panel.

## Solution Sketch

One tab-bar helper taking `(label, enabled)` pairs and owning the selection
write-back — `add_enabled` for all four, since a disabled tab is a thing the
app needs and `selectable_label` cannot show it. One shared heatmap-panel
type holding the floor slider and the `Heatmap` construction, parameterised
by orientation, map accessor, and optional overlay. `stacked_plot_height`
takes the count of rows actually about to be drawn.

## Out of Scope

- The availability trichotomy. `Available`/`resolve` (timeseries), a
  hardcoded `enabled: false` (Step Response), and silently skipping axes
  without a `time_map` (Spectrogram) are three answers to one question, but
  Step Response gets a real rule when it gets real content, and forcing a
  shared abstraction before then would be guessing at it.
- Y-axis bounds ignoring legend visibility in `TimeseriesPlot`. Pre-existing
  and unchanged by ticket 04 — hiding a tall trace does not rescale Y, since
  bounds are folded over every series and `auto_bounds` is off. Separate call,
  since the fix changes what zoom does.

## Comments

Recorded 2026-08-08 from the review of ticket 04.
