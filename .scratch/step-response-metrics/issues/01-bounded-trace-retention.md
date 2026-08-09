# 01 — Bound what the trace stack retains

Status: todo

Spec: `.scratch/step-response-metrics/spec.md`

First of three, because it is a regression already on `main`: tightening the hop
to window/16 in the parity work quadrupled a stack that was never bounded.

## What

`AxisStepResponse` keeps every surviving window's trace in full while the panel
draws at most a hundred of them and nothing else reads them. Measured: ~57 MB
retained per five-minute 1 kHz sublog, ~459 MB at 8 kHz, times up to eight
sublogs in a `.bbl`, plus a second copy in the panel's recompute cache.

## Scope

- Rename `traces` to `sample` and add `count` — the true number of surviving
  windows. `mean` keeps being accumulated over every window, never over the
  sample. The rename is the point: `traces.len()` is the number the panel prints
  today, and after this change that reading is wrong.
- Build the sample with a halving-stride reservoir that decimates in place as it
  fills, so peak allocation is bounded and not just what survives. Deterministic:
  the same log yields the same sample every run.
- `max_traces: usize` on `StepResponseAnalyzer`, default 200, field only — no
  knob.
- Remove the response-length knob from the panel. `response_ms` stays a public
  field at 500 ms: it sets where the steady-state tail sits, so a control
  labelled "response length" was re-normalising every trace while implying it
  only changed the view. Zoom covers looking at the rise.
- Panel reads `count` for its "mean of N responses" label and says nothing about
  the sample.

## Tests

Analyser unit tests, driven over synthetic flight data as the existing module
tests are.

- The same log analysed with a tiny `max_traces` and an enormous one produces
  identical means. The load-bearing one — it is what would rot silently if
  someone later accumulated the mean from the retained sample.
- The sample never exceeds the cap; `count` reports surviving windows, not the
  sample's length.

## Done when

Loading the eight-sublog fixture retains a bounded stack per sublog, the drawn
band is visually unchanged, and the mean is bit-identical to what the uncapped
analysis produced.
