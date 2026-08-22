//! What the filters actually occupy in frequency, computed once at load time.
//!
//! Replaces a line-shaped marker per filter. A notch has a bandwidth, a
//! dynamic filter has a range it swept, and the RPM filter has one band per
//! motor per harmonic — drawing any of them as a single line at a nominal
//! centre tells a pilot where a setting says the filter is, not where the
//! filter was.

use super::filter_response::{self, FilterResponse, Stage};
use crate::parser::metadata::{FilterConfig, LowpassConfig, NotchConfig, RpmFilterConfig};
use crate::parser::{Axis, Channel, Metadata, PerAxis, Trimmed};

/// Which PID loop a filter stage feeds. Part of the family rather than the
/// label, so a panel selects gyro overlays by matching the type instead of by
/// `label.starts_with("Gyro")`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterLoop {
    Gyro,
    Dterm,
}

impl FilterLoop {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Gyro => "Gyro",
            Self::Dterm => "D-term",
        }
    }
}

/// What kind of filter an overlay describes, and which loop it feeds. The
/// panel toggles by family; what a family is *called* is the panel's business,
/// not this module's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayFamily {
    Harmonics,
    DynNotch,
    Notch(FilterLoop),
    Lowpass(FilterLoop),
}

impl OverlayFamily {
    /// Which chain a family belongs to, so a panel can cascade the visible
    /// gyro stages without matching on each variant. The dynamic notch is a
    /// gyro stage; the harmonics are not a filter at all.
    pub const fn filter_loop(self) -> Option<FilterLoop> {
        match self {
            Self::Harmonics => None,
            Self::DynNotch => Some(FilterLoop::Gyro),
            Self::Notch(loop_) | Self::Lowpass(loop_) => Some(loop_),
        }
    }

    pub const ALL: [Self; 6] = [
        Self::Harmonics,
        Self::DynNotch,
        Self::Notch(FilterLoop::Gyro),
        Self::Notch(FilterLoop::Dterm),
        Self::Lowpass(FilterLoop::Gyro),
        Self::Lowpass(FilterLoop::Dterm),
    ];
}

/// One motor's noise at one harmonic order, over the frequencies it spent the
/// analysed window at.
#[derive(Debug, Clone, PartialEq)]
pub struct HarmonicBand {
    pub motor: usize,
    /// 1 is the fundamental.
    pub order: u32,
    /// The 5th and 95th percentile of this motor's running eRPM, times the
    /// order — not its two extremes. A band drawn from idle to full song
    /// covers most of the spectrum, and a band that covers everything cannot
    /// say that a peak came from somewhere else.
    pub low_hz: f64,
    pub high_hz: f64,
    /// False where this order's RPM filter weight is zero — the filter tracks
    /// the harmonic but takes nothing off it, which is not the same as being
    /// filtered and has to look different.
    pub filtered: bool,
}

/// Where a filter that moved actually sat, as time spent per setting.
///
/// The intermediate a swept response is averaged over — and kept, rather than
/// dropped once the average is taken: a curve says what the filter removed,
/// and a pilot also wants to know whether it was pinned on one frequency or
/// roaming across a range, which the average has exactly divided out.
#[derive(Debug, Clone, PartialEq)]
pub struct Dwell {
    /// Bin centres, Hz.
    pub freq_hz: Vec<f64>,
    /// Fraction of the analysed window spent in each bin, summing to 1.
    pub weight: Vec<f64>,
}

impl Dwell {
    /// The bins the filter actually visited, as weighted stages.
    fn stages(&self, stage_at: impl Fn(f64) -> Stage) -> Vec<(Stage, f64)> {
        self.freq_hz
            .iter()
            .zip(&self.weight)
            .filter(|&(_, &w)| w > 0.0)
            .map(|(&hz, &w)| (stage_at(hz), w))
            .collect()
    }

    /// The setting this filter spent `q` of the window at or below. Weights are
    /// time, so the rank is already time-weighted — the same rule the harmonic
    /// bands are cut to.
    fn percentile(&self, q: f64) -> Option<f64> {
        let mut cumulative = 0.0;
        self.freq_hz
            .iter()
            .zip(&self.weight)
            .find(|&(_, &w)| {
                cumulative += w;
                cumulative >= q && w > 0.0
            })
            .map(|(&hz, _)| hz)
    }

    /// The range this filter really used, against the range it was allowed:
    /// the configured min..max is what the firmware could have done, and on a
    /// real flight it is most of the spectrum.
    fn realised_range(&self) -> Option<(f64, f64)> {
        let (low, high) = BAND_PERCENTILES;
        Some((self.percentile(low)?, self.percentile(high)?))
    }
}

/// A value an overlay carries once for the whole log, or once per axis where
/// the firmware logs it per axis — the dynamic notch's tracked centre.
#[derive(Debug, Clone, PartialEq)]
pub enum ByAxis<T> {
    Shared(T),
    PerAxis(PerAxis<Option<T>>),
}

impl<T> ByAxis<T> {
    pub fn get(&self, axis: Axis) -> Option<&T> {
        match self {
            Self::Shared(value) => Some(value),
            Self::PerAxis(per_axis) => per_axis[axis].as_ref(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum OverlayShape {
    /// A filter whose shape we cannot derive — a notch with no usable cutoff.
    /// Everything we can size draws its real response instead.
    Line { hz: f64 },
    /// Where a filter was *allowed* to work, with nothing logged to say where
    /// it went: the dynamic notch's configured bounds. Drawn in the dwell lane
    /// along the plot floor rather than over the spectrum — a span across the
    /// curve reads as "everything in here is gone", which is the misread the
    /// response curves exist to kill.
    Allowed { low_hz: f64, high_hz: f64 },
    /// One band per motor per harmonic order.
    Harmonics(Vec<HarmonicBand>),
    /// What a filter took off, per frequency.
    Response(FilterResponse),
    /// The two rolloffs a dynamic lowpass ran somewhere between, where the log
    /// carried no throttle to weight the sweep with. Two real curves say
    /// "somewhere between these" in the plot's own language.
    Envelope {
        low: FilterResponse,
        high: FilterResponse,
    },
    /// The same, where it had to be measured per axis — the dynamic notch,
    /// whose centre the firmware logs one of per axis.
    Traced(PerAxis<Option<FilterResponse>>),
}

/// A filter's geometry in the spectrum, with the family that toggles it.
#[derive(Debug, Clone, PartialEq)]
pub struct FilterOverlay {
    pub label: String,
    pub family: OverlayFamily,
    pub shape: OverlayShape,
    /// This stage's power gain on the spectrum's own frequency grid, so the
    /// chain total is an elementwise product over the visible stages and the
    /// fill's two edges share their x with the raw trace.
    ///
    /// `None` where there is no stage to model: the harmonic bands, a notch
    /// with no usable Q, and a dynamic lowpass with no throttle to weight —
    /// that last one has bounds but no expected gain, and a total must not
    /// claim a cut nothing can weight.
    pub gain: Option<ByAxis<Vec<f64>>>,
    /// Where a stage that moved spent its time. `None` for a static one.
    pub dwell: Option<ByAxis<Dwell>>,
}

/// How many bins the traced centre is reduced to before the response is
/// averaged over them. Enough that a tracker sweeping its range averages to a
/// smooth trough, few enough to keep the averaging cheap.
const TRACE_BINS: usize = 64;

/// Betaflight's own ceiling for RPM filter harmonics. The header is read raw,
/// so a log claiming more is clamped rather than inventing a fourth identity
/// for an order the firmware cannot filter.
const MAX_HARMONIC_ORDERS: u32 = 3;

/// Where a motor spent the flight, rather than the two extremes it touched.
/// The full min..max of a freestyle log runs from idle to full song, and three
/// orders of that wash over most of the spectrum — a band covering everything
/// can never tell a pilot that a peak is *not* motor noise.
const BAND_PERCENTILES: (f64, f64) = (0.05, 0.95);

/// Throttle is logged on the stick scale, and the dynamic cutoff curve wants
/// a fraction.
const THROTTLE_MIN: f64 = 1000.0;
const THROTTLE_SPAN: f64 = 1000.0;

/// The two things every stage's geometry is measured against: the rate the
/// filters ran at, and the spectrum's own frequency bins, which the chain
/// total is re-multiplied over per frame.
struct Grid<'a> {
    /// The PID loop rate — a log written every second frame would otherwise
    /// show every stage rolling off far earlier than it does.
    fs: f64,
    /// The PSD's bins. Evaluating a gain here cannot represent a null narrower
    /// than a bin, which is correct rather than a compromise: the spectrum
    /// cannot show attenuation finer than its own resolution either.
    freq_hz: &'a [f64],
}

impl Grid<'_> {
    /// One stage's expected power gain across the spectrum's bins, from the
    /// settings it ran at — a static stage is one `(stage, 1.0)` pair.
    fn gain(&self, settings: &[(Stage, f64)]) -> Vec<f64> {
        filter_response::cascade(&[settings], self.freq_hz, self.fs)
    }

    fn stage_gain(&self, stage: Stage) -> Vec<f64> {
        self.gain(&[(stage, 1.0)])
    }
}

/// Every overlay this log can support, over the same window the spectra were
/// measured on. `spectrum_hz` is the PSD's frequency grid, shared by every
/// axis, and the grid every stage's gain is precomputed on.
pub(super) fn build(
    fd: &Trimmed<'_>,
    metadata: &Metadata,
    spectrum_hz: &[f64],
) -> Vec<FilterOverlay> {
    let cfg = &metadata.filters;
    let grid = Grid {
        fs: metadata.filter_rate_hz(fd.sample_rate_hz()),
        freq_hz: spectrum_hz,
    };
    let mut overlays = Vec::new();

    overlays.extend(harmonics(fd, metadata));
    overlays.extend(dyn_notch(fd, metadata, &grid));
    overlays.extend(notches(&cfg.gyro_notches, FilterLoop::Gyro, &grid));
    overlays.extend(notches(&cfg.dterm_notches, FilterLoop::Dterm, &grid));
    overlays.extend(lowpasses(cfg, fd.throttle(), &grid));
    overlays
}

/// A band per motor per harmonic order. The order count is the RPM filter's,
/// so the plot matches the Betaflight setting rather than a constant — clamped
/// to what the firmware can actually filter; without an RPM filter only the
/// fundamental is drawn, and nothing claims it is attenuated.
fn harmonics(fd: &Trimmed<'_>, metadata: &Metadata) -> Option<FilterOverlay> {
    let rpm_filter = metadata.filters.rpm_filter.as_ref();
    let configured = rpm_filter.map_or(1, |r| r.harmonics).max(1);
    let orders = configured.min(MAX_HARMONIC_ORDERS);
    if orders < configured {
        tracing::debug!(
            "rpm_filter_harmonics reads {configured}; drawing {orders}, Betaflight's own maximum"
        );
    }

    let bands: Vec<HarmonicBand> = (0..fd.rpm_count())
        .filter_map(|motor| {
            let (low, high) = spinning_span(fd.channel(Channel::Rpm(motor))?)?;
            let (low, high) = (metadata.erpm_to_hz(low), metadata.erpm_to_hz(high));

            Some((1..=orders).map(move |order| HarmonicBand {
                motor,
                order,
                low_hz: low * order as f64,
                high_hz: high * order as f64,
                filtered: is_filtered(rpm_filter, order),
            }))
        })
        .flatten()
        .collect();

    // No gain: the harmonics are noise the filters are aimed at, not a stage
    // in the chain, so they take no part in its total.
    (!bands.is_empty()).then(|| FilterOverlay {
        label: "Motor harmonics".to_string(),
        family: OverlayFamily::Harmonics,
        shape: OverlayShape::Harmonics(bands),
        gain: None,
        dwell: None,
    })
}

/// Where a motor actually spent the window, as a percentile of its *running*
/// eRPM. Stopped samples are dropped first: a band running down to zero
/// describes a prop that was not turning, not a frequency the craft flew.
///
/// Samples are uniform in time, so a rank over them is already time-weighted —
/// a brief blip to full throttle moves the 95th percentile by as little as it
/// occupied the flight, which is the whole point of not taking the maximum.
fn spinning_span(samples: &[f64]) -> Option<(f64, f64)> {
    let mut running: Vec<f64> = samples.iter().copied().filter(|&v| v > 0.0).collect();
    if running.is_empty() {
        return None;
    }
    running.sort_by(f64::total_cmp);

    let at = |q: f64| running[((running.len() - 1) as f64 * q).round() as usize];
    let (low, high) = BAND_PERCENTILES;
    Some((at(low), at(high)))
}

/// A weight of zero means the RPM filter tracks this harmonic and attenuates
/// nothing at it. No RPM filter at all is the same claim.
fn is_filtered(rpm_filter: Option<&RpmFilterConfig>, order: u32) -> bool {
    rpm_filter.is_some_and(|r| {
        r.weights
            .get(order as usize - 1)
            .copied()
            .unwrap_or(1.0)
            .abs()
            > 0.0
    })
}

/// The configured range as a band, and — where the log was flown in
/// `FFT_FREQ` — what the notch actually took off, from the centres the
/// tracker chose.
fn dyn_notch(fd: &Trimmed<'_>, metadata: &Metadata, grid: &Grid<'_>) -> Vec<FilterOverlay> {
    let Some(cfg) = &metadata.filters.dyn_notch else {
        return Vec::new();
    };
    let (low_hz, high_hz) = (cfg.min_hz as f64, cfg.max_hz as f64);

    let mut overlays = vec![FilterOverlay {
        label: match cfg.count {
            1 => "Dyn notch range".to_string(),
            n => format!("Dyn notch range (×{n})"),
        },
        family: OverlayFamily::DynNotch,
        // Where it was allowed to be, and no more: with no trace there is
        // nothing to say about where it went, so it draws in the floor lane
        // that means exactly that.
        shape: OverlayShape::Allowed { low_hz, high_hz },
        gain: None,
        dwell: None,
    }];

    if metadata.logs_dyn_notch_trace() {
        let q = cfg.q as f64;
        let notch_at = move |centre_hz| Stage::Notch { centre_hz, q };
        let dwells = PerAxis(Axis::ALL.map(|axis| {
            fd.debug_axis(axis)
                .and_then(|s| dwell_histogram(s, low_hz, high_hz))
        }));

        let traced = PerAxis(Axis::ALL.map(|axis| {
            let pad = (high_hz / q).max(10.0);
            filter_response::weighted(
                &dwells[axis].as_ref()?.stages(notch_at),
                // Past the configured bounds, out to where the skirts have
                // recovered — a V cut off at a bound would look like a wall
                // the filter does not have.
                low_hz - pad,
                high_hz + pad,
                grid.fs,
            )
        }));

        if traced.0.iter().any(Option::is_some) {
            overlays.push(FilterOverlay {
                // One notch, however many were configured: Betaflight logs one
                // centre per axis, so the others cannot be drawn and this does
                // not pretend they were.
                //
                // No realised range in the label either, unlike a dynamic
                // lowpass: there are three of them, one per axis, and a single
                // name cannot carry three. The dwell lane says it per axis.
                label: "Dyn notch response (traced)".to_string(),
                family: OverlayFamily::DynNotch,
                shape: OverlayShape::Traced(traced),
                gain: Some(ByAxis::PerAxis(PerAxis(Axis::ALL.map(|axis| {
                    Some(grid.gain(&dwells[axis].as_ref()?.stages(notch_at)))
                })))),
                dwell: Some(ByAxis::PerAxis(dwells)),
            });
        }
    }

    overlays
}

/// Binned over the range the tracker was allowed rather than over the range it
/// used — a tracker pinned at its configured maximum has to read as pinned,
/// which a histogram rescaled to its own extent cannot show.
fn dwell_histogram(samples: &[f64], low_hz: f64, high_hz: f64) -> Option<Dwell> {
    let (low, high) = match high_hz > low_hz {
        true => (low_hz, high_hz),
        false => return None,
    };
    let width = (high - low) / TRACE_BINS as f64;

    let mut counts = vec![0.0; TRACE_BINS];
    let mut total = 0.0;
    // Zero is the channel before the tracker has run, not a centre it chose.
    for &v in samples.iter().filter(|v| v.is_finite() && **v > 0.0) {
        // Clamped rather than dropped: the firmware holds its tracker inside
        // the configured range, so a sample outside it belongs to the end bin
        // it was held against.
        let bin = (((v - low) / width) as isize).clamp(0, TRACE_BINS as isize - 1) as usize;
        counts[bin] += 1.0;
        total += 1.0;
    }
    if total == 0.0 {
        return None;
    }

    Some(Dwell {
        freq_hz: (0..TRACE_BINS)
            .map(|i| low + (i as f64 + 0.5) * width)
            .collect(),
        weight: counts.into_iter().map(|c| c / total).collect(),
    })
}

fn notches(configs: &[NotchConfig], loop_: FilterLoop, grid: &Grid<'_>) -> Vec<FilterOverlay> {
    configs
        .iter()
        .enumerate()
        .map(|(i, notch)| {
            let (shape, stage) = notch_shape(notch, grid.fs);
            FilterOverlay {
                label: format!("{} notch {}", loop_.name(), i + 1),
                family: OverlayFamily::Notch(loop_),
                shape,
                gain: stage.map(|stage| ByAxis::Shared(grid.stage_gain(stage))),
                dwell: None,
            }
        })
        .collect()
}

/// Betaflight derives a notch's Q from its centre and cutoff
/// (`filterGetNotchQ`), and the V it cuts follows from the two. A cutoff at or
/// above the centre is not a notch this can size, and stays a bare line rather
/// than a curve of invented depth — and a shape we cannot derive is a shape we
/// cannot cascade either, so it carries no stage.
fn notch_shape(notch: &NotchConfig, sample_rate_hz: f64) -> (OverlayShape, Option<Stage>) {
    let (centre_hz, cutoff) = (notch.center_hz as f64, notch.cutoff_hz as f64);
    let line = (OverlayShape::Line { hz: centre_hz }, None);

    let q = match cutoff > 0.0 && centre_hz > cutoff {
        true => centre_hz * cutoff / (centre_hz * centre_hz - cutoff * cutoff),
        false => return line,
    };
    let stage = Stage::Notch { centre_hz, q };

    match filter_response::of(stage, sample_rate_hz) {
        Some(response) => (OverlayShape::Response(response), Some(stage)),
        None => line,
    }
}

/// Every lowpass stage, as the rolloff it is. A dynamic LPF1 is averaged over
/// the cutoffs the throttle actually took it to, the same way the dynamic
/// notch is averaged over the centres its tracker chose.
fn lowpasses(cfg: &FilterConfig, throttle: Option<&[f64]>, grid: &Grid<'_>) -> Vec<FilterOverlay> {
    let mut overlays = Vec::new();
    let mut push = |name: &str, loop_: FilterLoop, drawn: Option<Lowpass>| {
        if let Some(drawn) = drawn {
            overlays.push(FilterOverlay {
                label: format!("{name}{}", drawn.suffix),
                family: OverlayFamily::Lowpass(loop_),
                shape: drawn.shape,
                gain: drawn.settings.map(|s| ByAxis::Shared(grid.gain(&s))),
                dwell: drawn.dwell.map(ByAxis::Shared),
            });
        }
    };

    if let Some(lpf) = &cfg.gyro_lpf1 {
        push(
            "Gyro LPF1",
            FilterLoop::Gyro,
            lowpass_shape(lpf, throttle, grid),
        );
    }
    if let Some(lpf) = &cfg.gyro_lpf2 {
        push(
            "Gyro LPF2",
            FilterLoop::Gyro,
            static_lowpass(lpf.cutoff_hz as f64, lpf.filter_type, grid.fs),
        );
    }
    if let Some(lpf) = &cfg.dterm_lpf1 {
        push(
            "D-term LPF1",
            FilterLoop::Dterm,
            lowpass_shape(lpf, throttle, grid),
        );
    }
    if let Some(lpf) = &cfg.dterm_lpf2 {
        push(
            "D-term LPF2",
            FilterLoop::Dterm,
            static_lowpass(lpf.cutoff_hz as f64, lpf.filter_type, grid.fs),
        );
    }
    overlays
}

/// One lowpass stage as it will be drawn: its shape, what its name has to
/// carry beyond the stage's own, the settings its gain is cascaded from, and
/// where it spent its time.
struct Lowpass {
    shape: OverlayShape,
    /// `` for a static stage, `(dyn, 180–420 Hz)` for one that moved. A
    /// dynamic stage labelled like a static one reads as one soft rolloff,
    /// which is the one thing it is not.
    suffix: String,
    settings: Option<Vec<(Stage, f64)>>,
    dwell: Option<Dwell>,
}

fn static_lowpass(
    cutoff_hz: f64,
    filter_type: crate::parser::metadata::FilterType,
    fs: f64,
) -> Option<Lowpass> {
    let stage = Stage::Lowpass {
        cutoff_hz,
        filter_type,
    };
    Some(Lowpass {
        shape: OverlayShape::Response(filter_response::of(stage, fs)?),
        suffix: String::new(),
        settings: Some(vec![(stage, 1.0)]),
        dwell: None,
    })
}

/// A dynamic lowpass swept its corner all flight, so no one rolloff describes
/// it. Averaged over the cutoffs the throttle actually produced: a flight held
/// at one throttle draws that stage's own curve, and one worked across the
/// range draws the shallower, wider average of the corners it passed through.
///
/// Without throttle there is no sweep to weight, and the two ends of the
/// configured range are all that can honestly be claimed.
fn lowpass_shape(
    lpf: &LowpassConfig,
    throttle: Option<&[f64]>,
    grid: &Grid<'_>,
) -> Option<Lowpass> {
    let (fs, filter_type) = (grid.fs, lpf.filter_type);
    if !lpf.is_dynamic() {
        return static_lowpass(lpf.static_hz as f64, filter_type, fs);
    }
    let lowpass_at = move |cutoff_hz| Stage::Lowpass {
        cutoff_hz,
        filter_type,
    };
    let (min, max) = (lpf.dyn_min_hz as f64, lpf.dyn_max_hz as f64);

    // No throttle, no sweep to weight: two real rolloffs at the configured
    // extremes say "somewhere between these", and the label says the range is
    // the configured one rather than one this flight was measured to use.
    let Some(dwell) = throttle.and_then(|t| dwell_histogram(&cutoffs(lpf, t), min, max)) else {
        return Some(Lowpass {
            shape: OverlayShape::Envelope {
                low: filter_response::of(lowpass_at(min), fs)?,
                high: filter_response::of(lowpass_at(max), fs)?,
            },
            suffix: format!(" (dyn, {}, config)", range_hz(min, max)),
            settings: None,
            dwell: None,
        });
    };

    let settings = dwell.stages(lowpass_at);
    let (low, high) = dwell.realised_range().unwrap_or((min, max));
    Some(Lowpass {
        shape: OverlayShape::Response(filter_response::weighted(&settings, 1.0, max * 8.0, fs)?),
        suffix: format!(" (dyn, {})", range_hz(low, high)),
        settings: Some(settings),
        dwell: Some(dwell),
    })
}

/// The two ends of a range, as a pilot reads a range.
fn range_hz(low: f64, high: f64) -> String {
    format!("{low:.0}–{high:.0} Hz")
}

/// The cutoff this stage ran at on every logged frame, from the throttle the
/// pilot was holding — Betaflight's own dynamic curve, not a straight line
/// between the two ends.
fn cutoffs(lpf: &LowpassConfig, throttle: &[f64]) -> Vec<f64> {
    throttle
        .iter()
        .map(|&raw| {
            let fraction = ((raw - THROTTLE_MIN) / THROTTLE_SPAN).clamp(0.0, 1.0);
            lpf.cutoff_at(fraction as f32) as f64
        })
        .collect()
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::analysis::filter_response::MIN_GAIN_DB;
    use crate::parser::FlightData;
    use crate::parser::metadata::{DynNotchConfig, FilterType, StaticLowpassConfig};

    /// 8 kHz, and a coarse stand-in for the PSD's bins — the grid a gain is
    /// precomputed on is the spectrum's, whatever its resolution.
    fn grid() -> Grid<'static> {
        const BINS: &[f64] = &[100.0, 200.0, 300.0, 400.0, 500.0, 600.0];
        Grid {
            fs: 8000.0,
            freq_hz: BINS,
        }
    }

    fn notch(center_hz: f32, cutoff_hz: f32) -> NotchConfig {
        NotchConfig {
            center_hz,
            cutoff_hz,
        }
    }

    /// A notch is a V at its centre, not a rectangle across its bandwidth.
    /// How wide and how deep is `filter_response`'s claim; that the shape
    /// lands where the pilot configured it is this one's.
    #[test]
    fn a_notch_draws_its_null_at_the_centre_the_pilot_set() {
        let (OverlayShape::Response(response), Some(_)) = notch_shape(&notch(200.0, 100.0), 8000.0)
        else {
            panic!("a notch with a usable cutoff has a response and a stage");
        };

        let (null, gain) = response.deepest().expect("a curve was drawn");
        assert!((null - 200.0).abs() < 5.0, "null at {null:.0} Hz");
        assert!(gain < -20.0, "the null is only {gain:.1} dB");
    }

    /// A narrower notch — cutoff nearer the centre — starts taking later.
    #[test]
    fn a_higher_q_notch_is_narrower() {
        let near_edge = |cutoff| {
            let (OverlayShape::Response(response), _) = notch_shape(&notch(200.0, cutoff), 8000.0)
            else {
                panic!("expected a response");
            };
            response.corner().expect("a near edge").0
        };

        assert!(near_edge(180.0) > near_edge(100.0));
    }

    /// A cutoff at or above the centre is not a notch we can size. It stays a
    /// line rather than becoming a curve of invented depth — and a shape we
    /// cannot derive carries no stage, so it takes no part in the chain total.
    #[test]
    fn a_notch_without_a_usable_cutoff_stays_a_line() {
        for cutoff in [0.0, 300.0] {
            assert_eq!(
                notch_shape(&notch(200.0, cutoff), 8000.0),
                (OverlayShape::Line { hz: 200.0 }, None)
            );
        }
        assert!(
            notches(&[notch(200.0, 0.0)], FilterLoop::Gyro, &grid())[0]
                .gain
                .is_none()
        );
    }

    /// A static notch's gain lands on the spectrum's own bins, so the panel's
    /// chain total is a product over the raw trace's own points.
    #[test]
    fn a_static_notch_carries_its_gain_on_the_spectrums_grid() {
        let overlays = notches(&[notch(300.0, 280.0)], FilterLoop::Gyro, &grid());
        let Some(ByAxis::Shared(gain)) = &overlays[0].gain else {
            panic!("a sizeable notch carries a shared gain");
        };

        assert_eq!(gain.len(), grid().freq_hz.len());
        // The bin at the centre is nulled; the ones at either end are not.
        assert!(gain[2] < 0.1, "{gain:?}");
        assert!(gain[0] > 0.9 && gain[5] > 0.9, "{gain:?}");
    }

    fn dynamic_lpf() -> LowpassConfig {
        LowpassConfig {
            static_hz: 0.0,
            dyn_min_hz: 250.0,
            dyn_max_hz: 500.0,
            dyn_expo: 0.0,
            filter_type: FilterType::Pt1,
        }
    }

    /// Throttle held at the bottom of its range keeps the cutoff at the bottom
    /// of the dynamic range, and the corner drawn is that one — not the
    /// ceiling the old marker collapsed to, nor the whole range as a band.
    #[test]
    fn a_dynamic_lowpass_is_the_corner_the_throttle_actually_held_it_at() {
        let idle = vec![1000.0; 64];
        let Some(OverlayShape::Response(response)) =
            lowpass_shape(&dynamic_lpf(), Some(&idle), &grid()).map(|l| l.shape)
        else {
            panic!("a throttle trace gives the swept response");
        };

        let (corner, _) = response.corner().expect("a corner");
        assert!((corner - 250.0).abs() < 20.0, "corner at {corner:.0} Hz");
    }

    /// A flight worked across the throttle range swept the corner across the
    /// dynamic range, so the average rolls off later than the bottom of it.
    #[test]
    fn a_swept_lowpass_averages_the_corners_it_passed_through() {
        let held = vec![1000.0; 64];
        let swept: Vec<f64> = (0..64).map(|i| 1000.0 + i as f64 * 1000.0 / 63.0).collect();

        let corner = |throttle: &[f64]| {
            let Some(OverlayShape::Response(r)) =
                lowpass_shape(&dynamic_lpf(), Some(throttle), &grid()).map(|l| l.shape)
            else {
                panic!("expected a response");
            };
            r.corner().expect("a corner").0
        };

        assert!(corner(&swept) > corner(&held) + 20.0);
    }

    /// Without throttle there is no sweep to weight, and the configured range
    /// is all that can honestly be claimed — as two real rolloffs at its two
    /// ends, never as a span saying everything between them is gone. The label
    /// says `config`, so a configured range is never read as a measured one,
    /// and there is no gain to cascade from a sweep nothing weighted.
    #[test]
    fn a_dynamic_lowpass_without_throttle_draws_the_two_ends_it_ran_between() {
        let drawn = lowpass_shape(&dynamic_lpf(), None, &grid()).expect("a configured range");

        let OverlayShape::Envelope { low, high } = &drawn.shape else {
            panic!("no throttle gives an envelope, not a span");
        };
        assert!(low.corner().unwrap().0 < high.corner().unwrap().0);
        assert_eq!(drawn.suffix, " (dyn, 250–500 Hz, config)");
        assert!(drawn.settings.is_none() && drawn.dwell.is_none());
    }

    /// The name carries the range the stage really used, not the range it was
    /// allowed: a flight held near idle never took the corner anywhere near
    /// the configured maximum, and a label saying otherwise describes a filter
    /// that was not running.
    #[test]
    fn a_dynamic_lowpass_is_named_by_the_range_it_really_used() {
        let idle: Vec<f64> = vec![1050.0; 64];
        let drawn = lowpass_shape(&dynamic_lpf(), Some(&idle), &grid()).expect("a swept response");

        assert!(
            drawn.suffix.starts_with(" (dyn, 25"),
            "held at idle, the label reads {:?}",
            drawn.suffix
        );
        assert!(!drawn.suffix.contains("config"));
        assert!(drawn.dwell.is_some(), "the dwell is kept, not dropped");

        let worked: Vec<f64> = (0..64).map(|i| 1000.0 + i as f64 * 1000.0 / 63.0).collect();
        let swept = lowpass_shape(&dynamic_lpf(), Some(&worked), &grid()).expect("a response");
        assert_ne!(swept.suffix, drawn.suffix);
    }

    /// The label is the stage's name plus what it did, and a static stage adds
    /// nothing to its own.
    #[test]
    fn a_static_stage_is_named_by_itself_alone() {
        let cfg = FilterConfig {
            gyro_lpf1: Some(LowpassConfig {
                static_hz: 200.0,
                ..dynamic_lpf()
            }),
            ..Default::default()
        };

        assert_eq!(lowpasses(&cfg, None, &grid())[0].label, "Gyro LPF1");
    }

    /// A static stage never swept, so the throttle is beside the point.
    #[test]
    fn a_static_lowpass_draws_one_rolloff_whatever_the_throttle_did() {
        let static_lpf = LowpassConfig {
            static_hz: 300.0,
            ..dynamic_lpf()
        };
        let Some(OverlayShape::Response(response)) =
            lowpass_shape(&static_lpf, None, &grid()).map(|l| l.shape)
        else {
            panic!("a static lowpass has a response without any flight data");
        };

        // A little under the nominal 300: the discrete one-pole Betaflight
        // runs rolls off slightly early, and more so the closer its cutoff
        // gets to the loop rate.
        let (corner, _) = response.corner().expect("a corner");
        assert!(
            (255.0..=305.0).contains(&corner),
            "corner at {corner:.0} Hz"
        );
    }

    fn rpm_filter(weights: Vec<f32>) -> RpmFilterConfig {
        RpmFilterConfig {
            harmonics: weights.len() as u32,
            min_hz: 100.0,
            fade_range_hz: 50.0,
            q: 5.0,
            weights,
        }
    }

    #[test]
    fn a_zero_weight_harmonic_is_tracked_but_not_filtered() {
        let cfg = rpm_filter(vec![1.0, 0.0, 0.8]);

        assert!(is_filtered(Some(&cfg), 1));
        assert!(!is_filtered(Some(&cfg), 2));
        assert!(is_filtered(Some(&cfg), 3));
    }

    /// Without an RPM filter nothing is attenuating the harmonics, and the
    /// bands must not claim otherwise.
    #[test]
    fn without_an_rpm_filter_no_harmonic_is_filtered() {
        assert!(!is_filtered(None, 1));
    }

    /// One motor, 4000 eRPM steady, 14 poles: 952 Hz fundamental, and the
    /// third harmonic three times that.
    #[test]
    fn harmonic_bands_are_multiples_of_the_motors_own_range() {
        let fd = FlightData::default()
            .with_time(vec![0, 1000, 2000])
            .with_channel(Channel::Rpm(0), vec![0.0, 4000.0, 4000.0]);
        let metadata = Metadata {
            filters: FilterConfig {
                rpm_filter: Some(rpm_filter(vec![1.0, 1.0, 1.0])),
                ..Default::default()
            },
            ..Default::default()
        };

        let OverlayShape::Harmonics(bands) = harmonics(&fd.trimmed(0.0), &metadata).unwrap().shape
        else {
            panic!("harmonics are a harmonic group");
        };

        assert_eq!(bands.len(), 3);
        assert!((bands[0].low_hz - 952.38).abs() < 0.01, "{:?}", bands[0]);
        assert!(
            (bands[2].high_hz - 3.0 * 952.38).abs() < 0.05,
            "{:?}",
            bands[2]
        );
    }

    /// The stopped-motor samples at the head of the log are not a frequency
    /// the craft flew, and a band running from 0 Hz says nothing.
    #[test]
    fn a_stopped_motor_does_not_stretch_a_band_to_zero() {
        assert_eq!(
            spinning_span(&[0.0, 2000.0, 3000.0]),
            Some((2000.0, 3000.0))
        );
        assert_eq!(spinning_span(&[0.0, 0.0]), None);
    }

    /// The change the narrower band exists for: a motor held at one frequency
    /// for almost the whole window, with a brief blip either side, must draw
    /// the frequency it held — not the two frequencies it touched once.
    #[test]
    fn a_brief_excursion_does_not_widen_the_band() {
        let mut samples = vec![4000.0; 100];
        samples[0] = 500.0;
        samples[99] = 20000.0;

        let (low, high) = spinning_span(&samples).expect("the motor was running");
        assert_eq!((low, high), (4000.0, 4000.0));
    }

    /// A motor worked across its range keeps a band, and it is inside the
    /// extremes rather than equal to them.
    #[test]
    fn a_worked_motor_keeps_a_band_inside_its_extremes() {
        let samples: Vec<f64> = (1..=100).map(|i| i as f64 * 100.0).collect();

        let (low, high) = spinning_span(&samples).expect("the motor was running");
        assert!(low > 100.0 && high < 10000.0, "{low}..{high}");
        assert!(high > low);
    }

    /// Betaflight filters three harmonics at most. A header claiming five is a
    /// header, not a capability, and a fourth identity for an order nothing
    /// can filter is a claim the plot must not make.
    #[test]
    fn more_harmonics_than_betaflight_can_filter_are_clamped_to_three() {
        let fd = FlightData::default()
            .with_time(vec![0, 1000, 2000])
            .with_channel(Channel::Rpm(0), vec![4000.0, 4000.0, 4000.0]);
        let metadata = Metadata {
            filters: FilterConfig {
                rpm_filter: Some(rpm_filter(vec![1.0; 5])),
                ..Default::default()
            },
            ..Default::default()
        };

        let OverlayShape::Harmonics(bands) = harmonics(&fd.trimmed(0.0), &metadata).unwrap().shape
        else {
            panic!("harmonics are a harmonic group");
        };

        assert_eq!(bands.len(), 3, "{bands:?}");
        assert_eq!(bands.iter().map(|b| b.order).max(), Some(3));
    }

    /// Without eRPM there is nothing to draw, which is what greys the menu
    /// entry out rather than drawing bands at zero.
    #[test]
    fn no_erpm_means_no_harmonics_overlay() {
        let fd = FlightData::default().with_time(vec![0, 1000]);
        assert!(harmonics(&fd.trimmed(0.0), &Metadata::default()).is_none());
    }

    fn dyn_notch_metadata(debug_mode: &str) -> Metadata {
        Metadata {
            debug_mode: debug_mode.to_string(),
            filters: FilterConfig {
                dyn_notch: Some(DynNotchConfig {
                    min_hz: 100.0,
                    max_hz: 500.0,
                    count: 1,
                    q: 5.0,
                }),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// 8 kHz, the loop rate these filters actually run at — at 1 kHz a notch
    /// in the hundreds of hertz sits at Nyquist, where a biquad degenerates.
    fn traced_log(centres: Vec<f64>) -> FlightData {
        FlightData::default()
            .with_time((0..centres.len() as u64).map(|i| i * 125).collect())
            .with_channel(Channel::Debug(0), centres)
    }

    fn traced_of(centres: Vec<f64>) -> FilterResponse {
        let overlays = dyn_notch(
            &traced_log(centres).trimmed(0.0),
            &dyn_notch_metadata("FFT_FREQ"),
            &grid(),
        );
        let OverlayShape::Traced(per_axis) = &overlays[1].shape else {
            panic!("the second dyn notch overlay is the response");
        };
        per_axis[Axis::Roll].clone().expect("roll was traced")
    }

    /// The frequency the curve cuts hardest at, and by how much.
    fn deepest(traced: &FilterResponse) -> (f64, f64) {
        traced
            .freq_hz
            .iter()
            .zip(&traced.gain_db)
            .min_by(|a, b| a.1.total_cmp(b.1))
            .map(|(&f, &g)| (f, g))
            .expect("a curve was drawn")
    }

    /// The point of the whole overlay: a tracker that sat still cut one
    /// frequency hard, and the curve has to show that as a deep narrow V —
    /// not the flat band across the whole configured range that a pilot reads
    /// as "everything in here is gone".
    #[test]
    fn a_tracker_that_sat_still_draws_a_deep_narrow_notch() {
        let traced = traced_of(vec![250.0; 16]);
        let (freq, gain) = deepest(&traced);

        assert!((freq - 250.0).abs() < 10.0, "deepest at {freq:.0} Hz");
        assert!(gain < -20.0, "a still tracker only cut {gain:.1} dB");

        // Narrow: a hundred hertz off the centre the notch took almost nothing.
        let away = traced
            .freq_hz
            .iter()
            .zip(&traced.gain_db)
            .find(|&(&f, _)| f > 350.0)
            .map(|(_, &g)| g)
            .expect("the curve reaches past the notch");
        assert!(away > -1.0, "{away:.1} dB a hundred hertz off centre");
    }

    /// The diagnosis the average exists for: the same flight spent spread
    /// across the range cuts every frequency less hard than sitting on one,
    /// because no frequency ever got the full notch.
    #[test]
    fn a_roaming_tracker_cuts_less_deeply_than_a_still_one() {
        let still = deepest(&traced_of(vec![250.0; 16])).1;
        let roaming = deepest(&traced_of(vec![
            120.0, 170.0, 220.0, 270.0, 320.0, 370.0, 420.0, 470.0, 120.0, 170.0, 220.0, 270.0,
            320.0, 370.0, 420.0, 470.0,
        ]))
        .1;

        assert!(
            roaming > still + 10.0,
            "roaming cut {roaming:.1} dB against a still tracker's {still:.1} dB"
        );
    }

    /// Nothing is ever amplified, and nothing falls through the floor a plot
    /// axis can draw.
    #[test]
    fn the_response_stays_between_the_floor_and_no_cut_at_all() {
        let traced = traced_of(vec![250.0; 16]);

        assert!(
            traced
                .gain_db
                .iter()
                .all(|&g| (MIN_GAIN_DB..=0.0).contains(&g))
        );
        assert_eq!(traced.freq_hz.len(), traced.gain_db.len());
    }

    /// The dwell histogram the average is taken over: a tracker pinned at the
    /// top of its range spends most of the flight in the last bin.
    #[test]
    fn the_dwell_histogram_is_a_density_over_the_configured_range() {
        let dwell =
            dwell_histogram(&[495.0, 495.0, 495.0, 200.0], 100.0, 500.0).expect("four samples");

        assert!((dwell.weight.iter().sum::<f64>() - 1.0).abs() < 1e-9);
        assert!(
            (dwell.weight[TRACE_BINS - 1] - 0.75).abs() < 1e-9,
            "{dwell:?}"
        );
    }

    /// Flown in another debug mode, `debug[0..3]` is something else entirely.
    /// The configured range is still drawn — the overlay degrades rather than
    /// disappearing.
    #[test]
    fn without_fft_freq_the_configured_range_survives_but_the_trace_does_not() {
        let overlays = dyn_notch(
            &traced_log(vec![250.0; 8]).trimmed(0.0),
            &dyn_notch_metadata("GYRO_SCALED"),
            &grid(),
        );

        assert_eq!(overlays.len(), 1);
        assert_eq!(
            overlays[0].shape,
            OverlayShape::Allowed {
                low_hz: 100.0,
                high_hz: 500.0
            }
        );
        // Bounds are not a cut: nothing here may reach the chain total.
        assert!(overlays[0].gain.is_none());
    }

    /// The tracked notch is the one overlay measured per axis, so both the gain
    /// the total is a product over and the dwell the lane draws are per axis
    /// too — roll's tracker and yaw's went to different places.
    #[test]
    fn the_traced_notch_carries_a_gain_and_a_dwell_per_axis() {
        let overlays = dyn_notch(
            &traced_log(vec![250.0; 16]).trimmed(0.0),
            &dyn_notch_metadata("FFT_FREQ"),
            &grid(),
        );
        let traced = &overlays[1];

        let gain = traced
            .gain
            .as_ref()
            .and_then(|g| g.get(Axis::Roll))
            .expect("roll was traced");
        assert_eq!(gain.len(), grid().freq_hz.len());
        // 250 Hz is between the 200 and 300 Hz bins, and a notch pinned there
        // takes a real bite out of both.
        assert!(gain[1] < 0.9 && gain[2] < 0.9, "{gain:?}");

        let dwell = traced
            .dwell
            .as_ref()
            .and_then(|d| d.get(Axis::Roll))
            .expect("roll has a dwell");
        assert!((dwell.weight.iter().sum::<f64>() - 1.0).abs() < 1e-9);
        assert_eq!(dwell.weight.iter().filter(|&&w| w > 0.0).count(), 1);

        assert!(traced.dwell.as_ref().unwrap().get(Axis::Pitch).is_none());
    }

    /// The percentiles the label is cut to: weights are time, so a filter that
    /// spent almost the whole window on one setting is named by that setting
    /// and not by the blip either side of it.
    #[test]
    fn the_realised_range_is_where_the_filter_spent_the_window() {
        let mut samples = vec![300.0; 100];
        samples[0] = 110.0;
        samples[99] = 490.0;
        let dwell = dwell_histogram(&samples, 100.0, 500.0).expect("a hundred samples");

        let (low, high) = dwell.realised_range().expect("a range");
        assert!((low - high).abs() < 20.0, "{low}..{high}");
        assert!((250.0..350.0).contains(&low), "{low}..{high}");
    }

    /// The count Betaflight logs is one centre per axis however many notches
    /// were configured, and the label says so rather than implying three.
    #[test]
    fn more_than_one_configured_notch_is_named_as_such() {
        let mut metadata = dyn_notch_metadata("NONE");
        metadata.filters.dyn_notch.as_mut().unwrap().count = 3;

        let overlays = dyn_notch(&traced_log(vec![250.0; 8]).trimmed(0.0), &metadata, &grid());
        assert_eq!(overlays[0].label, "Dyn notch range (×3)");
    }

    #[test]
    fn a_static_lowpass_carries_its_loop_in_the_family() {
        let cfg = FilterConfig {
            gyro_lpf2: Some(StaticLowpassConfig {
                cutoff_hz: 500.0,
                filter_type: FilterType::Pt1,
            }),
            ..Default::default()
        };

        let overlays = lowpasses(&cfg, None, &grid());
        assert_eq!(overlays.len(), 1);
        assert_eq!(overlays[0].family, OverlayFamily::Lowpass(FilterLoop::Gyro));
    }
}
