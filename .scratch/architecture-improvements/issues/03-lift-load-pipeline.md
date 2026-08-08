Status: done
Type: task

# Lift the load pipeline out of the UI

File dialog, thread spawning, parsing, all spectral analysis and the channel
protocol sat in one 56-line method inside `app/`, which `lib.rs` does not
export — so none of it was reachable from a test, and `tests/` held fixtures
with no `.rs` file.

## Files

- `src/app/mod.rs` — `open_logs`, `poll_load`, `show_loading_modal`
- `src/app/log_store.rs` — `LoadEvent`, `LoadState`, `LoadedLog`
- `src/loader.rs` — new
- `tests/loader.rs` — new

## Problem

- `open_logs` did path picking, `LogFile::open` + error partitioning, thread
  spawning, `parse_logs`, `GyroNoiseAnalyzer::default().analyze` over every
  sublog, and channel wiring — all in one method on `BlackbirdApp`
- `GyroNoiseAnalyzer` is configurable per call site (that's why it holds its
  thresholds), but the only caller hard-coded `::default()` with no way to
  reach the knobs
- Progress was per *file* — a `.bbl` with eight flights showed one step and
  then sat still for ten seconds
- One corrupt sublog failed the whole file: `parse_logs` is a
  `collect::<Result<_,_>>`
- No cancellation anywhere; a wrong 17 MB file had to be waited out

## Solution

`LogLoader` in the library half, emitting `LoadEvent`s through a `LoadSink`.
The UI keeps path picking and rendering.

- `LoadSink` — one interface, two adapters: `Sender<LoadEvent>` for the
  threaded UI path, `Vec<LoadEvent>` for tests driving the load inline
- `LogLoader::load_path` / `load_file` — synchronous, sink-agnostic;
  `spawn(paths) -> LoadHandle` is the threaded adapter over them
- `LoadHandle { rx, cancel, expected }` — the stream plus its stop switch
- `CancelToken` — `Arc<AtomicBool>`, checked between sublogs
- `LogLoader { analyzer }` — the analysis knobs get a caller

## Seams under test (agreed)

1. `LogLoader` event sequence via the `Vec` sink — progress, ready, failure,
   cancellation
2. Analysis config reaching the analyzer, proving the knob is wired

## Comments

Done 2026-08-08. `tests/loader.rs` is the first `.rs` file in `tests/` — 5
tests plus one `#[ignore]`d 8-sublog `.bbl` case (12.9 s), matching the
existing convention for heavy fixture tests in `parser`. Suite at 60 passing.

Behaviour changes that fell out of the seam, all in the UI's favour:

- per-sublog progress (`file.parse_log(i)` in a loop instead of
  `parse_logs()`), so the modal reads `name — log 3 / 8`
- a corrupt sublog is reported and skipped; the file still yields a `Ready`
  with whatever parsed. Dropped the old "All logs in file were corrupt"
  catch-all, since each failure now names itself
- Cancel button in the loading modal

`poll_load` collects failures into a `Vec` and notifies after the drain —
`notify` takes `&mut self`, which the borrow on `load_state` rules out
mid-loop. `LogStore::is_empty` went with the catch-all message that was its
only caller.

`app::LoadedLog` still exists as the view-side wrapper (it owns
`active_sublog`, which is view state, not load output) and gets its data via
`From<loader::LoadedLog>`.

### Follow-up: the bar didn't move

First cut counted whole files, so the common case — one file, one flight —
showed `0 / 1` for the entire load and then vanished. Fixed by reporting real
decode progress:

- `LogFile::parse_log_with_progress(index, on_progress)` — the `blackbox-log`
  `DataParser` already tracks `stats().progress` (0..=1 by bytes consumed);
  `build_flight_data` forwards it every `PROGRESS_INTERVAL_FRAMES` (4096).
  `parse_log` is now a thin wrapper passing a no-op
- `LoadEvent::Progress` carries `fraction` within the current sublog; the UI
  keys a `HashMap<file_name, f32>` and `LoadState::fraction()` averages over
  `handle.expected`, so files not yet started count as 0 and the denominator
  never jumps
- `on_progress` returns `bool`. Returning `false` abandons the decode with
  `ParseError::Cancelled`, so the Cancel button lands mid-sublog instead of
  after one — otherwise a single 17 MB flight had to be waited out
- `LoadEvent::Cancelled` gained `file_name`, so a cancelled file settles at
  1.0 rather than stalling the bar

Bar swapped to `elegance::ProgressBar`, matching the rest of the UI's widgets.
