# Spec: compare several flights in the step response panel

Status: done

## Problem

A pilot changes one thing — a P gain, a D gain, a filter cutoff — flies again,
and wants to know whether the craft answers the sticks better than it did. Today
that comparison happens by memory: select one log, read the overshoot, select
another, read it again. The two curves are never on screen together, and the two
numbers are read seconds apart from a panel that redrew in between.

The store already holds everything needed to draw them together. `LoadedLog`
owns a `Vec<ParsedLog>` and a `Vec<Analysis>`, one entry per sublog, both
computed at load time — so an overlay costs no parse and no reanalysis. What is
missing is a way to say which flights, and a way to tell them apart on screen.

## Change

Comparison is a Step Response concept, not an app-wide selection mode.

The sidepanel keeps single selection: `LogStore::selected` and
`LoadedLog::active_sublog` are untouched, and every other tab is untouched with
them. The Step Response panel grows a compare row — coloured chips for the
flights being compared, and a picker to add more. Its curves are then coloured
by flight rather than by axis, and its metrics become a grid.

This follows a law the panel already states: the analysis knobs live in
`StepResponse` panel state *"so they survive a log or sublog switch and two
flights can be compared under identical analysis"*. Which flights are being
compared belongs in the same place, for the same reason.

### The model

- `LoadedLog` gains a monotonic `LogId`, assigned on `push`.
- The compare set is an ordered `Vec<(LogId, usize)>` in `StepResponse` panel
  state, capped at four entries including the base.
- Identity is by id, never by index. A compare set of `(log_index, sublog)`
  pairs would keep drawing after `LogStore::remove` shifted the indices under
  it, silently relabelled — the exact failure the store's single-selection
  invariant was written to prevent (see 2006f0f).
- The base flight is always the sidepanel selection, and always colour slot 0.
  Selecting a compared flight in the sidepanel removes it from the compare set;
  a flight is not its own comparison.
- Panels reach flights other than their own through a narrow read-only catalog
  trait on `TabCtx` — enumerate, label, resolve. Not `&LogStore`: a panel handed
  the store could `select` or `remove` mid-frame while the sidepanel is
  iterating it.

### On screen

- Colour is the slot, not the flight. Switching the sidepanel changes which
  flight sits in slot 0 while its colour stays; the chip label carries identity.
- Hue sequence is fixed so "the teal one" survives a theme switch; lightness and
  saturation come from the current palette. `get_axis_color`'s hardcoded
  `Palette::charcoal()` is fixed in the same pass — it is the same defect, and
  it makes the axis colours wrong in light mode today.
- Axis colouring is untouched in every single-log tab. Betaflight red/green/blue
  stays where a pilot expects it, and flight colouring exists only where
  comparison lives. No tab ever changes its own colour law.
- Chips are the legend, which is what makes the metrics grid readable and means
  no plot needs one.
- Individual traces default to off, at every count. `StepMetrics.spread_pct`
  already reports the inter-quartile range of the per-trace peaks and
  `metrics_line` already prints it, so the band was never the only witness to
  spread — and for most pilots the mean is the only line that matters. The
  checkbox stays, disabled above one compared flight: overlaid bands are mud.
- Metrics stay prose at one flight and become a colour-swatched grid at two or
  more. The prose form reads well as a sentence; three of them are a table
  pretending not to be.

### Comparability

A compared flight on a different rate curve is a real confound and an invisible
one: the shared `min_setpoint_dps` knob then selects different manoeuvres in each
flight, and the curves still render as if they were comparable. Differing
`rates` gets a warning glyph on the chip and a hover saying why; differing
`looptime_us` gets a hover line. Neither blocks the comparison — comparing
across a rates change is a legitimate thing to want to see.

## Out of scope

- App-wide multi-select, and comparison in any other tab. The compare widget is
  built to be reusable (PSD and Frequency across two logs is the obvious next
  one — same x axis in Hz, no time alignment problem) but no other tab adopts it
  here.
- Typed PID fields on `Metadata`. PIDs sit unparsed in `raw_headers`, and a chip
  showing `P+4 D−2` against the base is the label a pilot actually wants — but
  parsing them is a `parser/metadata.rs` change that Milestone 3's AI context
  needs anyway. It lands on its own, and the delta then goes in the chip
  *hover*, not the label: a label that changes width as the base changes makes
  the toolbar jump.
- Moving the off-defaults recompute onto a worker thread. Four flights
  reanalysed on knob release is up to four times a cost that already exists at
  one, only off the defaults, only on release. The cache is shaped so a worker
  drops in later.
- Time-aligning flights, or comparing raw timeseries. Step response is already
  time-independent — milliseconds since the step — which is exactly why it is
  the tab this belongs in first.

## Issues

1. `01-log-id-and-catalog.md` — stable ids, read-only catalog on `TabCtx`
2. `02-colour-slots.md` — hue ramp, theme-derived, `get_axis_color` fixed
3. `03-traces-off-by-default.md` — default off, `max_traces` 200 → 40
4. `04-compare-widget.md` — chips, picker, cap, labels, warning glyph
5. `05-step-response-draws-n-flights.md` — overlaid means, metrics grid
6. `06-recompute-cache-per-flight.md` — `Cached` becomes a map keyed by `LogId`
