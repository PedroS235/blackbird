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
    pub const ALL: [Self; 6] = [
        Self::Harmonics,
        Self::DynNotch,
        Self::Notch(FilterLoop::Gyro),
        Self::Notch(FilterLoop::Dterm),
        Self::Lowpass(FilterLoop::Gyro),
        Self::Lowpass(FilterLoop::Dterm),
    ];
}

/// One motor's noise at one harmonic order, over the frequencies it actually
/// reached across the analysed window.
#[derive(Debug, Clone, PartialEq)]
pub struct HarmonicBand {
    pub motor: usize,
    /// 1 is the fundamental.
    pub order: u32,
    pub low_hz: f64,
    pub high_hz: f64,
    /// False where this order's RPM filter weight is zero — the filter tracks
    /// the harmonic but takes nothing off it, which is not the same as being
    /// filtered and has to look different.
    pub filtered: bool,
}

/// Where a filter that moved actually sat, as time spent per setting. The
/// intermediate a swept response is averaged over, not an output: as a picture
/// it says where the filter was, and a pilot wants to know what it removed.
#[derive(Debug, Clone, PartialEq)]
struct Dwell {
    /// Bin centres, Hz.
    freq_hz: Vec<f64>,
    /// Fraction of the analysed window spent in each bin, summing to 1.
    weight: Vec<f64>,
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
}

#[derive(Debug, Clone, PartialEq)]
pub enum OverlayShape {
    /// A filter whose shape we cannot derive — a notch with no usable cutoff.
    /// Everything we can size draws its real response instead.
    Line { hz: f64 },
    /// The range a filter is allowed to work in. Not what it removed — the
    /// dynamic notch's configured bounds, and nothing else.
    Band { low_hz: f64, high_hz: f64 },
    /// One band per motor per harmonic order.
    Harmonics(Vec<HarmonicBand>),
    /// What a filter took off, per frequency.
    Response(FilterResponse),
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
}

/// How many bins the traced centre is reduced to before the response is
/// averaged over them. Enough that a tracker sweeping its range averages to a
/// smooth trough, few enough to keep the averaging cheap.
const TRACE_BINS: usize = 64;

/// Throttle is logged on the stick scale, and the dynamic cutoff curve wants
/// a fraction.
const THROTTLE_MIN: f64 = 1000.0;
const THROTTLE_SPAN: f64 = 1000.0;

/// Every overlay this log can support, over the same window the spectra were
/// measured on.
pub(super) fn build(fd: &Trimmed<'_>, metadata: &Metadata) -> Vec<FilterOverlay> {
    let cfg = &metadata.filters;
    // The rate the filters ran at, not the rate the log was written at.
    let fs = metadata.filter_rate_hz(fd.sample_rate_hz());
    let mut overlays = Vec::new();

    overlays.extend(harmonics(fd, metadata));
    overlays.extend(dyn_notch(fd, metadata));
    overlays.extend(notches(&cfg.gyro_notches, FilterLoop::Gyro, fs));
    overlays.extend(notches(&cfg.dterm_notches, FilterLoop::Dterm, fs));
    overlays.extend(lowpasses(cfg, fd.throttle(), fs));
    overlays
}

/// A band per motor per harmonic order. The order count is the RPM filter's,
/// so the plot matches the Betaflight setting rather than a constant; without
/// an RPM filter only the fundamental is drawn, and nothing claims it is
/// attenuated.
fn harmonics(fd: &Trimmed<'_>, metadata: &Metadata) -> Option<FilterOverlay> {
    let rpm_filter = metadata.filters.rpm_filter.as_ref();
    let orders = rpm_filter.map_or(1, |r| r.harmonics).max(1);

    let bands: Vec<HarmonicBand> = (0..fd.rpm_count())
        .filter_map(|motor| {
            let (low, high) = spinning_extent(fd.channel(Channel::Rpm(motor))?)?;
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

    (!bands.is_empty()).then(|| FilterOverlay {
        label: "Motor harmonics".to_string(),
        family: OverlayFamily::Harmonics,
        shape: OverlayShape::Harmonics(bands),
    })
}

/// Samples where the motor is stopped are left out: a band running down to
/// zero describes a prop that was not turning, not a frequency the craft flew.
fn spinning_extent(samples: &[f64]) -> Option<(f64, f64)> {
    samples
        .iter()
        .filter(|&&v| v > 0.0)
        .fold(None, |acc: Option<(f64, f64)>, &v| match acc {
            Some((lo, hi)) => Some((lo.min(v), hi.max(v))),
            None => Some((v, v)),
        })
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
fn dyn_notch(fd: &Trimmed<'_>, metadata: &Metadata) -> Vec<FilterOverlay> {
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
        shape: OverlayShape::Band { low_hz, high_hz },
    }];

    if metadata.logs_dyn_notch_trace() {
        let fs = metadata.filter_rate_hz(fd.sample_rate_hz());
        let traced = PerAxis(Axis::ALL.map(|axis| {
            let dwell = fd
                .debug_axis(axis)
                .and_then(|s| dwell_histogram(s, low_hz, high_hz))?;
            let q = cfg.q as f64;
            let pad = (high_hz / q).max(10.0);
            filter_response::weighted(
                &dwell.stages(|centre_hz| Stage::Notch { centre_hz, q }),
                // Past the configured bounds, out to where the skirts have
                // recovered — a V cut off at a bound would look like a wall
                // the filter does not have.
                low_hz - pad,
                high_hz + pad,
                fs,
            )
        }));

        if traced.0.iter().any(Option::is_some) {
            overlays.push(FilterOverlay {
                // One notch, however many were configured: Betaflight logs one
                // centre per axis, so the others cannot be drawn and this does
                // not pretend they were.
                label: "Dyn notch response (traced)".to_string(),
                family: OverlayFamily::DynNotch,
                shape: OverlayShape::Traced(traced),
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

fn notches(configs: &[NotchConfig], loop_: FilterLoop, sample_rate_hz: f64) -> Vec<FilterOverlay> {
    configs
        .iter()
        .enumerate()
        .map(|(i, notch)| FilterOverlay {
            label: format!("{} notch {}", loop_.name(), i + 1),
            family: OverlayFamily::Notch(loop_),
            shape: notch_shape(notch, sample_rate_hz),
        })
        .collect()
}

/// Betaflight derives a notch's Q from its centre and cutoff
/// (`filterGetNotchQ`), and the V it cuts follows from the two. A cutoff at or
/// above the centre is not a notch this can size, and stays a bare line rather
/// than a curve of invented depth.
fn notch_shape(notch: &NotchConfig, sample_rate_hz: f64) -> OverlayShape {
    let (centre_hz, cutoff) = (notch.center_hz as f64, notch.cutoff_hz as f64);
    let line = OverlayShape::Line { hz: centre_hz };

    let q = match cutoff > 0.0 && centre_hz > cutoff {
        true => centre_hz * cutoff / (centre_hz * centre_hz - cutoff * cutoff),
        false => return line,
    };

    match filter_response::of(Stage::Notch { centre_hz, q }, sample_rate_hz) {
        Some(response) => OverlayShape::Response(response),
        None => line,
    }
}

/// Every lowpass stage, as the rolloff it is. A dynamic LPF1 is averaged over
/// the cutoffs the throttle actually took it to, the same way the dynamic
/// notch is averaged over the centres its tracker chose.
fn lowpasses(cfg: &FilterConfig, throttle: Option<&[f64]>, fs: f64) -> Vec<FilterOverlay> {
    let mut overlays = Vec::new();
    let mut push = |label: &str, loop_: FilterLoop, shape: Option<OverlayShape>| {
        if let Some(shape) = shape {
            overlays.push(FilterOverlay {
                label: label.to_string(),
                family: OverlayFamily::Lowpass(loop_),
                shape,
            });
        }
    };

    if let Some(lpf) = &cfg.gyro_lpf1 {
        push(
            "Gyro LPF1",
            FilterLoop::Gyro,
            lowpass_shape(lpf, throttle, fs),
        );
    }
    if let Some(lpf) = &cfg.gyro_lpf2 {
        push(
            "Gyro LPF2",
            FilterLoop::Gyro,
            static_lowpass_shape(lpf.cutoff_hz as f64, lpf.filter_type, fs),
        );
    }
    if let Some(lpf) = &cfg.dterm_lpf1 {
        push(
            "D-term LPF1",
            FilterLoop::Dterm,
            lowpass_shape(lpf, throttle, fs),
        );
    }
    if let Some(lpf) = &cfg.dterm_lpf2 {
        push(
            "D-term LPF2",
            FilterLoop::Dterm,
            static_lowpass_shape(lpf.cutoff_hz as f64, lpf.filter_type, fs),
        );
    }
    overlays
}

fn static_lowpass_shape(
    cutoff_hz: f64,
    filter_type: crate::parser::metadata::FilterType,
    fs: f64,
) -> Option<OverlayShape> {
    filter_response::of(
        Stage::Lowpass {
            cutoff_hz,
            filter_type,
        },
        fs,
    )
    .map(OverlayShape::Response)
}

/// A dynamic lowpass swept its corner all flight, so no one rolloff describes
/// it. Averaged over the cutoffs the throttle actually produced: a flight held
/// at one throttle draws that stage's own curve, and one worked across the
/// range draws the shallower, wider average of the corners it passed through.
///
/// Without throttle there is no sweep to weight, and the two ends of the
/// configured range are all that can honestly be claimed.
fn lowpass_shape(lpf: &LowpassConfig, throttle: Option<&[f64]>, fs: f64) -> Option<OverlayShape> {
    if !lpf.is_dynamic() {
        return static_lowpass_shape(lpf.static_hz as f64, lpf.filter_type, fs);
    }
    let (min, max) = (lpf.dyn_min_hz as f64, lpf.dyn_max_hz as f64);
    let Some(dwell) = throttle.and_then(|t| dwell_histogram(&cutoffs(lpf, t), min, max)) else {
        return Some(OverlayShape::Band {
            low_hz: min,
            high_hz: max,
        });
    };

    let filter_type = lpf.filter_type;
    filter_response::weighted(
        &dwell.stages(|cutoff_hz| Stage::Lowpass {
            cutoff_hz,
            filter_type,
        }),
        1.0,
        max * 8.0,
        fs,
    )
    .map(OverlayShape::Response)
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
        let OverlayShape::Response(response) = notch_shape(&notch(200.0, 100.0), 8000.0) else {
            panic!("a notch with a usable cutoff has a response");
        };

        let (null, gain) = response.deepest().expect("a curve was drawn");
        assert!((null - 200.0).abs() < 5.0, "null at {null:.0} Hz");
        assert!(gain < -20.0, "the null is only {gain:.1} dB");
    }

    /// A narrower notch — cutoff nearer the centre — starts taking later.
    #[test]
    fn a_higher_q_notch_is_narrower() {
        let near_edge = |cutoff| {
            let OverlayShape::Response(response) = notch_shape(&notch(200.0, cutoff), 8000.0)
            else {
                panic!("expected a response");
            };
            response.corner().expect("a near edge").0
        };

        assert!(near_edge(180.0) > near_edge(100.0));
    }

    /// A cutoff at or above the centre is not a notch we can size. It stays a
    /// line rather than becoming a curve of invented depth.
    #[test]
    fn a_notch_without_a_usable_cutoff_stays_a_line() {
        assert_eq!(
            notch_shape(&notch(200.0, 0.0), 8000.0),
            OverlayShape::Line { hz: 200.0 }
        );
        assert_eq!(
            notch_shape(&notch(200.0, 300.0), 8000.0),
            OverlayShape::Line { hz: 200.0 }
        );
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
            lowpass_shape(&dynamic_lpf(), Some(&idle), 8000.0)
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
                lowpass_shape(&dynamic_lpf(), Some(throttle), 8000.0)
            else {
                panic!("expected a response");
            };
            r.corner().expect("a corner").0
        };

        assert!(corner(&swept) > corner(&held) + 20.0);
    }

    /// Without throttle there is no sweep to weight, and the configured range
    /// is all that can honestly be claimed.
    #[test]
    fn a_dynamic_lowpass_without_throttle_falls_back_to_its_range() {
        assert_eq!(
            lowpass_shape(&dynamic_lpf(), None, 8000.0),
            Some(OverlayShape::Band {
                low_hz: 250.0,
                high_hz: 500.0
            })
        );
    }

    /// A static stage never swept, so the throttle is beside the point.
    #[test]
    fn a_static_lowpass_draws_one_rolloff_whatever_the_throttle_did() {
        let static_lpf = LowpassConfig {
            static_hz: 300.0,
            ..dynamic_lpf()
        };
        let Some(OverlayShape::Response(response)) = lowpass_shape(&static_lpf, None, 8000.0)
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
            spinning_extent(&[0.0, 2000.0, 3000.0]),
            Some((2000.0, 3000.0))
        );
        assert_eq!(spinning_extent(&[0.0, 0.0]), None);
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
        );

        assert_eq!(overlays.len(), 1);
        assert_eq!(
            overlays[0].shape,
            OverlayShape::Band {
                low_hz: 100.0,
                high_hz: 500.0
            }
        );
    }

    /// The count Betaflight logs is one centre per axis however many notches
    /// were configured, and the label says so rather than implying three.
    #[test]
    fn more_than_one_configured_notch_is_named_as_such() {
        let mut metadata = dyn_notch_metadata("NONE");
        metadata.filters.dyn_notch.as_mut().unwrap().count = 3;

        let overlays = dyn_notch(&traced_log(vec![250.0; 8]).trimmed(0.0), &metadata);
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

        let overlays = lowpasses(&cfg, None, 8000.0);
        assert_eq!(overlays.len(), 1);
        assert_eq!(overlays[0].family, OverlayFamily::Lowpass(FilterLoop::Gyro));
    }
}
