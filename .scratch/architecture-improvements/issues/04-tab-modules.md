Status: done
Type: task

# One module per tab, not 13 static `show_` functions

From the 2026-08-08 grilling session. Fourth ticket in this effort — see `01`,
`02`, `03` for the preceding three.

## Files

- `src/app/mainview.rs` — 662 lines, 13 `show_*` functions, 4 tab enums
- `src/app/view_state.rs` — `MainViewState` and its three per-tab structs
- `src/app/mod.rs` — the four selected-tab fields on `BlackbirdApp`
- `src/app/ui/timeseries_plot.rs` — `Series`, `TimeseriesPlot`
- `src/app/tabs/` — new

## Problem Statement

A tab is currently spread across three files. Its enum lives in `mainview.rs`,
the field recording which variant is selected lives on `BlackbirdApp` in
`mod.rs`, and its widget state lives in `view_state.rs`. Nothing holds a tab
together; adding one means editing three files, and reading one means opening
three.

The `show_*` functions that result take `ui` plus the data plus two or three
`&mut` slices of state — `show_gyro_plots(ui, fd, &mut filtered_visible, &mut
raw_visible)`. The interface is as complex as the body it guards, which is the
signature of a shallow module.

Two consequences follow from that shape:

- **Positional read-back.** `TimeseriesPlot` mutates `Series.visible` during
  `show`, so every caller reads its own state back out by index afterwards:
  `raw_visible[axis] = plot.series[0].visible`. One of those reads
  (`mainview.rs:223`) indexes `series[1]` without checking the series was
  pushed, and panics on a log with raw gyro but no filtered gyro. The identical
  shape 50 lines below guards with `.get(1)`. The bug exists because the
  interface invites it.

- **Duplicated toggles.** `TimeseriesPlot` already configures
  `Legend::default()`, and then draws its own checkbox row above the plot for
  the same series. Every timeseries plot in the app ships two independent
  controls for one piece of state.

Three parent tabs each repeat the same `current_flight()` unwrap and the same
`"No log selected"` label.

## Solution

One module per tab, at both levels of the hierarchy. Each module owns its own
enum, its own selected-tab field, and its own widget state, and exposes a
single `show`. The custom checkbox row is deleted in favour of the legend that
was already there.

```
src/app/tabs/
├── mod.rs                    MainTab, Tabs, TabCtx, router, AutoTune arm
├── timeseries/
│   ├── mod.rs                TimeseriesTab, sub-tab bar, availability rule, state
│   ├── gyro.rs
│   ├── power_battery.rs
│   └── rssi.rs
├── filter_analysis/
│   ├── mod.rs                FilterAnalysisTab, sub-tab bar, state
│   ├── psd.rs
│   ├── frequency.rs
│   ├── vs_reference.rs
│   └── spectrogram.rs
└── pid_analysis/
    ├── mod.rs                PidAnalysisTab, sub-tab bar, state, StepResponse arm
    └── gyro_vs_setpoint.rs
```

`mainview.rs` and `view_state.rs` both delete. `BlackbirdApp` drops from four
tab fields to one `tabs: Tabs`.

## User Stories

1. As a pilot, I want the visibility of a trace to be controlled from exactly
   one place, so that I do not have to work out why unchecking a box above the
   plot left the line drawn.
2. As a pilot, I want a log with raw gyro but no filtered gyro to open, so that
   a log recorded in a debug mode that omits the filtered trace does not crash
   the app.
3. As a pilot, I want the Power & Battery tab to be selectable only when the
   log actually contains battery data, so that I do not land on an empty panel.
4. As a pilot, I want the Receiver RSSI tab to be selectable only when the log
   contains RSSI data, for the same reason.
5. As a pilot, I want a tab I am sitting on to fall back to Gyro when I switch
   to a log that lacks that data, so that the view never shows a blank panel
   for a tab that cannot render.
6. As a pilot, I want the PSD tab to keep its explicit `show filtered`
   checkbox, so that the filtered trace stays discoverable on a panel whose
   legend would otherwise list every detected noise peak.
7. As a pilot, I want peak markers on the PSD and Frequency panels to keep
   their labels, so that I can still read which frequency a marker sits at.
8. As a pilot, I want `"No log selected"` to appear consistently on every tab
   before a log is loaded, so that the app behaves the same wherever I am.
9. As a pilot, I want my trace visibility choices to survive switching between
   loaded logs, so that comparing two logs does not mean re-toggling traces.
10. As a pilot, I want the PSD and Frequency panels to remember their filtered
    toggles independently of each other, so that hiding the filtered trace on
    one does not hide it on the other.
11. As a pilot, I want every panel to look and behave exactly as it does today
    apart from the removed checkbox row, so that the refactor costs me no
    relearning.
12. As a developer, I want each tab's enum, selected variant, and widget state
    in one module, so that reading or changing a tab means opening one file.
13. As a developer, I want a tab's `show` to take only the data that tab reads,
    so that a signature tells me what the tab depends on.
14. As a developer, I want the shared analysis data handed down through one
    context type, so that adding a new kind of data to tabs is a single change
    rather than an edit to every signature.
15. As a developer, I want the `current_flight()` unwrap to happen once, so
    that no tab re-implements the empty case.
16. As a developer, I want the tab availability rule to be a pure function over
    state, so that it can be tested without a renderer.
17. As a developer, I want `BlackbirdApp` to hold one tabs field rather than
    four, so that the app struct describes the app rather than the view.
18. As a developer, I want adding a new sub-tab to touch one directory, so that
    the cost of a new panel is proportional to the panel.
19. As a developer, I want the behaviour change and the file moves in separate
    commits, so that `git bisect` and review can tell motion from change.
20. As a developer, I want `TimeseriesPlot::show` to take `&self`, so that the
    plot cannot be a channel for state to travel back to its caller.

## Implementation Decisions

**Granularity.** Both levels of the hierarchy become modules. A module per leaf
alone leaves the sub-tab enum and the availability rule without a home; a
module per top-level tab alone leaves `filter_analysis` as a ~250-line file,
which trades one large file for a smaller large file.

**No `Tab` trait.** Each tab module is a struct with an inherent `show`, by
convention rather than contract. A trait buying uniform `label()` / `enabled()`
only pays off if the tab list is built dynamically, which it is not — the tabs
are hardcoded. A trait would also force every tab to accept the union of all
data any tab needs, reintroducing the wide interfaces this ticket removes.

**`TabCtx`.** A shared borrow context carrying flight data, spectral analysis,
and log metadata, passed down from the router. Deliberately built as the seam
for future additions. It carries raw data only — derived predicates stay with
whoever reads them. `has_dyn_notch_trace` (debug mode is `FFT_FREQ` and the log
has debug axes) is one line used by the spectrogram leaf alone, so it is
derived there. `has_power` / `has_rssi` are read by the timeseries sub-tab bar,
so they stay in that module.

**Empty-state handling.** The top-level router resolves `current_flight()`
once, renders `"No log selected"` on the empty case, and builds `TabCtx`.
Nothing below the router sees an `Option`. This removes three copies of the
same unwrap-and-label.

**Tab availability.** The rule that `PowerBattery` and `Rssi` are enabled only
when the log carries that data, and that a selected-but-now-unavailable tab
falls back to `Gyro`, moves into `timeseries/mod.rs` as a pure method over the
tab state taking what the log has and resolving the selection. It is currently
two loose `if` statements interleaved with widget construction. This is the
only conditional tab logic in the app — `filter_analysis` has no enable rules,
and `pid_analysis`'s `StepResponse` is unconditionally disabled.

**Visibility moves to the legend, for the timeseries family only.** Plots built
through `TimeseriesPlot` (gyro, power & battery, RSSI, gyro vs setpoint)
already configure a legend. Their checkbox row is deleted; the legend becomes
the single control. `Series.visible` is deleted with it, and
`TimeseriesPlot::show` becomes `&self` — `egui_plot` filters hidden items from
its own memory before the build closure runs, so the field has no reader left.
The five visibility fields those plots used leave app state entirely.

The legend stays in its default top-right position inside the plot rect. This
is unchanged from today, since those plots already draw it there.

**PSD and Frequency keep their checkbox.** Those two plots configure no legend,
and their checkbox is not a hide but a conditional build — the filtered line is
never added to the plot. Adopting the legend there would mean enabling one, and
those panels emit a named `VLine` per detected peak, a named `Text` per peak,
and named filter markers, all of which become legend entries. A noisy log would
render a legend of a dozen frequency labels. They are also the only two places
wanting a non-default initial visibility (filtered hidden), which is the only
thing that would require seeding legend state at all.

Seeding was investigated and rejected as unnecessary given the above.
Recorded because it is not obvious from the API: `Legend::hidden_items` feeds
only the legend's own checkbox rendering — item filtering reads plot memory
unconditionally, so a seeded item still draws for one frame, and re-applying
the seed every frame reverts the user's clicks. It is a seed-once API.
`PlotMemory::load` / `store` are public and are the honest way to do it.

**Placeholder tabs stay inline.** `MainTab::AutoTune` and
`PidAnalysisTab::StepResponse` are each a single `ui.label("… - coming soon")`
and remain match arms. A module whose body is one label is ceremony.
`analysis/step_response.rs` already exists, so that leaf gets its module when
it has content to put in one.

**`src/app/ui/` is unchanged.** `heatmap`, `timeseries_plot`, and `log_card`
stay where they are as shared widgets; `log_card` belongs to the sidepanel, not
to any tab.

**Deprecation.** `CentralPanel::show_inside` is deprecated in egui 0.35 in
favour of `show`, which now takes `&mut Ui`. The migration in `mainview.rs`
carries over into `tabs/mod.rs`. The same deprecated call at
`notification.rs:16` is a separate one-line commit, not part of this ticket.

**Commit sequence.** The behaviour change goes first and alone, before any file
moves, so that the one commit a reviewer must judge by eye is isolated and the
rest is verifiably pure motion.

1. This ticket
2. Delete `show_controls` and `Series.visible`; `show` takes `&self`; drop the
   read-backs, the `mainview.rs:223` panic, and five state fields
3. Scaffold `tabs/mod.rs` — `MainTab`, `Tabs`, `TabCtx`, router, single unwrap
4. `timeseries/`
5. `filter_analysis/`
6. `pid_analysis/`
7. Delete `mainview.rs` and `view_state.rs`; `BlackbirdApp` down to `tabs`

## Testing Decisions

A good test here asserts on behaviour a pilot could observe, not on the module
layout that produces it. Steps 3–7 are motion: the compiler and the existing
suite are the check, and a test that pins the new structure would have to be
rewritten the next time the structure moves. Tests are therefore scoped to the
one piece of logic in this ticket that can be wrong without failing to compile.

**Seam under test (agreed): the timeseries tab availability rule.** One seam,
one module. Once extracted as a pure method over the tab state it is tested
in-module with `#[cfg(test)] mod test`, no renderer and no fixture. Cases:

- both power and RSSI present — every tab enabled, selection untouched
- selection on Power & Battery, log without power — falls back to Gyro
- selection on Receiver RSSI, log without RSSI — falls back to Gyro
- selection on Gyro, log without either — untouched, Gyro always available
- selection on Power & Battery, log with power but no RSSI — untouched

**Prior art:** `src/app/log_store.rs` unit-tests `LogStore` selection the same
way, in the same directory. This matters because `lib.rs` does not export
`app`, so `tests/` cannot reach any of it — in-module is the only seam
available without changing what the library exposes, and changing that is not
justified by this ticket.

**Not adding a render harness.** `egui_kittest` or a snapshot test would be a
new dev-dependency and a second seam covering the parts that are already pure
motion.

## Out of Scope

- Adding a legend to the PSD or Frequency panels, and any seeding of legend
  state that would require.
- `MainTab::AutoTune` and `PidAnalysisTab::StepResponse` gaining real content.
- Any change to `src/app/ui/` widgets beyond deleting `Series.visible` and
  `show_controls`.
- The deprecated `show_inside` at `notification.rs:16` — separate commit.
- Exporting `app` from `lib.rs`, or any render-level test harness.
- Resetting view state when a new log is selected. Trace visibility persists
  across log switches today and continues to; the difference is that it now
  lives in `egui_plot`'s memory rather than in `MainViewState`.
- The sliders (`frequency_peak_min_hz`, `heatmap_floor_db`,
  `spectrogram_floor_db`). They stay as tab state, moved but unchanged.

## Further Notes

Visibility state for the timeseries family leaves app code and lives in
`egui_plot`'s per-plot memory, keyed by plot id and by item name within it.
Current plot ids are already distinct per tab and per axis, so the cross-tab
bleed that `view_state.rs` documents — PSD and Frequency once shared a field,
and toggling one silently toggled the other — does not return. The trade is
that this state is no longer resettable from app code without going through
`PlotMemory`. Nothing wants to reset it today; noted in case something does.

`Series.visible` currently means two things at once: "the pilot unchecked it"
and "this data exists in the log". Only the second has a caller left after this
ticket, and it is already expressed by not pushing the `Series` at all —
`if let Some(filtered) = fd.gyro(axis)`. Collapsing the two is what removes the
panic rather than relocating it.

`filter_analysis` remains the largest module after the split. Its four leaves
are what keep it from being a single long file.

## Comments

**2026-08-08 — implemented.** Five commits on `refactor/axis-enum`:

- `c5864e5` — the behaviour change, alone as step 2 required: checkbox row and
  `Series.visible` deleted, `show` takes `&self`, read-backs and the
  `mainview.rs:223` panic gone.
- `41d8803` — steps 3–7 as one motion commit rather than five. Splitting them
  would have meant transitional wiring in `mainview.rs` (passing the tab enum
  and view state into free functions) that existed only to be deleted in the
  next commit. Story 19's intent holds — the one commit to judge by eye is
  isolated, and everything after it is verifiably pure motion.
- `359c894`, `dbd1d29`, `31fefd1` — review fixes, below.

Seven visibility fields left app state, not the five the ticket predicted:
`PidAnalysisTabState`'s two also went through `TimeseriesPlot`, so that struct
emptied out entirely.

**Review outcome.** Ten findings; three did not survive fact-checking against
the source and were closed without change:

- Y-bounds ignoring visibility — pre-existing, the old code folded bounds over
  all series regardless of `visible` too. Recorded in ticket `05`.
- `frequency.rs` measuring plot height before its slider — pre-existing and
  faithfully preserved (`HEAD~3:mainview.rs:553`). Recorded in ticket `05`.
- Plot memory surviving a log switch — out of scope by this ticket's own
  decision, and story 9 wants visibility to persist.

Three were real and are fixed:

- **The one genuine regression** (`359c894`): the legend gates *drawing*, not
  *compute*. `egui_plot` retains items after the build closure returns
  (`plot.rs:956`), so hidden series still paid a full `windowed_downsample`
  every frame — the checkbox row had gated the work itself. The closure now
  reads `hidden_items` from `PlotMemory` and registers hidden series with no
  points. The item must stay in the list: the legend is built from it
  (`overlays/legend.rs:262`, filtering on name only), so dropping it would
  take the entry with it. Note this contradicts the ticket's claim that
  "egui_plot filters hidden items from its own memory before the build closure
  runs" — it does not.
- **Auto Tune behind the log check** (`dbd1d29`): hoisting the single unwrap
  put the placeholder behind it, so a fresh launch answered a click with "No
  log selected". It reads no flight data and now resolves first.
- **Two cleanups** (`31fefd1`): `Psd`'s hand-written `Default` was equivalent
  to the derive; `rssi.rs` re-inlined the height arithmetic the same commit
  introduced a helper for.

The lost sub-tab bars before a log loads are accepted, not a defect: the
router's single unwrap is what removes the three duplicated empty cases, and
rendering them pre-log would put the `Option` back below the router.

Two tickets spawned: `05-tab-bar-and-heatmap-panel-shape` here, and
`.scratch/logs-without-gyrounfilt/issues/01-blank-gyro-and-filter-tabs` for
the blank-panel bug, which spans parser, analysis, and UI and predates this
work.
