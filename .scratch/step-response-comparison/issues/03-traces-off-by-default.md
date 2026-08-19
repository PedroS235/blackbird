# 03 — Individual traces off by default, and a smaller retained sample

Status: done

Spec: `.scratch/step-response-comparison/spec.md`

Independent of the comparison work and landable on its own: it is a default and a
memory figure, both wrong for the pilot the panel is for.

## What

`StepResponse::default()` sets `show_individual: true`, justified in a comment
saying the spread is the information and hiding it "would leave a mean curve with
nothing to judge it against". That premise no longer holds: `StepMetrics`
reports `spread_pct`, the inter-quartile range of the per-trace peaks, and
`metrics_line` already prints it alongside the surviving count. The spread has a
number. For most pilots the mean is the only line that matters, and the band is
what they see first.

The band is also not free. `max_traces` is 200 and `response_ms` is 500, so at
8 kHz each retained trace is 4000 `f64` — about 6.4 MB per axis, ~19 MB per
sublog, computed and held at load time for **every** sublog in the file. A
twelve-sublog `.bbl` parks roughly 230 MB behind a checkbox that is now off.

Forty traces still read as a band. The existing test asserting that the mean is
identical under a tiny `max_traces` and an enormous one is what makes lowering it
safe by construction.

## Scope

- `show_individual` defaults to `false`. Rewrite the comment: it should point at
  `spread_pct` and the count as what carries the spread now, so the next
  contributor does not "restore" the old default from a stale rationale.
- `max_traces` default 200 → 40. Still a field, still no knob.
- The checkbox stays. Revealing the band is how a pilot distinguishes a clean
  mean of agreement from a mean of two different flight regimes, which two
  quartile numbers cannot show the *shape* of.
- Tooltip on the checkbox says what the band is and that the numbers beside it
  are the same claim.

## Tests

The existing analyser tests already cover the cap. Add:

- The mean and every field of `StepMetrics` are unchanged between
  `max_traces: 40` and the old 200 on the same synthetic log. Guards the claim
  that this is a retention change and not a measurement change.

## Done when

A freshly loaded log draws mean curves only, the retained sample is a fifth of
what it was, and ticking the checkbox still draws a readable band.
