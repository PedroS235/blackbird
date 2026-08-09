# 02 — Report the step response as numbers

Status: done
Blocked by: 01

Spec: `.scratch/step-response-metrics/spec.md`

The headline deliverable, and what the AI milestone is waiting on — a prompt
cannot read a curve.

## What

The curve is trustworthy since the parity work but has to be eyeballed. A pilot
cannot say whether the overshoot is 12% or 22%, cannot compare two flights
without opening both plots, and the prompt builder has no figure to reason over.

## Scope

- `StepMetrics` as a struct on `AxisStepResponse`, computed eagerly during
  analysis: overshoot as a percentage over the steady state, time of the peak,
  delay to the 50% crossing, and the interquartile range of the per-trace peak
  as a spread.
- Measured on the mean curve, so the figure always describes the curve drawn.
- No settling time — a 500 ms window averaged from as few as forty traces cannot
  support one, and a figure that moves with the stick mask reads as precision
  that is not there.
- Panel: a metrics line on each axis' heading, e.g. `18% overshoot (spread
  12–24%), peak 54 ms, delay 21 ms`. Whole percentages and whole milliseconds.
- A single marker on the curve at the peak, so the number and the picture are
  visibly the same claim. Rise and delay stay numeric.
- Never suppressed. Below ten responses the line carries a caveat naming the
  count — a pilot must be able to tell "not much data" from "nothing computed".
  The threshold is a constant, not a knob.
- No serialisation derives on the analysis types. `StepMetrics` is what will
  cross into the prompt builder, hand-mapped, when the AI milestone lands.

## Tests

- Metrics off the known second-order system match its analytic overshoot and
  peak time — the same claim the existing overshoot test makes, now against the
  reported number rather than a hand-scan of the curve.
- A thin stack still reports metrics rather than nothing.
- Integration, over a real fixture: metrics land in plausible ranges — believable
  overshoot, peak time in tens of milliseconds.

## Done when

Every drawn axis carries its three numbers and a peak marker, a four-response
stack says so, and the analytic system's overshoot is asserted through
`StepMetrics`.
