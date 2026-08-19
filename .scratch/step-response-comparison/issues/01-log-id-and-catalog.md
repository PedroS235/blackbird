# 01 — Stable log ids and a read-only catalog

Status: done

Spec: `.scratch/step-response-comparison/spec.md`

First, because everything else needs a way to name a flight that survives a
removal, and a way to reach one that is not the selected flight.

## What

A flight is identified today by its position: `LogStore.selected` is an index
into `logs`, and `remove` repairs it on shift. That works while exactly one
flight is named at a time. A compare set names up to four, from panel state the
store cannot see, so index repair stops being possible — and index *reuse* is
worse than a dangling reference: after removing file 0, a stale `(0, 2)` pair
resolves to a different file and keeps drawing under the old chip.

Panels also see one flight each. `TabCtx` carries `flight`, `analysis`,
`metadata`, resolved once in `Tabs::show` so no panel re-implements the empty
case. A compare picker has to enumerate every loaded flight and resolve the
chosen ones.

## Scope

- `LogId(u64)` newtype, assigned from a monotonic counter in `LogStore::push`
  and stored on `LoadedLog`. Never reused, so a removed flight's id resolves to
  `None` for good.
- A read-only catalog trait — enumerate `(LogId, sublog_index)`, resolve one to
  `(&FlightData, &Analysis, &Metadata)`, and give a display label per entry.
  `LogStore` implements it.
- `TabCtx` gains the catalog as a borrow. Its doc-comment already claims to be
  "the one place a new kind of shared data is added", so this is that place.
  Panels that do not care ignore the field.
- The trait, not `&LogStore`: the store's mutators (`select`, `remove`) must stay
  out of a panel's reach — the sidepanel iterates the store mutably in the same
  frame.
- `LogStore::selected` stays an index. Single selection is already correct and
  already tested; this issue adds identity, it does not migrate the existing
  invariant.

## Tests

Store unit tests, alongside the existing ones.

- Ids are unique across pushes and removals: push three, remove the first, push
  another — the new log's id equals none of the earlier three.
- A removed log's id no longer resolves, and the ids of the logs that shifted
  down still resolve to the same flights they did before. The load-bearing one:
  it is what an index-keyed compare set gets wrong.
- Resolving `(id, sublog)` past the end of that log's sublogs yields `None`
  rather than another log's sublog.

## Done when

A flight can be named, held across an unrelated removal, and resolved back to
the same data — with every existing store test still green and no behaviour
change on screen.
