# Step response: numbers, not just a curve

Status: done

Follow-up to `.scratch/step-response-parity/` (done), which made the curve
trustworthy. This spec makes it *readable* — and affordable to keep around.

## Problem Statement

A pilot opens PID Analysis → Step Response and sees a curve that now matches
the tool they already trust. They still cannot say what it shows. Whether the
overshoot is 12% or 22%, whether the peak lands at 45 ms or 70 ms, whether this
tune responds faster than the one they flew last weekend — all of it has to be
eyeballed off a plot, and two flights cannot be compared without opening them
side by side and squinting.

The AI layer has the same problem in a worse form: a prompt cannot read a
curve. Every diagnostic relationship the tuning expertise is built on —
overshoot above some threshold means P/D imbalance — needs a number, and there
is none to hand it.

Second problem, introduced by the parity work: the analysis keeps every
surviving window's trace in full, while the panel draws at most a hundred of
them and nothing else ever reads them. Tightening the hop to window/16
quadrupled that stack. A five-minute log at 1 kHz now retains ~57 MB per
sublog, ~459 MB at 8 kHz, and an SD-card `.bbl` holds up to eight sublogs, each
with its own analysis held for the life of the session — with the panel's
recompute cache holding a second copy of whichever one is on screen.

Third problem: the stick-input presets ask a pilot to choose between 25, 70 and
120 deg/s, but nothing on screen says what those numbers mean on *their* quad.
A pilot on 670 deg/s rates and one on 1200 deg/s are not describing the same
manoeuvre when they pick "Freestyle". The log carries the answer — every
Betaflight log header records the craft's rate type, centre sensitivity and
maximum rate — and none of it is parsed.

## Solution

The step response reports itself. Beside each axis' curve sits the line a pilot
would say out loud: *18% overshoot (spread 12–24%), peak 54 ms, delay 21 ms* —
with a marker on the curve at the peak, so the number and the picture are
visibly the same claim. A thin stack says so rather than quoting a confident
figure off four windows.

Underneath, the analysis keeps only as much of the trace stack as it can draw.
The mean still comes from every window of the flight; what is retained is a
bounded, evenly spread sample of it, so the band on screen is unchanged and
loading eight flights no longer costs a gigabyte.

Beside the presets, and on the log card, the craft's own rates: *Actual
67/67/67*. The pilot choosing a preset can see what their quad was set to while
they choose.

## User Stories

1. As a pilot, I want the overshoot shown as a number, so that I can tell a
   12% tune from a 22% one without measuring a plot by eye.
2. As a pilot, I want the overshoot in percent rather than as a normalised
   value, so that it is the same figure I already discuss with other pilots.
3. As a pilot, I want the time of the peak, so that I can tell a slow craft
   from a fast one.
4. As a pilot, I want the delay before the craft reaches half its commanded
   rate, so that I can judge how much of my tune is latency rather than gain.
5. As a pilot, I want the numbers measured from the same curve that is drawn,
   so that the figure and the picture can never disagree.
6. As a pilot, I want a marker on the curve at the peak, so that I can see at a
   glance which feature of the plot the overshoot number refers to.
7. As a pilot, I want the spread of overshoot across the individual responses,
   so that I can tell a curve averaged from a tight band from one averaged over
   traces that disagree wildly.
8. As a pilot, I want that spread shown as a range rather than a plus-or-minus,
   so that I am not told a lopsided distribution is symmetric.
9. As a pilot with a short flight, I want to be warned when the numbers come
   from very few responses, so that I do not retune on the strength of four
   windows.
10. As a pilot with a short flight, I want the numbers shown anyway, so that I
    can tell "not much data" apart from "nothing was computed".
11. As a pilot, I want the numbers as whole percentages and milliseconds, so
    that I am not reading precision the measurement does not support.
12. As a pilot switching stick-input presets, I want the numbers to move with
    the curve, so that I can watch how my craft behaves differently under hard
    inputs than under gentle ones.
13. As a pilot comparing two flights, I want to read three numbers off each,
    so that I can compare tunes without opening both plots side by side.
14. As a pilot, I want no settling-time figure quoted, rather than one that
    changes every time I move a knob, so that I am not given a number I cannot
    rely on.
15. As a pilot loading a multi-flight SD-card log, I want the app not to
    consume hundreds of megabytes per flight, so that opening a full card does
    not exhaust my machine.
16. As a pilot, I want the band of individual responses to look exactly as it
    did before, so that bounding memory costs me nothing I could see.
17. As a pilot, I want the averaged curve to still come from every part of the
    flight, so that capping what is retained never changes the answer.
18. As a pilot, I want the count of averaged responses to stay truthful, so
    that it tells me how much flight the curve is built from rather than how
    much of it was kept in memory.
19. As a pilot, I want the retained sample spread evenly across the flight, so
    that the band reflects the whole log and not just its opening seconds.
20. As a pilot, I want the same log to draw the same band every time I open it,
    so that a diagnostic tool does not show me something different each run.
21. As a pilot choosing a stick-input preset, I want to see what rates my quad
    was on, so that I know whether 120 deg/s is a flick or a full flip for me.
22. As a pilot, I want my craft's rates on the log card, so that I can check
    what a flight was flown on without hunting through a config dump.
23. As a pilot, I want the rate type named, so that I can tell Actual rates
    from Betaflight rates when reading the numbers.
24. As a pilot whose rate type this build does not recognise, I want to be told
    that plainly rather than shown a wrong conversion.
25. As a pilot, I want the response-length control gone rather than
    silently re-normalising my curve, so that no control on screen changes the
    data while claiming to change the view.
26. As a pilot, I want to zoom into the rise of the curve with the plot itself,
    so that looking closer is never confused with measuring differently.
27. As a developer, I want the metrics in one struct rather than scattered
    fields, so that the prompt builder can hand the whole thing to the model.
28. As a developer, I want the analysis types to stay free of serialisation
    derives, so that dumping raw traces into an AI prompt is not the path of
    least resistance.
29. As a developer, I want the retention cap to be a plain field with a
    documented default, so that a test can prove capping does not change the
    mean.
30. As a developer, I want the retention cap kept out of the UI, so that the
    knob row stays things that change the answer rather than things that change
    memory use.
31. As a developer, I want the rate headers parsed into a typed value, so that
    header string parsing never leaks into a panel.
32. As a developer, I want the rate type modelled the way filter types already
    are, so that there is one shape for "a Betaflight enum we decode".

## Implementation Decisions

**Step response analyser (`analysis::step_response`)**

- `AxisStepResponse.traces` becomes `sample` — a bounded, evenly spread subset —
  alongside `count`, the true number of surviving windows. The rename is the
  point: `traces.len()` is today the number the panel prints, and after this
  change that reading is wrong. `mean` continues to be accumulated over every
  surviving window, not over the sample.
- The sample is built by a halving-stride reservoir: traces are appended until
  the buffer is full, then the buffer is decimated in place by dropping every
  other entry and the stride doubles. This bounds *peak* allocation, not merely
  what is retained — collecting everything and decimating afterwards would leave
  the reported spike untouched.
- Retention is deterministic. The same log yields the same sample every run;
  random eviction is rejected because a diagnostic tool that draws a different
  band each time it opens the same flight is not one a pilot can reason about.
- `max_traces: usize`, default 200, is a field on `StepResponseAnalyzer` and is
  deliberately **not** exposed as a knob. The knob row is for parameters that
  change the answer; this one changes only how much of an already-saturated band
  survives.
- `StepMetrics` is a struct held on `AxisStepResponse`, computed eagerly during
  analysis. Laziness would need interior mutability or a `&mut` accessor for a
  few scans over 500 samples.
- Metrics are measured on the mean curve, so the figure always describes the
  curve that is drawn. It carries: overshoot as a percentage over the steady
  state, the time of the peak, the delay to the 50% crossing, and the
  interquartile range of the per-trace peak as a spread.
- No settling time. A 500 ms measurement window averaged from as few as forty
  traces cannot support a stable one, and a number that moves whenever the stick
  mask moves reads as precision that is not there.
- `response_ms` stays a public field at its 500 ms default and loses its panel
  knob. It sets both how much response is measured and where the steady-state
  tail sits, so a user-facing control labelled "response length" was changing
  every trace's normalisation while implying it changed only the view.

**Step response panel (`app::tabs::pid_analysis::step_response`)**

- Each axis' heading gains the metrics line: overshoot, its spread, peak time
  and delay — whole percentages and whole milliseconds throughout.
- A single marker is drawn on the curve at the peak. Rise and delay stay
  numeric; three annotations across three stacked plots is noise.
- Metrics are never suppressed. Below ten responses the line carries a caveat
  naming the count. The threshold is a constant — it is a statement about
  statistics, not about flying, so it is not a knob.
- The response-length knob is removed. Looking at the rise is plot zoom, which
  `egui_plot` already provides along with double-click to reset.
- Beside the stick-input presets sits a short echo of the craft's rates.

**Parser metadata (`parser::metadata`)**

- `RateConfig` holds the rate type, the per-axis RC rates, the per-axis rates
  and the per-axis expo, built from the raw headers by a `parse_rate_config`
  step alongside the existing filter-config parsing.
- `RateType` is an enum decoded from the Betaflight code, mirroring the existing
  filter-type decode, and carries an `Unknown(code)` variant that renders as the
  raw code rather than guessing at a conversion.
- No per-type centre-sensitivity or maximum-rate maths in this spec. Converting
  RC rates into deg/s needs a different formula for each of the five Betaflight
  rate types, and none of them are verified here — the typed struct is the place
  that work will land.
- The log card gains a rates line beside the craft name, firmware and loop rate
  it already shows. There is no Log Info panel in this codebase yet; the log
  card is the craft-configuration surface that exists, and the full row belongs
  wherever that panel eventually lands.

**Delivery**

Three issues, in order: retention first because it is a regression already in
`main`; metrics second because the AI milestone is waiting on them; rates third
because it is independent of both and is the prerequisite for deriving presets
from the craft's own rates later.

## Testing Decisions

A good test here states a claim about the reported response that a pilot would
recognise, and would survive the internals being rewritten: what number the
curve reports, whether capping what is retained changed the answer, what a log's
headers say the craft was set to. A test that asserts a reservoir's stride
schedule, a struct's field order or the number of scans over the mean is testing
the implementation and should not be written.

**Seams.** No new ones. Everything here is reachable from the two that already
exist, which is the whole test surface this feature needs:

- The analyser itself, driven directly over synthetic flight data — prior art is
  the existing module tests, which push a known second-order system through
  `analyze` and assert its analytic overshoot comes back out.
- The load pipeline over real fixtures — prior art is
  `analysis_knobs_reach_the_analyzer` and
  `responses_from_a_real_flight_start_from_rest`, which drive open → parse →
  analyse through `LogLoader` with a knob changed and assert on the result.
- Metadata parsing rides the second seam, with the existing `bfl_metadata`
  parser test as prior art for asserting decoded header values.

**Analyser unit tests**

- The metrics off the known second-order system match its analytic overshoot and
  peak time — the same claim the existing overshoot test makes, now made against
  the reported number rather than a hand-scan of the curve.
- Capping does not change the answer: the same log analysed with a tiny
  `max_traces` and an enormous one produces identical means. This is the
  load-bearing one — it is the property that would rot silently if someone later
  accumulated the mean from the retained sample.
- The retained sample never exceeds the cap, and `count` reports the true number
  of surviving windows rather than the sample's length.
- A thin stack still reports metrics rather than nothing, so the panel's caveat
  has something to caveat.

**Integration tests**

- A real flight fixture yields metrics in plausible ranges — overshoot within
  believable bounds, peak time in tens of milliseconds — as the regression guard
  on actual data rather than a synthetic.
- A fixture's headers decode to their recorded rate type and values.

**Panel** — not directly tested; there is no UI test harness in this repo, and
everything it renders comes from the analyser, which is tested above.

## Out of Scope

- Settling time as a metric, and any metric measured per-trace and aggregated
  rather than read off the mean.
- Betaflight rate-type maths: converting RC rates into centre sensitivity and
  maximum rate in deg/s, and deriving the stick-input presets from them as a
  share of the craft's own maximum rate. Both wait on the typed struct this
  spec adds.
- A dedicated Log Info panel. The rates land on the existing log card.
- Splitting the response into low-input and high-input curves the way
  PID-Analyzer does. Revisit once these metrics make that comparison numeric
  rather than visual.
- Background or threaded recompute. Measured at 333 ms for a five-minute log at
  1 kHz, 709 ms at 2 kHz and 3.3 s at 8 kHz; holding the previous result while a
  knob is dragged covers the common case. Revisit when a real 4–8 kHz log makes
  a single click hang.
- Serialisation derives on the analysis types. The mapping into an AI prompt is
  hand-written when the AI milestone lands, precisely so that raw traces cannot
  reach a prompt by default.
- Weighted-mode averaging and a frequency-dependent noise model for λ. Both
  measured as sub-0.01 effects on the fixture.
- Persisting any of this across app restarts.

## Further Notes

Measurements quoted above come from the parity work and from probes over
`tests/fixtures/eight_logs_in_one.bbl` at `--release`.

The stick-input presets move the reported overshoot in a way these metrics will
make plain, which is the main argument for landing them: on the fixture's two
freestyle sublogs the mean peak falls from 1.20 at a 25 deg/s mask to 1.12 at
120 deg/s, with peak time steady at 51–63 ms. That is the craft meeting its rate
limits under big inputs, and until now it was only visible by flipping presets
and watching the curve move.

Trace counts across the presets on those two sublogs — 268/1768 at 25, 75/333 at
70, 40/98 at 120 — are what set the ten-response caveat threshold and confirm
that the hardest preset still averages a real stack on a real flight.

The rate headers are present and unparsed today: the fixture records
`rates_type:3` (Actual), `rc_rates:7,7,7`, `rates:67,67,67` and `rc_expo:0,0,0`.
Under Actual rates those decode to 70 deg/s centre sensitivity and 670 deg/s
maximum — which is where the Racing preset's 70 came from — but that decode is
exactly the per-type maths this spec defers.
