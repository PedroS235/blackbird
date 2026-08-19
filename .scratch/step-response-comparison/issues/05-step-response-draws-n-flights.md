# 05 — The panel draws several flights

Status: done

Spec: `.scratch/step-response-comparison/spec.md`
Depends on: 01, 02, 03, 04

## What

`show_axis` draws one flight: the axis colour for the mean, a faded band, a peak
diamond, and one `metrics_line` above the plot in a `ui.horizontal`. It becomes N
flights, up to four.

Colour switches from axis to slot inside this panel, at every count — including
one. A curve must not change colour the moment a second flight is added, and axis
identity is already carried by the label above each stacked plot. Every other tab
keeps Betaflight red/green/blue untouched.

## Scope

- One mean line per compared flight per axis, in its slot colour, plus its peak
  diamond. Plots stay stacked one per axis; `stacked_plot_height` still divides by
  the number of axes drawn, not by flights.
- An axis is drawn if *any* compared flight can fill it. A flight whose axis
  errored contributes no curve and instead names its reason — `explain()` already
  turns each `NoStepResponse` into something a pilot can act on, and it should
  say which flight it is talking about now that more than one is on screen.
  Union, not intersection: one setpoint-less sublog must not blank the axis for
  the whole comparison.
- Metrics: the existing prose `metrics_line` at one flight; at two or more, a
  grid per axis — a colour swatch, then overshoot, peak, delay, spread and count
  as columns. Aligning the numbers vertically is the entire reason to compare
  them. The `THIN_STACK` caveat becomes a warning-coloured count cell rather than
  a trailing clause.
- The traces checkbox is disabled above one compared flight, with the honest
  reason in its tooltip: overlaid bands are mud.
- Differing sample rates across flights need no special handling — each flight
  brings its own `time_ms`, and the x axis is milliseconds for all of them.
- The toolbar's `rates` label describes the base flight. Leave it as the base's,
  since the chips now carry the others' rates on hover and the glyph from 04 says
  when they differ.

## Tests

Layout is not unit-testable; the logic around it is.

- Which axes draw, given a set of flights whose `StepResponseAnalysis` errors on
  different axes: the union, with one entry per flight explaining itself.
- The metrics table's rows are in slot order and carry the right flight's
  numbers — built from a pure function over the resolved flights, so it can be
  asserted without a `Ui`.
- At one compared flight the panel's output is the prose form, unchanged from
  before this issue.

## Done when

Two flights flown on the same craft, one before and one after a D change, show
two mean curves and two rows of numbers per axis, and it is obvious from the
colours which is which.
