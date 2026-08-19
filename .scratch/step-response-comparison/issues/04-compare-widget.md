# 04 — The compare widget: chips and picker

Status: done

Spec: `.scratch/step-response-comparison/spec.md`
Depends on: 01, 02

## What

The pilot needs to say which flights to compare, and to read which colour is
which flight. One widget does both: a row of coloured chips, one per compared
flight, and a "+ compare" button opening the candidate list.

It lives in `app/ui/compare.rs`, next to `log_card.rs`, and takes the compare set
plus the catalog — not `StepResponse`. PSD and Frequency across two logs is the
same comparison with the same picker (same x axis in Hz, no time alignment), and
a picker welded inside the step response panel gets either duplicated or
refactored under pressure when that lands.

## Scope

- Chips: one per compared flight, in slot order, in the slot colour. The chips
  are the legend — no plot legend anywhere.
- Chip label is mechanical: file name and sublog number, truncated from the left
  so the discriminating tail of a name like `blackbox_012.bbl` survives. Hover
  carries craft, firmware, duration, rates, looptime.
- Candidate list grouped by file, listing every `(LogId, sublog)` the catalog
  offers. The base flight is not a candidate.
- Cap of four including the base. At the cap, unchecked candidates are
  **disabled** with a tooltip naming the rule — not refused on click, and never
  silently evicted. A greyed checkbox states the rule before the click; a
  notification states it after, and dropping the flight someone is comparing
  against is the one thing that must not happen.
- Warning glyph on a chip whose `rates` differ from the base, hover explaining
  that the shared minimum stick input then selects different manoeuvres in each
  flight. A differing `looptime_us` gets a hover line, no glyph.
- Removing a chip is one click on the chip itself.
- The base flight is always the sidepanel selection and always slot 0. Selecting
  a compared flight in the sidepanel drops it from the set — the widget reconciles
  this, since it is the only place that knows both.

## Tests

The set logic is testable without a `Ui`; extract it from the drawing.

- Adding past the cap is impossible: the fifth entry cannot be added, and the
  first four are untouched.
- A base that is already in the compare set is removed from it, leaving no
  duplicate.
- An entry whose `LogId` no longer resolves is dropped, and the flights after it
  keep their slots rather than shuffling colour.
- Ordering is insertion order, so a colour does not move when an unrelated entry
  is removed.

## Done when

Up to three flights can be added beside the base, each with a coloured chip that
names it, and the set survives sidepanel switches, file removals and the cap
without ever mislabelling a colour.
