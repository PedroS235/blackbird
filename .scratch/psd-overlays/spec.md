# Spec: overlay menu for the PSD panel

Status: ready-for-agent

## Problem Statement

A pilot opens the PSD sub-tab to answer one question: is my noise being
filtered, and are my filters where the noise actually is. The panel today makes
that question harder to answer than reading the raw curve alone.

Every overlay is drawn always, in one colour, with its label permanently on.
Per axis that is up to eight peak lines, each rendered *twice* — once as a
labelled vertical line and again as floating text at the peak — plus one line
per gyro filter stage. Three axes stacked, and the curve is behind a picket
fence. Nothing can be turned off.

Worse, what is drawn is not what the filters do:

- A dynamic notch appears as a single line at the midpoint of its configured
  range. Its `count` and `Q` are parsed and thrown away, so the pilot sees a
  guess at a centre the notch never sits at, and no indication of how much
  bandwidth it removes.
- A dynamic lowpass collapses to its `dyn_max_hz` ceiling — one line standing
  in for a cutoff that moved all flight.
- The RPM filter, the single most important noise filter on a modern quad, is
  not drawn at all. `RpmFilterConfig` is parsed and never reaches the plot.
- `eRPM` is in the log and is never decoded, so the panel cannot show where the
  motors actually were — only where the config said filters would be.

So the pilot is offered a Filter settings tab in Betaflight full of numbers, and
a PSD that cannot tell them whether any of those numbers were the right ones.

## Solution

Two changes, one enabling the other.

**An overlays menu.** A single dropdown above the plots, with one section per
overlay family: Harmonics, Dyn notch, Static notches, LPFs. Every family is off
by default, so the panel opens as a clean spectrum and the pilot adds exactly
the reference they want. The lines that are always-on today become entries in
this menu. The menu is a dropdown rather than an inline panel because vertical
space above the plots is divided between three stacked axes.

**Overlays that show what the filter does, not just where it nominally is.**
Every filter is drawn as the shape it actually occupies in frequency, over the
window that was analysed:

- Motor harmonics, decoded from `eRPM`, as a band per motor per harmonic
  spanning the minimum to maximum frequency that motor reached. Four motors and
  three harmonics is the common case, so twelve bands, coloured by harmonic
  index. Bands overlap where the motors agree and fan out where one motor is
  working harder, which is itself the diagnosis.
- The dynamic notch as its configured range, plus the **traced** centre the
  firmware actually chose, read from the debug channel when the log was flown in
  `FFT_FREQ` debug mode. Configured range against measured behaviour, on one
  plot.
- Notches and lowpasses as bands of their real bandwidth, derived from `Q`.
- A peak sitting outside the dynamic notch's reachable range is recoloured to
  the warning colour and counted in a prose line under the plot — noise the
  tracker can never touch, stated as such.

The pilot can then read the Filter settings tab in Betaflight and see, on the
PSD, whether each setting is aimed at real noise.

## User Stories

1. As an FPV pilot, I want the PSD to open with no overlay lines, so that I can
   see the shape of my noise before anything is drawn on top of it.
2. As an FPV pilot, I want a single menu listing every overlay, so that I do not
   have to hunt across the panel for the control that hides a line.
3. As an FPV pilot, I want each overlay family to toggle independently, so that I
   can compare my noise against exactly one reference at a time.
4. As an FPV pilot, I want the overlays menu to take no vertical space when
   closed, so that three stacked axis plots stay tall enough to read.
5. As an FPV pilot, I want toggling an overlay on the PSD to leave the
   Spectrogram unchanged, so that I can set each sub-tab up the way that
   sub-tab needs.
6. As an FPV pilot, I want overlay toggles to be instant, so that ticking a box
   never re-runs analysis or stalls the frame.
7. As an FPV pilot, I want to toggle motor harmonics on, so that I can see which
   peaks in my spectrum are motor noise and which are something else.
8. As an FPV pilot, I want each harmonic order drawn in its own colour, so that
   I can tell a fundamental from its second and third harmonic at a glance.
9. As an FPV pilot, I want each motor's harmonic drawn as a band spanning the
   frequencies it actually reached, so that I am not shown a single line for a
   frequency that moved all flight.
10. As an FPV pilot, I want all four motors drawn separately within a harmonic
    order, so that one motor spinning faster than the others shows up as a band
    that does not overlap the rest.
11. As an FPV pilot, I want to see three harmonic bands when I have configured
    three RPM filter harmonics, and one when I have configured one, so that the
    plot matches my Betaflight settings.
12. As an FPV pilot, I want a harmonic whose RPM filter weight is zero drawn as
    an outline with no fill, so that I can tell "the filter is tracking this" from
    "the filter is attenuating this".
13. As an FPV pilot, I want the harmonics overlay to be disabled with a stated
    reason when my log has no `eRPM`, so that I know the log is the limitation and
    not the tool.
14. As an FPV pilot flying without bidirectional DShot, I want the rest of the
    overlays to still work, so that one missing data source does not cost me the
    whole menu.
15. As an FPV pilot, I want harmonic frequencies computed from my motor pole
    count, so that the bands land on the right frequencies for my motors.
16. As an FPV pilot, I want to toggle the dynamic notch overlay on, so that I can
    see the frequency range my dynamic notch is allowed to work in.
17. As an FPV pilot, I want the dynamic notch's configured minimum and maximum
    drawn as a shaded range, so that I can see at a glance how much of my
    spectrum it can reach.
18. As an FPV pilot, I want to see where the dynamic notch tracker actually put
    its centre frequency during the flight, so that I can tell whether the range
    I configured is the range it needed.
19. As an FPV pilot, I want the traced centre drawn as a density over frequency
    rather than a single number, so that a tracker pinned at one end of its range
    for half the flight is obvious.
20. As an FPV pilot, I want the traced centre labelled as traced, so that I never
    mistake a measurement for a setting.
21. As an FPV pilot with more than one dynamic notch configured, I want the panel
    to show the one centre the firmware logs without pretending to show the
    others, so that I am not misled about what was measured.
22. As an FPV pilot whose log was not flown in `FFT_FREQ` debug mode, I want the
    configured dynamic notch range still drawn, so that the overlay degrades
    rather than disappears.
23. As an FPV pilot, I want a noise peak that sits outside my dynamic notch range
    recoloured as a warning, so that I can see immediately that my notch can never
    reach it.
24. As an FPV pilot, I want a written count of out-of-range peaks under the plot,
    so that I still get the verdict after I have zoomed the peak off screen.
25. As an FPV pilot, I want notches drawn as bands of their real bandwidth
    derived from `Q`, so that I can judge whether a notch is wide enough to cover
    a peak.
26. As an FPV pilot, I want a dynamic lowpass drawn as the range its cutoff moved
    through, so that I am not shown a single ceiling for a filter that swept.
27. As an FPV pilot, I want static gyro notches drawn only when they are actually
    enabled, so that a disabled notch does not appear as a filter at zero hertz.
28. As an FPV pilot, I want to toggle gyro and D-term filter families separately,
    so that I can look at the filters feeding one loop without the other.
29. As an FPV pilot, I want at most the three strongest peaks labelled, so that
    the labels I do see are the ones that matter.
30. As an FPV pilot, I want each peak drawn once, so that half the marks on the
    plot are not duplicates of the other half.
31. As an FPV pilot, I want a labelled peak to tell me how much the filters
    attenuated it, so that I can judge whether the filter chain worked without
    reading two plots.
32. As an FPV pilot in light mode, I want every overlay colour to be readable
    against the background, so that the panel is usable in the theme I chose.
33. As an FPV pilot, I want a harmonic order to keep its colour when I switch
    theme, so that "the orange band" means the same thing in both.
34. As an FPV pilot, I want overlay colours to be distinct from the axis colours,
    so that a reference band is never mistaken for a signal trace.
35. As a contributor, I want overlay geometry computed once at load and stored
    with the rest of the analysis, so that the panel is a renderer and the
    geometry can be tested against real log fixtures.
36. As a contributor, I want one overlay type describing lines, bands, harmonic
    groups and traced distributions, so that panels stop distinguishing overlay
    kinds by matching on label text.
37. As a contributor, I want motor RPM available as a channel like every other
    signal, so that a later RPM-binned heatmap needs no new parsing.
38. As a contributor, I want the whole feature exercised through the existing
    loader integration seam, so that no new test harness is introduced.

## Implementation Decisions

### Parser: decode eRPM

- `eRPM` fields are detected alongside the existing motor, gyro and debug fields.
  `FlightData`'s `rpm` field — currently declared, hardcoded empty and never
  read — becomes real.
- A `Channel::Rpm(index)` variant makes RPM reachable through the same channel
  accessor every other signal uses. Without it the data is unreachable by
  panels, which is the state the existing dead field is already in.
- `motor_poles` is read from `Metadata`'s raw header passthrough rather than
  promoted to a typed field. It is the first consumer of that passthrough.
- Frequency conversion is `hz = erpm * 100 / (poles / 2) / 60`.
- Missing `motor_poles` falls back to 14, Betaflight's default and the value in
  both repository fixtures. A wrong pole count is wrong by an obvious integer
  factor, which a pilot spots; a disabled overlay teaches nothing.
- Missing `eRPM` entirely is different in kind, and disables the harmonics
  toggle with a stated reason. This follows the tab bar's existing law that a
  control which cannot be filled greys out rather than vanishing.

### Analysis: overlay geometry replaces filter markers

- `FilterMarker` — a line-shaped record of label, centre and optional cutoff — is
  replaced by an overlay type carrying the shapes the filters actually have:
  a line, a band with a low and high bound, a harmonic group, and a traced
  distribution. It has exactly one consumer today, so the replacement is
  contained.
- The replacement removes the string-prefix filter the PSD panel currently uses
  to select gyro-only markers. Family membership becomes part of the type
  instead of a label convention.
- A dynamic lowpass emits a band across its dynamic minimum and maximum instead
  of collapsing to the maximum.
- A notch emits a band of width `centre / Q` rather than a bare centre.
- The dynamic notch emits its configured range as a band, not a midpoint line.
- Harmonic groups are computed per motor per harmonic, each carrying the minimum
  and maximum frequency that motor reached over the analysed window, and a flag
  for whether that harmonic's RPM filter weight is non-zero.
- The number of harmonic orders drawn comes from `RpmFilterConfig`'s harmonic
  count, not from a constant.
- The traced dynamic notch centre comes from the per-axis debug channel, gated
  on the log's debug mode being `FFT_FREQ`, reduced to a histogram over
  frequency. The Spectrogram sub-tab already reads this channel for its own
  overlay; the gating rule is shared, not reinvented.
- Out-of-range peaks are determined in analysis, not in the panel: each detected
  peak carries whether it falls outside the configured dynamic notch range, plus
  a count for the prose summary.
- **All of this is computed at load time and stored on `Analysis`.** It is not a
  pure function the panel calls per frame. Two reasons: the geometry depends on
  the analysed window, which is fixed at load and does not change with a
  visibility toggle; and storing it puts the whole feature behind the existing
  loader integration seam instead of requiring a new one.

### Colours

- Overlay colours move out of per-panel hardcoded constants into the one colour
  module, alongside the axis and compare-slot palettes. The hardcoded constants
  today do not follow the theme, which is the exact defect that module exists to
  prevent.
- Harmonic orders get a fixed hue sequence, in the same spirit as the compare
  slot hues: hue is fixed so a harmonic order keeps its identity across a theme
  switch, while saturation and value come from the active palette.
- Every new colour is subject to the module's existing contrast and distinctness
  tests. Expect the current peak and filter marker colours to shift slightly in
  one theme to pass them.

### UI: the overlays menu

- A single dropdown button above the plots, with a section per overlay family and
  a switch per family plus that family's sub-knobs. This is the idiom the compare
  picker already uses, and it costs no vertical space when closed.
- Overlay visibility state is a **shared struct type with a separate instance per
  sub-tab**. Shared type so the menu and the geometry consumers are written once;
  separate instances because the PSD and Frequency sub-tabs once shared a
  visibility field and toggling one silently toggled the other.
- Every family defaults to off. There is no settings persistence in this
  application, so state is per session by construction.
- Peak rendering loses the duplicated floating text; the vertical line's own
  label carries the peak. Labels are capped to the three strongest peaks by
  amplitude, with the remainder drawn as unlabelled lines.
- A labelled peak carries its post-filter attenuation, a value the analysis
  already computes and the panel currently never displays.
- Out-of-range peaks are recoloured to the palette warning colour, and a single
  prose line under the plot states the count and the range bound they exceeded.
  Prose under a plot is the idiom the step response panel already uses to state
  what its data cannot support.

## Testing Decisions

A good test here asserts what a pilot could observe, from the outside: given
this log, does the analysis report harmonic bands at these frequencies, is this
peak marked out of range, is the traced centre present. It does not assert how
the geometry was computed, which helper was called, or in what order overlays
were pushed. Rendering is not asserted at all — no test should claim to know
what a plot looks like.

**One seam, already in the repository.** The loader integration test drives a
real fixture file through parse and analysis and asserts on the loaded result.
Both repository fixtures carry `rpm_filter_harmonics:3`, `motor_poles:14` and
`dshot_bidir:1`, and one of them was flown in `FFT_FREQ` debug mode, so the
whole feature — eRPM decode, pole conversion, harmonic extents, notch bands,
traced centre, out-of-range detection — is reachable end to end from there. Prior
art in that file: the test asserting a fixture's headers decode to the rates it
was flown on, and the test asserting analysis runs at load time. New tests follow
their shape.

This is the reason overlay geometry is stored on `Analysis` rather than computed
in the panel. A pure function called per frame would need a second seam and would
never be exercised against a real log.

Supporting unit tests, all in modules that already have them:

- Parser: eRPM field detection, and the eRPM-to-hertz conversion including the
  missing-poles fallback.
- Analysis: the notch bandwidth derivation from `Q`, the dynamic lowpass band,
  the zero-weight harmonic flag, and out-of-range peak classification against a
  constructed filter config.
- Colours: the new harmonic hues join the existing contrast-against-background
  and mutual-distinctness tests, in both palettes.
- PSD panel: plot id stability and overlays-default-off, following the heatmap
  panel's existing precedent of asserting plot ids so a rename cannot silently
  discard a pilot's persisted zoom.

The fixture without `FFT_FREQ` debug mode is the negative case for the traced
centre: the configured range must still be produced.

## Out of Scope

- **Suggesting filter values.** The panel says where the noise is and whether the
  filters reach it. It does not propose a dynamic notch minimum, maximum or
  count. That is tuning advice and belongs with the AI panel, where it can be
  argued for in prose rather than asserted as a silent number on a plot.
- **Overlays on the other frequency sub-tabs.** Spectrogram, Frequency and Vs
  Reference are all frequency-axis plots that could take the same geometry. The
  geometry is stored where they can reach it and the state struct is a shared
  type, so extending them later is small. This spec draws only on the PSD.
- **Any analysis knob in the overlays menu.** No trim, window or peak-detection
  control. The menu is visibility only, and there is no recompute path from this
  panel.
- **Settings persistence.** Overlay toggles do not survive a restart. The
  application has no storage layer, and adding one is its own change.
- **Plotting motor output or RPM as a timeseries.** `eRPM` is decoded and made
  reachable as a channel, but no timeseries panel draws it here.
- **An RPM-binned spectrogram.** The binning plumbing accepts an RPM reference
  and, after this change, the reference exists. Using it is a separate feature.
- **`dyn_notch_width_percent`.** Betaflight dropped it after 4.2 and it is in
  neither fixture. Old logs would have to read it from the raw header
  passthrough.
- **Promoting PIDs to typed fields.** They sit unparsed in the raw header
  passthrough. Unrelated to this feature, and noted below only because the
  project documentation claims otherwise.

## Further Notes

**Delivery in two commits.** Parser and analysis first — eRPM decode, the
overlay type replacing filter markers, the geometry on `Analysis` — landing
tested against both fixtures before anything is drawn. Then the UI: menu,
colours, per-family rendering, peak label cull. The first commit is verifiable
without the second, and the second is nearly all rendering.

**The project documentation is out of date and this feature widens the gap.**
`CLAUDE.md` describes a `HeaderData` struct with typed PID arrays, a
`SpectralResult`, and a `NotchFilter` type. None of these exist: the real types
are `Metadata`, `AxisSpectral` and `NotchConfig`, and PIDs are not parsed at all.
Correcting that section is part of this work.

**Two things the analysis already computes and nothing displays**:
per-peak attenuation and the per-axis noise floor. The first is used by this
spec, in the peak label. The second is still unused, and is the obvious material
for a future summary line.

**The traced dynamic notch centre is the highest-value part of this feature and
the cheapest.** The data is already decoded, already has a semantic accessor, and
is already consumed by one sub-tab. A tracker pinned at its configured maximum
for half a flight is the single most legible filter fault this panel could
surface, and it costs one histogram.

## Issues

Not yet decomposed. Expected shape, in dependency order:

1. Decode `eRPM`, add the RPM channel, pole conversion with fallback.
2. Replace `FilterMarker` with the overlay type; notch and lowpass bands.
3. Harmonic group geometry, weight-gated, stored on `Analysis`.
4. Traced dynamic notch centre histogram, debug-mode gated.
5. Out-of-range peak classification and count.
6. Overlay colours into the colour module, harmonic hues under the contrast tests.
7. The overlays menu and per-family rendering.
8. Peak label cull, attenuation in the label, out-of-range recolour and prose line.
9. Correct the stale type names in `CLAUDE.md`.
