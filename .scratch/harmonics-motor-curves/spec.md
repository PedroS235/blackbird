# Spec: harmonics as per-motor curves, PIDToolbox style

Status: done

## Problem Statement

A pilot turns on the Harmonics family to answer one question: is this peak my
motors, or is it the frame?

What the PSD draws today cannot answer it. Twelve vertical bands — four motors
at three orders — hue-coded by *order*, each spanning the full min..max of that
motor's eRPM across the analysed window. On any real freestyle log that range
runs from idle to full song, so the three order-bands overlap into a wash that
covers most of the spectrum. Every peak lands inside one, so every peak looks
motor-explained, and the overlay says nothing.

The colour tells the pilot the least useful of the two facts. Order is
derivable — the second harmonic is at twice the first, and the pilot can see
that. Which *motor* is loud is not derivable and is the actual diagnosis: one
motor running hotter than its three siblings is a bent shaft, a chipped prop,
or a dying bearing. Today all four motors of an order share one hue and are
indistinguishable.

And the Spectrogram — the one panel with a time axis, where a motor's frequency
over the flight is a curve the pilot could read directly — draws no harmonics at
all. The eRPM is in the log and in memory. PIDToolbox draws exactly this and it
is the view pilots already know.

## Solution

Two encodings of the same fact, one per panel, sharing one identity scheme:
**hue is the motor, line style is the harmonic order.** Solid is the
fundamental, dashed the second, dotted the third.

**On the PSD**, each motor's order becomes an unfilled span at the frequencies
that motor actually spent its time — the 5th to 95th percentile of its running
eRPM, not the full excursion. A band that no longer swallows the spectrum is a
band a peak can fall *outside* of, which is where "that is not motor noise, that
is resonance" becomes visible. Twelve outlines carry no in-plot labels; a legend
appears above the plots when the family is on, keying the four motor hues and
the three styles.

**On the Spectrogram**, each motor's order becomes a curve of frequency against
time over the heatmap — the trace pilots recognise from PIDToolbox. Motors mostly
overlap into one thick trace, and it splits exactly when one motor diverges from
its siblings. Which means the same overlay diagnoses a bad motor and a frame
resonance, in the two panels where each is legible.

The Spectrogram gains the overlay menu it never had, so the family can be turned
off there like anywhere else — and the dynamic notch trace, which draws
unconditionally today, joins that menu as a family.

## User Stories

1. As an FPV pilot, I want each motor drawn in its own colour, so that I can see which motor is making the noise rather than only which harmonic order it is.
2. As an FPV pilot, I want harmonic order shown as a line style, so that I can tell the fundamental from its harmonics without spending a colour on a fact I could already infer.
3. As an FPV pilot, I want the PSD harmonic bands to cover only where a motor spent its time, so that a peak falling outside them tells me the noise is not from a motor.
4. As an FPV pilot, I want a peak that sits outside every motor band to be visibly outside them, so that I can conclude the noise is frame or prop resonance and stop expecting the RPM filter to fix it.
5. As an FPV pilot, I want one motor's band drifting away from the other three to be obvious, so that I can suspect a bent shaft, a chipped prop or a failing bearing on that specific motor.
6. As an FPV pilot, I want the harmonic bands unfilled, so that the spectrum curve underneath stays readable with twelve of them drawn.
7. As an FPV pilot, I want a legend keying motor hue and order style, so that I can read the overlay without twelve labels drawn over the curve I came to see.
8. As an FPV pilot, I want that legend to appear only when the Harmonics family is on, so that a clean panel stays clean.
9. As an FPV pilot, I want harmonic curves on the Spectrogram, so that I can watch a motor's noise frequency track my throttle over the whole flight.
10. As an FPV pilot, I want the Spectrogram curves to use the same hue and style scheme as the PSD bands, so that I learn one key and read both panels.
11. As an FPV pilot, I want the four motors' curves to overlap when the quad is healthy, so that a split between them stands out as the anomaly it is.
12. As an FPV pilot, I want the harmonic curves clipped to the heatmap's own frequency range, so that a third harmonic above Nyquist does not distort the plot.
13. As an FPV pilot, I want an overlay menu on the Spectrogram, so that I can turn harmonics off there as I can on the PSD.
14. As an FPV pilot, I want the dynamic notch trace to be a family in that same menu, so that every overlay on the panel obeys one rule instead of two.
15. As an FPV pilot, I want a harmonic the RPM filter tracks but does not attenuate to be drawn dimmed, so that I can see the noise is there and know nothing is being taken off it.
16. As an FPV pilot flying without an RPM filter, I want only the fundamental drawn, so that the plot does not claim harmonics are being tracked when no filter exists.
17. As an FPV pilot, I want a motor that was never spinning to draw no band at all, so that no band runs down toward zero describing a prop that was not turning.
18. As an FPV pilot on a hex or an octo, I want motors beyond the fourth still drawn, so that the overlay works on my craft even if two motors end up sharing a hue.
19. As an FPV pilot, I want the number of orders drawn to follow my RPM filter's harmonic setting, so that the plot matches what Betaflight is actually doing.
20. As an FPV pilot whose log claims more than three harmonics, I want the drawn orders capped at three, so that a fourth style is not invented for an order Betaflight cannot filter.
21. As an FPV pilot, I want the overlay to cost nothing when it is off, so that turning families on and off never recomputes analysis.
22. As an FPV pilot in light mode, I want the motor hues to keep their contrast, so that the overlay is legible in either theme.
23. As an FPV pilot, I want the hover readout on the Spectrogram to keep working with curves drawn, so that adding an overlay does not cost me the values underneath.
24. As a contributor, I want the harmonic geometry to stay in the analysis layer and the hue-and-style choice to stay in the UI layer, so that the two can change independently.
25. As a contributor, I want the eRPM-to-frequency conversion to remain in one place, so that the PSD bands and the Spectrogram curves can never disagree about where a motor was.

## Implementation Decisions

**Geometry stays in `analysis::overlays`, computed at load.**
`OverlayShape::Harmonics` keeps its list of per-motor per-order bands and the
band keeps its existing shape — motor index, order, low and high frequency, and
the `filtered` flag for a zero-weight order. What changes is the meaning of low
and high: the 5th and 95th percentile of that motor's *running* eRPM, rather
than its min and max. Stopped-motor samples continue to be excluded before the
percentile is taken, so a percentile is over flight, not over arming. Samples
are uniform in time, so a sample-count percentile is already time-weighted; no
separate dwell histogram is needed.

**Orders are clamped to three.** Betaflight's own maximum for RPM filter
harmonics is three; the header is read raw, so a log claiming more is clamped
and the clamp is logged at `debug`. Without an RPM filter, one order — the
fundamental — is drawn, and it is not claimed to be attenuated. Both are the
existing rules, kept.

**Identity moves from order to motor, in `app::colors`.** The per-order hue
lookup is replaced by a per-motor one: four fixed hues, cycled for motors five
through eight, luminance still taken from the installed palette so both themes
stay legible. Order is carried by `egui_plot`'s line style — solid, dashed,
dotted for orders one through three — which `Span` supports on its border and
`Line` supports directly. A zero-weight order keeps the same hue and style and
is drawn dimmed, on both panels; the old "outline versus fill" distinction dies
with the fill.

**The PSD draws unfilled spans.** Per motor per order, hue by motor, border
style by order, no fill — twelve filled spans obscure the curve. No per-span
labels. A legend row renders above the plots, only while the family is on, with
one swatch per motor present in the log and one sample per order drawn.

**The Spectrogram reads eRPM at draw time.** No new stored analysis: the panel
already receives the flight data, and the dynamic notch trace sets the precedent
of building an overlay series from raw samples per frame. Per motor per order the
series is that motor's eRPM converted to Hz through the metadata's existing
conversion and multiplied by the order. Cost is a map over samples that the plot
then decimates.

**The heatmap takes a list of overlay series, not one.** Its single optional
series becomes a list, and each entry carries its own colour and line style. The
dynamic notch trace becomes one entry in that list rather than a special case.

**The Spectrogram gains the shared overlay menu**, with its own visibility
instance as every sub-tab has. Harmonics and the dynamic notch trace are both
families in it, and both default off with everything else — which means the
dynamic notch trace, drawn unconditionally today, will not appear until a pilot
ticks it. That is the accepted cost of one rule for every overlay on the panel.
The menu's description of the Harmonics family is reworded, since it no longer
describes bands coloured by order.

**Peak classification is deliberately excluded** — see Out of Scope.

## Testing Decisions

A good test here asserts what a pilot could see: how many bands exist, at which
frequencies, which are marked unfiltered, and that two motors never share a
drawn identity. It does not assert the intermediate arrays a percentile was
taken from, nor anything about how a span is submitted to the plot.

**The loader seam is the primary one and already exists.** The end-to-end
harmonic test in the loader integration tests loads a fixture and inspects the
resulting overlays. It is extended rather than replaced:

- The band count invariant holds — four motors at three orders on the steady
  hover fixture, whose RPM filter configures three harmonics with non-zero
  weights.
- The fundamental's band still lands in a plausible hover range and the third
  order is still exactly three times the first per motor, which is what catches
  a pole count or a unit error.
- New: the percentile band is strictly narrower than the full extent on a
  fixture whose throttle moved, which is the whole behavioural change. A band
  equal to min..max means the percentile was not applied.
- The existing zero-weight-harmonic test over the multi-flight `.bbl` fixture is
  unchanged in intent and stays ignored by default for the same reason.

**Percentile and clamp mechanics are unit-tested in `analysis::overlays`**,
against synthetic flight data as that module's existing tests do — a motor held
at one frequency for most of the window with brief excursions must produce a band
that excludes the excursions, and a header claiming five harmonics must produce
three orders.

**`app::colors` keeps its two existing property tests**, retargeted from orders
to motors: every drawn motor hue clears the contrast floor against the
background in both themes, and no two of the first four motors share a colour.

**Rendering is not tested.** Nothing in this repo tests egui output, and a test
asserting a border style was set would assert the implementation and nothing a
pilot sees. The legend, the span styling, the curve clipping and the new menu on
the Spectrogram are verified by running the app against a fixture log.

## Out of Scope

- **Classifying peaks as motor noise versus resonance in prose.** Adding a
  motor-source field to a detected peak, and the sentence under the plot that
  reads it out, is the natural next step and the reason the narrower bands
  matter — but it needs the percentile geometry settled first, and it changes
  what the AI context carries. Its own spec.
- **The Frequency sub-tab.** Same frequency axis as the PSD, so the same spans
  would work, but it has no overlay plumbing at all today. Adding it is a
  separate change.
- **Per-motor or per-order sub-toggles** in the overlay menu. The family switch
  turns the lot off, which is enough; overlapping motors are the signal, not
  clutter to be filtered.
- **Eight distinct motor hues.** Hues that all clear the contrast floor would
  crowd into indistinguishable neighbours; quads are the target and a hex still
  draws.
- **Persisting overlay visibility** across launches. There is still no settings
  store.

## Further Notes

The dynamic notch trace becoming opt-in is a visible regression for anyone who
relies on it today, and it is the one part of this change a user could file a bug
about. It is accepted because the alternative leaves two overlays on the same
panel obeying different rules.

PIDToolbox is the reference for both encodings: per-motor curves over the
spectrogram, and harmonic markers over the PSD. Matching a view pilots already
read is worth more here than inventing a better one.
