# Spec: filter overlays drawn on the data

Status: done

## Problem Statement

The Harmonics family answers its question. A pilot turns it on and the peak in
front of them is either sitting under a recoloured stretch of their own
spectrum — motor 3, second order — or it is not, and it is the frame. The
answer is made of measured data, and the family says both where it applies and
where it does not.

No other overlay family does this.

Every filter family draws a `FilterResponse` — a curve of gain against
frequency, correct in shape, a V for a notch and a rolloff for a lowpass —
hung off `anchor_db`, a bare `hline` sitting at the raw spectrum's peak. All of
them share one colour. The curve is a picture of the filter in the abstract,
floating over the plot, and the pilot's question is not abstract. It is *did
this filter do anything to that peak*. Three specific ways the panel cannot
answer it:

**Nothing is anchored to the data.** `anchor_db` is the raw peak, which is a
level, not a reference — the curve descends from it by the filter's gain, so
"how far down" is measured against a line that has no physical relationship to
the frequency the pilot is looking at. A stage that takes 12 dB off a 340 Hz
peak and a stage that takes 12 dB off silence draw the identical mark.

**Stages overlap and nothing totals them.** Gyro LPF1, LPF2 and the dynamic
notch can all be cutting 300 Hz. The panel draws three separate curves in one
colour and leaves the pilot to multiply them by eye, which is not a thing eyes
do in decibels.

**Dynamic filters are silently averaged.** `gyro_lpf1` is dynamic by default in
Betaflight, so the curve labelled `Gyro LPF1` is a dwell-weighted average over
the cutoffs the throttle actually produced, and the label says none of that —
it reads as one soft static rolloff, which is the one thing it is not. The
`Dwell` histogram that produced it is computed at load and thrown away. The
dynamic notch has the same problem per axis: its `Traced` average is a shallow
trough, and a shallow trough is what a pinned notch and a roaming notch both
look like once the difference has been averaged out of them.

## Solution

Four changes, one idea: an overlay is a claim about *this spectrum*, so it is
drawn against this spectrum.

### The gyro chain is drawn on the data

The visible gyro stages are cascaded into one **chain total**, and it is drawn
at `raw_db − chain_gain_db` — on the raw curve's own scale, at the raw curve's
own frequencies. The region between the raw trace and the chain total is
filled. That fill *is* the energy the gyro filter chain removed: thick where
the chain worked, a hairline where it did nothing, and the pilot reads "where
is this filter actually being used" as "where is the fill thick", with no
legend and no arithmetic.

Individual stage curves survive underneath, thin and dimmed, anchored the same
way. They are shape, not magnitude: which of three stages owns a given bin is
not a question the plot tries to answer, because the honest answer at an
overlap is "all of them, multiplied".

No threshold. The fill is drawn everywhere the chain total differs from raw at
all. A threshold would draw a vertical edge at 3 dB that the physics does not
have — the same "boundary where there is none" defect that killed the harmonic
spans in the previous milestone.

### The D-term chain is not

The PSD plots gyro power: `raw_psd` from `gyroUnfilt`, `filtered_psd` from
`gyroADC`. The D-term lowpasses never touched that signal. Anchoring them to
the raw curve would claim an attenuation that did not happen to the trace being
drawn.

So the D-term stages stay, because that 200 Hz peak is exactly what the D-term
LPF has to survive and a pilot wants to see both at once — but they are visibly
a different kind of object. Unanchored, in their own hue, hanging from a
labelled 0 dB reference line that appears only while a D-term family is on.
`anchor_db` was meaningless as a shared anchor and becomes meaningful as a
dedicated one: it is the D-term chain's unity gain, and nothing else uses it.

### Dynamic filters say that they moved, and where they lived

Two additions, both from the `Dwell` histogram that already exists and is
currently discarded:

- **The name carries the range it really used** — `Gyro LPF1 (dyn, 180–420 Hz)`,
  from the 5th to 95th percentile of the realised cutoffs, not the configured
  min..max. Where no throttle was logged there is no realised range, and the
  label says `config` so the pilot knows which they are reading.
- **The dwell is drawn as a filled histogram along the plot floor**, in the
  chain hue. A pinned notch is a spike, a roaming one a plateau. Time is a
  third variable on a plot whose two axes are spent, so it gets its own strip
  of pixels rather than being encoded into the curve — a curve faded by dwell
  reads as uncertainty, and is indistinguishable from a curve that is merely
  shallow.

### Two hues, not one and not eight

Gyro chain and D-term chain get one hue each. Everything within a chain — total,
per-stage curves, fill, dwell strip — shares its chain's hue and separates by
width and alpha. Hue is spent on the distinction the pilot cannot derive:
*which stage* a curve is, is derivable (the corner label says so, and LPF2 is
always above LPF1), whereas *which loop* it belongs to is not derivable from
the shape and is the one that changes what the pilot types into the CLI.

## Implementation shape

The chain total depends on which families are visible, so it cannot be a stored
product — but recomputing it must not be expensive. Two grids solve it:

- **Every stage's power gain is precomputed at load, on the spectrum's own
  frequency grid.** `window_size_for` gives a 0.128 s window at any sample
  rate, so that grid is ~513 bins at ~7.81 Hz. Per frame the chain total is an
  elementwise product over the visible stages: ~513 × 5 multiplies per axis, on
  the order of ten thousand for the panel, which is arithmetic and not a
  recomputation. The fill needs no resampling, because both of its edges are on
  the PSD's bins.
- **The fine 512-point `FilterResponse` stays** for drawing the per-stage curve,
  so a notch still looks like a V. Anchoring it to the raw trace at its own
  frequencies is an interpolation the codebase already has: `ui::hover::y_at`.

Evaluating the chain on 7.81 Hz bins cannot represent a null narrower than a
bin, and that is correct rather than a compromise: the PSD cannot show
attenuation finer than its own resolution either, so a notch that fits between
two bins did not visibly do anything to the spectrum being drawn.

Cascading is a product of each stage's *expected* gain. For two dynamic stages
whose settings both track throttle this treats them as independent and is a
mild approximation; it is noted where it is done, and it is a smaller error
than the current panel's, which is to draw no total at all.

## Decisions

| Decision | Rationale |
|---|---|
| Modelled response anchored to the raw spectrum, not measured `raw − filtered` | Measured attenuation is the truth but it is the whole chain at once and cannot be attributed to a stage. Anchoring the model puts it on the data without pretending to a per-stage measurement nothing can make |
| The removed energy is a fill between two curves, not a recolour of one | Harmonics already own recolouring the raw trace, and the two would fight for the same pixels wherever a harmonic sits inside a rolloff. A fill never collides, and it is the better picture: an area is a quantity, and "what the chain removed" is a quantity |
| No dB threshold on the fill | Thickness is the signal. A cut-in at 3 dB invents a vertical edge, which is exactly the misread that the harmonic spans were removed for |
| Chain total is the product of the *visible* families, per frame | CLAUDE.md's "toggling never recomputes anything" exists so a click cannot trigger an FFT. Multiplying precomputed gain arrays is not that. The alternative — a fixed whole-chain total — lies to a pilot who switched the dynamic notch off and still sees its cut in the total |
| D-term stages are drawn but never anchored | They acted on a signal this plot does not show. Removing them loses real tuning context; anchoring them states a falsehood about the trace. Unanchored, in their own hue, off their own reference line, is the only option that is both present and true |
| Two filter hues, one per loop, not one per stage | Which stage a curve is, is derivable from its corner and its order. Which loop it belongs to is not, and it is the one that changes the CLI line the pilot types |
| Dynamic filters get their dwell drawn on the plot floor | Time is a third variable and the two axes are spent. A dedicated strip says "spent its time here"; alpha on the curve says "we are unsure", which is a different and wrong claim |
| A dynamic stage's label carries its realised p5–p95, not its configured range | The configured range is what the filter was allowed to do. The realised range is what it did, and it is the same distinction the harmonic bands were narrowed to make |
| `OverlayShape::Band` dies for the dynamic LPF, replaced by an envelope of two real rolloffs | A span says "everything in here is gone", the precise misread the response curves were introduced to kill. Two rolloffs at the configured extremes say "somewhere between these" in the plot's own language |
| The dynamic notch's configured bounds become a floor-lane marker | With no trace to average, the bounds are the same kind of claim as dwell — where it was allowed to be — so they belong in the lane that means that, not over the spectrum |
| `OverlayShape::Line` stays | A notch whose cutoff yields no Q has no derivable shape, it is rare, and nothing better exists to draw |

## Out of scope

- ~~**The Spectrogram and Vs Reference.**~~ Done as a follow-up in the same
  branch: both maps now draw every family on their own two axes, with a stage
  the throttle drove as the firmware's own curve rather than its average — the
  "genuinely new and correct thing" this section deferred. See
  `tabs::filter_analysis::filter_marks`.
- **Marking where the predicted total disagrees with the measured filtered
  PSD.** It is the real diagnosis and it is deliberately deferred: the model
  ignores stage ordering and cascade interaction, and `gyroADC` has been
  through the whole loop, so a normal amount of disagreement is unknown. A
  threshold picked before seeing real logs would only generate false alarms.
  Revisit with the fill on screen and real flights behind it.
- **Per-stage attribution at a bin.** At an overlap the honest answer is "all of
  them", and the chain total says it.
