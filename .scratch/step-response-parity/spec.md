# Step response: parity with PIDToolbox, and knobs to tweak it

Status: done

Follow-up to `.scratch/step-response/` (done). The curve that shipped there does
not match what other tools draw for the same flight, and the analyser's
thresholds — already fields on `StepResponseAnalyzer` — have no way to reach a
pilot.

## Problem Statement

A pilot loads a log, opens PID Analysis → Step Response, and gets a curve that
disagrees with the tool they already trust. It is wrong in a specific,
recognisable way: it leaps off the origin instead of rising, and it barely
overshoots.

Measured on `tests/fixtures/eight_logs_in_one.bbl`, roll, log 7 (287 s, 1013 Hz,
peak setpoint 669 deg/s, 707 windows):

| pipeline | t = 0 | 10 ms | 50 ms | peak |
|---|---|---|---|---|
| as shipped | **0.241** | **0.831** | 1.013 | 1.055 @ 73 ms |
| with a Hann window on the segment | 0.024 | 0.304 | 1.159 | 1.189 @ 65 ms |

A quad cannot be 83% of the way to its commanded rate 10 ms after the stick
moves — its rise time is 30–50 ms. The step response is being read as far
faster and far better damped than the craft actually is, which is the exact
diagnosis a pilot uses to decide whether to add D or take P away. On a short
log it is not merely biased but unusable: log 2 (13 s, one surviving window)
draws a curve that peaks at **3.63** and swings to **−2.99** 15 ms later.

Second problem: every threshold is a compile-time default. A cinematic log that
never breaks the stick mask, a log where the pilot wants a tighter window, a
suspicion that regularisation is flattening the peak — none of these are
answerable without a rebuild.

## Solution

Bring the analysis in line with the reference implementations, and expose its
parameters in the panel so a pilot can see what each one does to the curve.

The curve rises from zero over ~50 ms and overshoots by a believable amount,
matching PIDToolbox on the same flight. Above the plots sits a row of knobs —
window length, hop, minimum stick input, regularisation, steady-state
acceptance band — that recompute the stack live, in the panel, without
reloading the log. Defaults match the reference tools so the out-of-the-box
curve needs no tuning to be trusted.

## Root cause

Each window is sliced out of continuous flight and handed to the FFT as a
rectangle. Both edges are step discontinuities the craft never flew; they leak
across the whole spectrum and land in the recovered impulse response as a
spurious spike at `t = 0`. Cumulative-summed, that spike becomes the step
response's head start. PIDToolbox and PID-Analyzer both multiply the segment by
a Hann window before the transform; Blackbird does not.

Secondary contributors, measured on the same fixture:

- **Steady-state gate is sign-blind and far too loose.** `min_steady_state =
  1e-3` accepts a trace whose steady state is 0.002 and multiplies it by 500,
  and accepts a *negative* steady state — dividing by which flips the trace
  upside down into the mean. Log 7 has 39 traces outside a plausible band and 4
  negative ones.
- **Stick mask at 20 deg/s** admits windows the reference rejects. Log 3 (peak
  43 deg/s) currently draws a full curve out of what is essentially noise, where
  the other tool correctly shows nothing.
- **2 s window, 0.25 s hop** against the reference's 1 s window and
  window/16 hop: a quarter of the traces, each blending twice as much flight —
  throttle and attitude changes included — into one estimate.
- **λ** was swept (1e-4 … 1e-1) on the fixture and `0.01` sits in a sensible
  place; 1e-3 buys ~0.03 more overshoot at the cost of 16 more junk traces.
  It stays as-is, but becomes a knob.

## User Stories

1. As a pilot, I want the step response curve to start at zero and rise over
   tens of milliseconds, so that it reflects how my craft actually responds
   rather than a windowing artefact.
2. As a pilot, I want the overshoot shown on the curve to match what PIDToolbox
   shows for the same flight, so that the tuning advice I already understand
   still applies.
3. As a pilot, I want the peak of the curve to land at the right time, so that
   I can tell a slow-responding craft from a fast one.
4. As a pilot comparing Blackbird against the tool I already use, I want the two
   curves to be recognisably the same shape, so that I can trust Blackbird
   enough to stop opening the other one.
5. As a pilot with a short log, I want a curve that stays within believable
   bounds, so that one 13-second flight does not draw a wildly oscillating trace
   that means nothing.
6. As a pilot, I want traces whose deconvolution did not settle anywhere
   plausible to be dropped, so that they do not drag the mean around.
7. As a pilot, I want a trace that came out inverted to never appear the right
   way up, so that the stack shows only responses the craft could have made.
8. As a pilot flying gently, I want the analyser to say the sticks never moved
   enough rather than draw a curve out of noise, so that I am not misled into
   changing a tune based on nothing.
9. As a pilot, I want the number of averaged responses shown, so that I can
   judge how much flight the curve is built from.
10. As a pilot, I want to shorten or lengthen the analysis window, so that I can
    see whether the curve is stable across timescales.
11. As a pilot, I want to change the hop between windows, so that I can trade
    trace density against recompute time on a long log.
12. As a pilot flying cinematic, I want to lower the minimum stick input, so
    that a gentle flight still yields a step response.
13. As a pilot chasing a suspicious peak, I want to raise the minimum stick
    input, so that only the hardest inputs contribute.
14. As a pilot, I want to change the regularisation, so that I can see whether
    the overshoot I am looking at is real or an artefact of smoothing.
15. As a pilot, I want to widen or narrow the steady-state acceptance band, so
    that I can see how many traces are being discarded and why.
16. As a pilot, I want to change the response length, so that I can look at a
    long settle or zoom into the rise.
17. As a pilot, I want the curve to redraw as soon as I change a knob, so that
    the effect of each parameter is obvious.
18. As a pilot, I want a reset control, so that I can get back to the defaults
    after experimenting without remembering what they were.
19. As a pilot, I want each knob to show its unit and default, so that I know
    what I am changing and by how much.
20. As a pilot, I want the knobs to state their meaning in FPV terms — stick
    input in deg/s, window in seconds — so that I never have to think about
    FFT lengths.
21. As a pilot who has changed the knobs into a state that yields nothing, I
    want the panel to say which threshold rejected everything, so that I can
    walk it back.
22. As a pilot, I want switching to another log or sublog to keep my knob
    settings, so that I can compare two flights under identical analysis.
23. As a pilot, I want recomputing not to freeze the app on a five-minute log,
    so that dragging a slider stays usable.
24. As a pilot, I want the individual-traces toggle to keep working with the
    knobs, so that I can watch the spread tighten or widen as I change them.
25. As a developer, I want the analyser's parameters to remain plain fields with
    documented defaults, so that a test or a future AI context can set them
    without going through the UI.
26. As a developer, I want the load-time analysis to keep using the defaults, so
    that panel experimentation never leaks into what the AI layer will later be
    handed.
27. As a developer, I want the windowing decision recorded, so that nobody
    removes the Hann window later on the theory that λ handles truncation.

## Implementation Decisions

**Analyser (`analysis/step_response.rs`)**

- Each window's setpoint and gyro are multiplied by a Hann window before being
  handed to the deconvolver. The window is precomputed once per log, alongside
  the FFT plan, on the existing internal `Plan` — not per window, not per axis.
- `min_steady_state: f64` is replaced by a `RangeInclusive<f64>` acceptance band
  for the trace's steady state, default `0.5..=3.0`. A steady state outside the
  band — including any negative value — rejects the trace. This subsumes the old
  divide-by-zero guard.
- `NoStepResponse::NoSteadyState` keeps its meaning: sticks moved, but no trace
  landed inside the band. Its explanatory text should mention the band.
- Defaults change to match the reference tools: `window_s = 1.0`,
  `hop_s = window_s / 16.0` (62.5 ms), `min_setpoint_dps = 52.0`. `lambda_k`
  stays `0.01`, `response_ms` stays `500.0`, `tail_ms` stays `100.0`.
- `hop_s` remains an absolute duration rather than a fraction of the window, so
  the two knobs stay independent and the field keeps its unit.
- `StepResponseAnalyzer` derives `PartialEq` so the panel can tell whether its
  knobs still sit at the defaults.
- Averaging stays a plain pointwise mean. The reference's weighted-mode average
  changed the curve by ~0.005 on this fixture; not worth the machinery yet.
- Regularisation stays a single flat λ. The reference's frequency-dependent
  noise model is a separate change with its own evidence bar.

**Panel (`app/tabs/pid_analysis/step_response.rs`)**

- The panel owns a `StepResponseAnalyzer` (its knobs) and a cached
  `StepResponseAnalysis`.
- While the knobs equal `StepResponseAnalyzer::default()`, the panel draws
  `ctx.analysis.step` — the load-time result — and computes nothing.
- Once any knob differs, the panel recomputes from `ctx.flight` and caches the
  result. Recompute is synchronous: the full fixture stack (707 windows over
  287 s at 1 kHz) runs in tens of milliseconds.
- Cache identity is the log's time axis `Arc` compared by pointer, plus the
  analyzer value. `FlightData` already holds `time_us: Arc<[u64]>`, so the panel
  clones the `Arc` and uses `Arc::ptr_eq` — no index, no generation counter, and
  no way for a reallocated `LogStore` to silently alias two logs.
- Knobs are drag values with units, one row above the plots, collapsed into a
  collapsing header so the default view is unchanged: window (s), hop (s),
  minimum stick input (deg/s), λ, steady-state band (low, high), response length
  (ms).
- A reset control restores `StepResponseAnalyzer::default()`, which also drops
  the cache and returns the panel to the load-time result.
- Knobs are panel state and survive a log or sublog switch, following the
  existing precedent that the individual-traces toggle is panel state rather
  than shared state.
- Knobs are not persisted across app restarts.

**Pipeline**

- `LogLoader`, `Analysis`, `LogStore` and `TabCtx` are unchanged. Load-time
  analysis keeps using `LogLoader.step_response`, which keeps defaulting to
  `StepResponseAnalyzer::default()`.

**Docs**

- The decision table in `.scratch/step-response/spec.md` and the signal
  processing notes in `CLAUDE.md` both record the old window, hop and mask
  values, and neither mentions a window function. Both are updated.
- The test comment in `signal/deconv.rs` claiming that λ absorbs the window's
  edge truncation states the wrong mechanism and is corrected — that is what the
  Hann window is for.

## Testing Decisions

A good test here states a claim about the curve that a pilot would recognise,
and would still hold if the internals were rewritten: what the response does at
`t = 0`, whether a trace that could not have come from a craft is in the stack,
which explanation an empty axis gives. Tests that assert an FFT length, a plan
field or a call order are testing the implementation and should not be written.

**`analysis/step_response.rs` unit tests** — prior art: the existing module
tests, which drive a known second-order system through the analyser and assert
the analytic overshoot comes back out.

- The existing `recovers_a_known_second_order_overshoot` must keep passing under
  the new defaults, which is itself the parity check.
- A step response starts near zero: the first sample of the mean is a small
  fraction of the steady state. This is the test that fails today and is the
  reason for the change.
- A synthetic system whose true rise time is known is not reported as arriving
  materially sooner — the leakage artefact, stated as behaviour.
- A trace whose deconvolution settles outside the acceptance band does not
  contribute to the mean. Drivable by pushing the band to a range the synthetic
  system cannot land in and asserting the axis reports `NoSteadyState`.
- An inverted response — gyro of opposite sign to setpoint — never appears the
  right way up in the stack.
- `SticksTooStill` names the threshold that rejected the windows, at the new
  default of 52 deg/s.
- Every threshold remains reachable: setting a knob to a loosened value turns a
  rejected log into an analysed one, extending the existing
  `the_stick_mask_is_tunable`.

**`tests/loader.rs` integration tests** — prior art:
`analysis_knobs_reach_the_analyzer` and
`dropping_the_stick_mask_recovers_traces_from_the_hover`, which drive the whole
load path over a real fixture with a knob changed.

- `a_hover_moves_the_sticks_too_little_for_a_step_response` asserts the old
  20 deg/s threshold and must be updated to 52.
- The hover fixture, with the mask dropped, still yields finite traces on a real
  flight under the new defaults.
- A trace stack computed over the real `.bbl` fixture starts near zero — the
  regression guard for the actual bug, on actual flight data rather than a
  synthetic.

**`signal/deconv.rs`** — unchanged. The primitive is correct; the caller was
feeding it an unwindowed segment.

**Panel** — not directly tested; there is no UI test harness in this repo, and
the recompute path it exercises is the analyser, which is tested above.

## Out of Scope

- Overshoot, rise time and settling metrics as numbers. Still deferred until the
  curve is trusted — this spec is what makes it trustworthy.
- Weighted-mode averaging in place of the mean.
- A frequency-dependent noise model for λ.
- Throttle gating or throttle-binned step responses.
- Persisting knob settings across restarts, and any global settings surface.
- Re-running the load-time analysis when knobs change, or feeding tweaked
  parameters into the future AI context.
- Reconstructing setpoint from `rcCommand` for logs that do not log it.
- Background/async recompute. Revisit only if a real log makes a knob drag stutter.
- The filter delay analyser, which has its own cross-correlation path.

## Further Notes

All measurements above come from a throwaway harness run over
`tests/fixtures/eight_logs_in_one.bbl` at `--release`: eight sublogs parsed and
five analyser configurations compared per sublog in 1.14 s total, which is where
the "recompute is cheap enough to be synchronous" decision comes from.

Log 2 of that fixture is the cheapest reproduction of the bug: 13 s, exactly one
surviving window, and the current pipeline draws +3.63/−2.99 where the windowed
one draws 1.44.

The reference parameters (1 s frame, window/16 hop, 52 deg/s minimum, Hann
window) come from the user's reading of the tool being compared against, and are
consistent with PID-Analyzer's `framelen = 1.0`, `superpos = 16` and
`np.hanning(flen)`.
