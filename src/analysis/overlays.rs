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

/// One section of the overlays menu. Every overlay belongs to exactly one, and
/// the pilot toggles the family rather than the individual line.
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

    /// Position in `ALL` — what a per-family array of visibility flags is
    /// indexed by, so a new family cannot be silently left out of the menu.
    pub fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|&f| f == self)
            .expect("every family is in ALL")
    }

    /// The menu entry's label.
    pub fn title(self) -> String {
        match self {
            Self::Harmonics => "Motor harmonics".to_string(),
            Self::DynNotch => "Dynamic notch".to_string(),
            Self::Notch(l) => format!("{} notches", l.name()),
            Self::Lowpass(l) => format!("{} lowpass", l.name()),
        }
    }

    /// Why the entry is greyed out. A control the log cannot fill says what
    /// the log is missing rather than vanishing.
    pub fn unavailable_reason(self) -> &'static str {
        match self {
            Self::Harmonics => {
                "This log has no eRPM. Motor harmonics are computed from the RPM the ESCs \
                 report back, which needs bidirectional DShot (`set dshot_bidir = ON`)."
            }
            Self::DynNotch => "No dynamic notch was configured on this flight.",
            Self::Notch(_) => "No static notch was enabled on this flight.",
            Self::Lowpass(_) => "No lowpass stage was configured on this flight.",
        }
    }
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

/// Where the firmware's dynamic notch tracker actually sat, as time spent per
/// frequency. A single number cannot say that a tracker was pinned at one end
/// of its range for half the flight; this can.
#[derive(Debug, Clone, PartialEq)]
pub struct TracedCenter {
    /// Bin centres, Hz.
    pub freq_hz: Vec<f64>,
    /// Fraction of the analysed window spent in each bin, summing to 1.
    pub weight: Vec<f64>,
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
    Traced(PerAxis<Option<TracedCenter>>),
}

/// A filter's geometry in the spectrum, with the family that toggles it.
#[derive(Debug, Clone, PartialEq)]
pub struct FilterOverlay {
    pub label: String,
    pub family: OverlayFamily,
    pub shape: OverlayShape,
}

/// How many bins the traced centre is reduced to. Enough that a tracker
/// sweeping its range reads as a sweep, few enough that a pinned one reads as
/// a single bar.
const TRACE_BINS: usize = 64;

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
/// `FFT_FREQ` — the centre the tracker actually chose, as a density.
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
        let traced = PerAxis(Axis::ALL.map(|axis| {
            fd.debug_axis(axis)
                .and_then(|s| histogram(s, low_hz, high_hz))
        }));
        if traced.0.iter().any(Option::is_some) {
            overlays.push(FilterOverlay {
                // Betaflight logs one centre per axis however many notches are
                // configured, and this says so rather than implying the rest.
                label: "Dyn notch centre (traced)".to_string(),
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
fn histogram(samples: &[f64], low_hz: f64, high_hz: f64) -> Option<TracedCenter> {
    let (low, high) = match high_hz > low_hz {
        true => (low_hz, high_hz),
        false => return None,
    };
    let width = (high - low) / TRACE_BINS as f64;

    let mut counts = vec![0.0; TRACE_BINS];
    let mut total = 0.0;
    for &v in samples.iter().filter(|v| v.is_finite() && **v > 0.0) {
        let bin = (((v - low) / width) as isize).clamp(0, TRACE_BINS as isize - 1) as usize;
        counts[bin] += 1.0;
        total += 1.0;
    }
    if total == 0.0 {
        return None;
    }

    Some(TracedCenter {
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

    fn traced_log() -> FlightData {
        FlightData::default()
            .with_time(vec![0, 1000, 2000, 3000])
            .with_channel(Channel::Debug(0), vec![495.0, 495.0, 495.0, 200.0])
    }

    /// A tracker pinned at the top of its range spends most of the flight in
    /// the last bin, which is the fault this overlay exists to show.
    #[test]
    fn the_traced_centre_is_a_density_over_the_configured_range() {
        let overlays = dyn_notch(&traced_log().trimmed(0.0), &dyn_notch_metadata("FFT_FREQ"));
        let OverlayShape::Traced(per_axis) = &overlays[1].shape else {
            panic!("the second dyn notch overlay is the trace");
        };

        let roll = per_axis[Axis::Roll].as_ref().expect("roll was traced");
        assert!((roll.weight.iter().sum::<f64>() - 1.0).abs() < 1e-9);
        assert!(
            (roll.weight[TRACE_BINS - 1] - 0.75).abs() < 1e-9,
            "{roll:?}"
        );
    }

    /// Flown in another debug mode, `debug[0..3]` is something else entirely.
    /// The configured range is still drawn — the overlay degrades rather than
    /// disappearing.
    #[test]
    fn without_fft_freq_the_configured_range_survives_but_the_trace_does_not() {
        let overlays = dyn_notch(
            &traced_log().trimmed(0.0),
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

        let overlays = dyn_notch(&traced_log().trimmed(0.0), &metadata);
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
