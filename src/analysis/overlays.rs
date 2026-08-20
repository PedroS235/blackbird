//! What the filters actually occupy in frequency, computed once at load time.
//!
//! Replaces a line-shaped marker per filter. A notch has a bandwidth, a
//! dynamic filter has a range it swept, and the RPM filter has one band per
//! motor per harmonic — drawing any of them as a single line at a nominal
//! centre tells a pilot where a setting says the filter is, not where the
//! filter was.

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

/// What the dynamic notch actually took off, per frequency.
///
/// A notch is not a rectangle: its response is a V, deep at the centre and
/// recovering either side at a rate its Q sets. And a *dynamic* notch has no
/// one centre — it moved all flight, so no single V describes it either. This
/// is the notch's response at every centre the tracker used, averaged in
/// power over how long it spent at each.
///
/// So a tracker that sat still leaves a deep narrow V, and one that roamed
/// leaves a broad shallow trough — because no single frequency ever got the
/// full cut. That difference is the whole diagnosis, and a band drawn across
/// the configured range states neither.
#[derive(Debug, Clone, PartialEq)]
pub struct TracedResponse {
    pub freq_hz: Vec<f64>,
    /// Mean power gain in dB, at or below zero. Floored at
    /// [`MIN_GAIN_DB`] — the null of a notch is unbounded, and a plot cannot
    /// draw minus infinity.
    pub gain_db: Vec<f64>,
}

/// Where the tracker sat, as time spent per frequency. The intermediate the
/// response is averaged over, not an output: as a picture it says where the
/// notch was, and a pilot wants to know what it removed.
#[derive(Debug, Clone, PartialEq)]
struct Dwell {
    /// Bin centres, Hz.
    freq_hz: Vec<f64>,
    /// Fraction of the analysed window spent in each bin, summing to 1.
    weight: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OverlayShape {
    /// A filter with no width to draw — a fixed lowpass corner.
    Line { hz: f64 },
    /// A notch's bandwidth, or the range a dynamic cutoff moved through.
    Band { low_hz: f64, high_hz: f64 },
    /// One band per motor per harmonic order.
    Harmonics(Vec<HarmonicBand>),
    /// Measured, not configured. Per axis, because the tracker is.
    Traced(PerAxis<Option<TracedResponse>>),
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

/// Points the response curve is drawn from. A notch's null is narrow, so a
/// coarse grid would miss the bottom of the V and understate the cut.
const RESPONSE_POINTS: usize = 512;

/// The floor the response is clamped to. A notch's null is unbounded; a plot
/// axis is not, and 40 dB down is already "gone".
pub const MIN_GAIN_DB: f64 = -40.0;

/// Every overlay this log can support, over the same window the spectra were
/// measured on.
pub(super) fn build(fd: &Trimmed<'_>, metadata: &Metadata) -> Vec<FilterOverlay> {
    let cfg = &metadata.filters;
    let mut overlays = Vec::new();

    overlays.extend(harmonics(fd, metadata));
    overlays.extend(dyn_notch(fd, metadata));
    overlays.extend(notches(&cfg.gyro_notches, FilterLoop::Gyro));
    overlays.extend(notches(&cfg.dterm_notches, FilterLoop::Dterm));
    overlays.extend(lowpasses(cfg));
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
        let fs = fd.sample_rate_hz();
        let traced = PerAxis(Axis::ALL.map(|axis| {
            let dwell = fd
                .debug_axis(axis)
                .and_then(|s| dwell_histogram(s, low_hz, high_hz))?;
            traced_response(&dwell, cfg.q as f64, fs, low_hz, high_hz)
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

/// The notch's response at each centre the tracker used, averaged in power by
/// how long it sat there.
///
/// Averaged in power rather than in decibels because that is what the noise
/// does: a frequency notched hard for a tenth of the flight and untouched for
/// the rest kept nine tenths of its energy, which averaging the decibels would
/// report as a 10 dB cut it never got.
fn traced_response(
    dwell: &Dwell,
    q: f64,
    sample_rate_hz: f64,
    low_hz: f64,
    high_hz: f64,
) -> Option<TracedResponse> {
    if q <= 0.0 || sample_rate_hz <= 0.0 {
        return None;
    }
    // Past the range, out to where the skirts of the widest notch have
    // recovered — the V drawn cut off at the configured bound would look like
    // a wall the filter does not have.
    let nyquist = sample_rate_hz / 2.0;
    let pad = (high_hz / q).max(10.0);
    let (from, to) = ((low_hz - pad).max(1.0), (high_hz + pad).min(nyquist));
    if to <= from {
        return None;
    }

    let step = (to - from) / (RESPONSE_POINTS - 1) as f64;
    let centres: Vec<(f64, f64)> = dwell
        .freq_hz
        .iter()
        .zip(&dwell.weight)
        .filter(|&(_, &w)| w > 0.0)
        .map(|(&f, &w)| (f, w))
        .collect();

    let (freq_hz, gain_db) = (0..RESPONSE_POINTS)
        .map(|i| {
            let freq = from + i as f64 * step;
            let power: f64 = centres
                .iter()
                .map(|&(centre, weight)| weight * notch_power_gain(freq, centre, q, sample_rate_hz))
                .sum();

            (freq, (10.0 * power.log10()).clamp(MIN_GAIN_DB, 0.0))
        })
        .unzip();

    Some(TracedResponse { freq_hz, gain_db })
}

/// |H(f)|² of the biquad notch Betaflight runs, at `centre` with quality `q`.
///
/// The digital response, not the analogue approximation: the filter runs at
/// the gyro loop rate, and this is the shape it actually has there.
fn notch_power_gain(freq_hz: f64, centre_hz: f64, q: f64, sample_rate_hz: f64) -> f64 {
    use std::f64::consts::TAU;

    // RBJ cookbook notch: b = [1, -2cos w0, 1], a = [1 + α, -2cos w0, 1 - α].
    let w0 = TAU * centre_hz / sample_rate_hz;
    let alpha = w0.sin() / (2.0 * q);
    let (b1, a0, a2) = (-2.0 * w0.cos(), 1.0 + alpha, 1.0 - alpha);

    let w = TAU * freq_hz / sample_rate_hz;
    let (cos1, sin1, cos2, sin2) = (w.cos(), w.sin(), (2.0 * w).cos(), (2.0 * w).sin());

    let num = (1.0 + b1 * cos1 + cos2).powi(2) + (b1 * sin1 + sin2).powi(2);
    let den = (a0 + b1 * cos1 + a2 * cos2).powi(2) + (b1 * sin1 + a2 * sin2).powi(2);

    match den > 0.0 {
        true => num / den,
        false => 1.0,
    }
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

fn notches(configs: &[NotchConfig], loop_: FilterLoop) -> Vec<FilterOverlay> {
    configs
        .iter()
        .enumerate()
        .map(|(i, notch)| FilterOverlay {
            label: format!("{} notch {}", loop_.name(), i + 1),
            family: OverlayFamily::Notch(loop_),
            shape: notch_shape(notch),
        })
        .collect()
}

/// Betaflight derives a notch's Q from its centre and cutoff
/// (`filterGetNotchQ`); the −3 dB width is `centre / Q`, which is how much of
/// the spectrum the notch actually removes. A cutoff at or above the centre is
/// not a notch this can size, and stays a line.
fn notch_shape(notch: &NotchConfig) -> OverlayShape {
    let (centre, cutoff) = (notch.center_hz as f64, notch.cutoff_hz as f64);
    let q = match cutoff > 0.0 && centre > cutoff {
        true => centre * cutoff / (centre * centre - cutoff * cutoff),
        false => return OverlayShape::Line { hz: centre },
    };
    let half = centre / q / 2.0;

    OverlayShape::Band {
        low_hz: (centre - half).max(0.0),
        high_hz: centre + half,
    }
}

fn lowpasses(cfg: &FilterConfig) -> Vec<FilterOverlay> {
    let mut overlays = Vec::new();
    let mut push = |label: &str, loop_: FilterLoop, shape: OverlayShape| {
        overlays.push(FilterOverlay {
            label: label.to_string(),
            family: OverlayFamily::Lowpass(loop_),
            shape,
        });
    };

    if let Some(lpf) = &cfg.gyro_lpf1 {
        push("Gyro LPF1", FilterLoop::Gyro, lowpass_shape(lpf));
    }
    if let Some(lpf) = &cfg.gyro_lpf2 {
        push(
            "Gyro LPF2",
            FilterLoop::Gyro,
            OverlayShape::Line {
                hz: lpf.cutoff_hz as f64,
            },
        );
    }
    if let Some(lpf) = &cfg.dterm_lpf1 {
        push(
            "D-term LPF1",
            FilterLoop::Dterm,
            lowpass_shape(&lpf.lowpass),
        );
    }
    if let Some(lpf) = &cfg.dterm_lpf2 {
        push(
            "D-term LPF2",
            FilterLoop::Dterm,
            OverlayShape::Line {
                hz: lpf.cutoff_hz as f64,
            },
        );
    }
    overlays
}

/// A dynamic lowpass is the range its cutoff moved through, not the ceiling it
/// reached — the ceiling is one number for a filter that swept all flight.
fn lowpass_shape(lpf: &LowpassConfig) -> OverlayShape {
    match lpf.is_dynamic() {
        true => OverlayShape::Band {
            low_hz: lpf.dyn_min_hz as f64,
            high_hz: lpf.dyn_max_hz as f64,
        },
        false => OverlayShape::Line {
            hz: lpf.static_hz as f64,
        },
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::parser::FlightData;
    use crate::parser::metadata::{DynNotchConfig, FilterType, StaticLowpassConfig};

    fn notch(center_hz: f32, cutoff_hz: f32) -> NotchConfig {
        NotchConfig {
            center_hz,
            cutoff_hz,
        }
    }

    /// A 200 Hz notch cut off at 100 Hz has Q = 2/3, so it removes 300 Hz of
    /// spectrum centred on 200 — a band, not the line the panel used to draw.
    #[test]
    fn a_notch_is_as_wide_as_its_q_says() {
        let OverlayShape::Band { low_hz, high_hz } = notch_shape(&notch(200.0, 100.0)) else {
            panic!("a notch with a cutoff has a width");
        };

        assert!(
            (high_hz - low_hz - 300.0).abs() < 1e-6,
            "{low_hz}..{high_hz}"
        );
        assert!(((low_hz + high_hz) / 2.0 - 200.0).abs() < 1e-6);
    }

    /// A narrower notch — cutoff nearer the centre — takes less off.
    #[test]
    fn a_higher_q_notch_is_narrower() {
        let width = |cutoff| match notch_shape(&notch(200.0, cutoff)) {
            OverlayShape::Band { low_hz, high_hz } => high_hz - low_hz,
            _ => panic!("expected a band"),
        };

        assert!(width(180.0) < width(100.0));
    }

    /// A cutoff at or above the centre is not a notch we can size. It stays a
    /// line rather than becoming a band of invented width.
    #[test]
    fn a_notch_without_a_usable_cutoff_stays_a_line() {
        assert_eq!(
            notch_shape(&notch(200.0, 0.0)),
            OverlayShape::Line { hz: 200.0 }
        );
        assert_eq!(
            notch_shape(&notch(200.0, 300.0)),
            OverlayShape::Line { hz: 200.0 }
        );
    }

    /// The defect this replaced: a dynamic lowpass collapsed to `dyn_max_hz`,
    /// one line standing in for a cutoff that moved all flight.
    #[test]
    fn a_dynamic_lowpass_is_the_range_it_swept() {
        let dynamic = LowpassConfig {
            static_hz: 0.0,
            dyn_min_hz: 250.0,
            dyn_max_hz: 500.0,
            filter_type: FilterType::Pt1,
        };

        assert_eq!(
            lowpass_shape(&dynamic),
            OverlayShape::Band {
                low_hz: 250.0,
                high_hz: 500.0
            }
        );
        assert_eq!(
            lowpass_shape(&LowpassConfig {
                static_hz: 300.0,
                ..dynamic
            }),
            OverlayShape::Line { hz: 300.0 }
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

    fn traced_of(centres: Vec<f64>) -> TracedResponse {
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
    fn deepest(traced: &TracedResponse) -> (f64, f64) {
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

        let overlays = lowpasses(&cfg);
        assert_eq!(overlays.len(), 1);
        assert_eq!(overlays[0].family, OverlayFamily::Lowpass(FilterLoop::Gyro));
    }
}
