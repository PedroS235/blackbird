# 06 — One recompute cache entry per compared flight

Status: done

Spec: `.scratch/step-response-comparison/spec.md`
Depends on: 01, 05

## What

`Cached` holds exactly one `{ time, analyzer, analysis }`, keyed by
`Arc::ptr_eq` on the flight's time handle so a reallocating store cannot alias
two flights. At the defaults the panel returns the load-time analysis and
computes nothing; off the defaults it recomputes on knob release, holding the
last result while a `DragValue` is being dragged.

With up to four flights and one knob off default, all four need reanalysis under
the *same* analyzer. One cache slot would thrash — each flight evicting the last
every frame.

## Scope

- `Cached` becomes a small map keyed by `LogId` and sublog index, bounded by the
  cap of four. The `analyzer` is stored once, not per entry: the knobs are shared
  by design, and a per-entry copy could disagree.
- Key by id rather than by the time handle now that ids exist — the `ptr_eq`
  trick was standing in for identity the store did not have.
- Entries for flights no longer in the compare set are dropped, so removing a
  chip frees its analysis.
- The defaults path is unchanged and still free: at `analyzer == default` every
  flight draws its own load-time `Analysis` and the map stays empty.
- Drag deferral is unchanged in shape: while a knob is dragged, keep drawing what
  is cached; on release, recompute every compared flight once.
- Off the defaults this costs up to four times a wait that already exists at one
  — accepted, and only in an expert mode. Shape it so the recompute is one call
  over the set, which is where a worker thread would later go.

## Tests

- A compare set of several flights under a non-default analyzer produces one
  cache entry per flight, each holding that flight's own analysis.
- Changing a knob invalidates every entry, not just the base's. The load-bearing
  one: a stale entry would draw two flights analysed under different parameters,
  which is the mistake the shared knobs exist to prevent.
- Removing a flight from the compare set drops its entry.
- At the defaults the map stays empty and the panel returns each flight's
  load-time analysis.

## Done when

Four flights can be compared under a moved λ or a moved window with the panel
recomputing once per release, and no two curves on screen were ever produced by
different parameters.
