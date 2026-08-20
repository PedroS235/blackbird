use super::overlays::{self, FilterOverlay};
use crate::parser::metadata::DynNotchConfig;
use crate::parser::{Axis, FlightData, Metadata, PerAxis};
use crate::signal::fft::{self, BinnedSpectrum, Psd, SignalAnalyzer, Spectrum};

/// Runs PSD/peak/harmonic analysis on a gyro axis against its Betaflight
/// filter config. Holds the detection thresholds so they're tunable per call
/// site (e.g. a lower `peak_min_above_floor_db` for a noisier craft) instead
/// of being buried as free-function constants.
#[derive(Debug, Clone)]
pub struct GyroNoiseAnalyzer {
    pub throttle_bins: usize,
    /// Cut off each end of the log. Props spinning on the ground resonate
    /// through whatever the craft is sitting on, and that is not flight noise.
    pub trim_s: f64,
    pub time_bins: usize,
    /// Peaks below this aren't reported — flight dynamics/stick input, not motor/prop noise.
    pub peak_search_min_hz: f64,
    /// Minimum prominence above the noise floor to count as a peak.
    pub peak_min_above_floor_db: f64,
    pub max_peaks: usize,
    /// Tolerance (fraction of the harmonic index) for grouping a peak under a fundamental.
    pub harmonic_tolerance: f64,
}

impl Default for GyroNoiseAnalyzer {
    fn default() -> Self {
        Self {
            throttle_bins: 10,
            trim_s: super::DEFAULT_TRIM_S,
            time_bins: 60,
            peak_search_min_hz: 30.0,
            peak_min_above_floor_db: 6.0,
            max_peaks: 8,
            harmonic_tolerance: 0.05,
        }
    }
}

/// A local maximum in the raw PSD — candidate motor/prop noise.
#[derive(Debug, Clone)]
pub struct FrequencyPeak {
    pub freq_hz: f64,
    pub amplitude_db: f64,
    /// Index into the same `peaks` vec of the fundamental this is a multiple of.
    pub harmonic_of: Option<usize>,
    /// raw − filtered amplitude at this frequency; how much the current filter
    /// config is already knocking it down. `None` if there's no filtered signal.
    pub attenuated_db: Option<f64>,
    /// Whether the dynamic notch could ever have reached this peak. `None`
    /// when no dynamic notch was configured, which is not the same as a peak
    /// the tracker chose to ignore.
    pub dyn_notch_reach: Option<DynNotchReach>,
}

/// Where a peak sits against the dynamic notch's configured range. Decided
/// here rather than in the panel: it is a claim about the filter config, and
/// the prose count under the plot and the recoloured line have to agree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DynNotchReach {
    Inside,
    BelowMin,
    AboveMax,
}

impl DynNotchReach {
    fn of(freq_hz: f64, cfg: &DynNotchConfig) -> Self {
        match freq_hz {
            f if f < cfg.min_hz as f64 => Self::BelowMin,
            f if f > cfg.max_hz as f64 => Self::AboveMax,
            _ => Self::Inside,
        }
    }

    pub fn is_outside(self) -> bool {
        self != Self::Inside
    }
}

#[derive(Debug, Clone)]
pub struct AxisSpectral {
    pub raw_psd: Psd,
    pub filtered_psd: Option<Psd>,
    pub raw_spectrum: Spectrum,
    pub filtered_spectrum: Option<Spectrum>,
    pub throttle_map: Option<BinnedSpectrum>,
    /// Raw-signal power binned by time instead of throttle — a spectrogram.
    pub time_map: Option<BinnedSpectrum>,
    pub peaks: Vec<FrequencyPeak>,
    pub noise_floor_db: f64,
}

#[derive(Debug, Clone, Default)]
pub struct SpectralAnalysis {
    axes: PerAxis<Option<AxisSpectral>>,
    /// Shared across axes — filter config is global, and the one overlay that
    /// is measured per axis carries its own three.
    pub overlays: Vec<FilterOverlay>,
}

impl SpectralAnalysis {
    /// `None` when the axis had no pre-filter gyro to analyse.
    pub fn axis(&self, axis: Axis) -> Option<&AxisSpectral> {
        self.axes[axis].as_ref()
    }
}

impl AxisSpectral {
    /// How many peaks sit where the dynamic notch cannot reach them. Counted
    /// here rather than in the panel so the recoloured line and the prose
    /// under the plot are the same claim.
    pub fn peaks_reaching(&self, reach: DynNotchReach) -> usize {
        self.peaks
            .iter()
            .filter(|p| p.dyn_notch_reach == Some(reach))
            .count()
    }
}

impl GyroNoiseAnalyzer {
    pub fn analyze(&self, fd: &FlightData, metadata: &Metadata) -> SpectralAnalysis {
        let fd = fd.trimmed(self.trim_s);
        let fs = fd.sample_rate_hz();
        let throttle = fd.throttle();
        let time_ref = fd.time_s();

        let dyn_notch = metadata.filters.dyn_notch.as_ref();
        let axes = PerAxis(Axis::ALL.map(|axis| {
            fd.gyro_raw(axis).map(|raw| {
                self.analyze_axis(raw, fd.gyro(axis), throttle, &time_ref, fs, dyn_notch)
            })
        }));

        SpectralAnalysis {
            axes,
            overlays: overlays::build(&fd, metadata),
        }
    }

    fn analyze_axis(
        &self,
        raw: &[f64],
        filtered: Option<&[f64]>,
        throttle: Option<&[f64]>,
        time_ref: &[f64],
        fs: f64,
        dyn_notch: Option<&DynNotchConfig>,
    ) -> AxisSpectral {
        let window = fft::window_size_for(fs, raw.len());
        let analyzer = SignalAnalyzer::new(fs, window, window / 2);

        // One chunked pass over each signal; PSD, magnitude and both maps are
        // derived from that pass's shared power array.
        let mut raw_pass = analyzer.pass(raw);
        if let Some(t) = throttle {
            raw_pass = raw_pass.binned_by(t, self.throttle_bins);
        }
        if !time_ref.is_empty() {
            raw_pass = raw_pass.binned_by(time_ref, self.time_bins);
        }
        let raw_view = raw_pass.run();
        let filtered_view = filtered.map(|f| analyzer.pass(f).run());

        let raw_psd = raw_view.psd();
        let raw_spectrum = raw_view.magnitude();
        let filtered_psd = filtered_view.as_ref().map(|v| v.psd());
        let filtered_spectrum = filtered_view.as_ref().map(|v| v.magnitude());

        // Same order the references were registered in above.
        let mut maps = raw_view.into_binned().into_iter();
        let throttle_map = throttle.and_then(|_| maps.next());
        let time_map = maps.next();

        let noise_floor_db = median(&raw_psd.power_db);
        let peaks = self.find_peaks(&raw_psd, filtered_psd.as_ref(), noise_floor_db, dyn_notch);

        AxisSpectral {
            raw_psd,
            filtered_psd,
            raw_spectrum,
            filtered_spectrum,
            throttle_map,
            time_map,
            peaks,
            noise_floor_db,
        }
    }

    /// Top local maxima in the raw PSD above the noise floor, grouped into
    /// harmonics of a lower-frequency fundamental where the ratio is near-integer.
    fn find_peaks(
        &self,
        raw_psd: &Psd,
        filtered_psd: Option<&Psd>,
        floor_db: f64,
        dyn_notch: Option<&DynNotchConfig>,
    ) -> Vec<FrequencyPeak> {
        let threshold = floor_db + self.peak_min_above_floor_db;
        let mag = &raw_psd.power_db;
        let freq = &raw_psd.freq_hz;

        let mut candidates: Vec<usize> = (1..mag.len().saturating_sub(1))
            .filter(|&k| freq[k] >= self.peak_search_min_hz)
            .filter(|&k| mag[k] > threshold && mag[k] > mag[k - 1] && mag[k] > mag[k + 1])
            .collect();

        candidates.sort_by(|&a, &b| mag[b].total_cmp(&mag[a]));
        candidates.truncate(self.max_peaks);
        candidates.sort_by(|&a, &b| freq[a].total_cmp(&freq[b]));

        let mut peaks: Vec<FrequencyPeak> = candidates
            .into_iter()
            .map(|k| FrequencyPeak {
                freq_hz: freq[k],
                amplitude_db: mag[k],
                harmonic_of: None,
                attenuated_db: filtered_psd
                    .filter(|fp| k < fp.power_db.len())
                    .map(|fp| mag[k] - fp.power_db[k]),
                dyn_notch_reach: dyn_notch.map(|cfg| DynNotchReach::of(freq[k], cfg)),
            })
            .collect();

        for i in 0..peaks.len() {
            let mut best: Option<(usize, f64)> = None;
            for j in 0..i {
                let ratio = peaks[i].freq_hz / peaks[j].freq_hz;
                let n = ratio.round();
                if n < 2.0 {
                    continue;
                }
                let err = (ratio - n).abs() / n;
                if err < self.harmonic_tolerance && best.is_none_or(|(_, best_err)| err < best_err)
                {
                    best = Some((j, err));
                }
            }
            peaks[i].harmonic_of = best.map(|(j, _)| j);
        }

        peaks
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::parser::{Axis, Channel};

    /// 200 Hz sine on roll's pre-filter gyro, nothing on pitch/yaw.
    fn one_axis_log(freq_hz: f64, fs: f64, samples: usize) -> FlightData {
        let dt_us = (1e6 / fs) as u64;
        let sine = (0..samples)
            .map(|i| (std::f64::consts::TAU * freq_hz * i as f64 / fs).sin() * 50.0)
            .collect();
        FlightData::default()
            .with_time((0..samples as u64).map(|i| 7_000_000 + i * dt_us).collect())
            .with_channel(Channel::RawGyro(Axis::Roll), sine)
    }

    #[test]
    fn analyze_reports_the_injected_noise_frequency() {
        let analysis = GyroNoiseAnalyzer::default()
            .analyze(&one_axis_log(200.0, 2000.0, 8192), &Metadata::default());

        let roll = analysis.axis(Axis::Roll).expect("roll analysed");
        let loudest = roll
            .peaks
            .iter()
            .max_by(|a, b| a.amplitude_db.total_cmp(&b.amplitude_db))
            .expect("a peak was found");

        assert!(
            (loudest.freq_hz - 200.0).abs() < 10.0,
            "expected peak near 200 Hz, got {} Hz",
            loudest.freq_hz
        );
    }

    /// Props spinning on the ground before the launch ring the frame at a
    /// frequency the craft never saw in the air. Reporting it sends the pilot
    /// notching a peak that is not there.
    #[test]
    fn noise_from_the_ends_of_the_log_is_not_reported_as_flight_noise() {
        const FS: f64 = 2000.0;
        let mut log = one_axis_log(200.0, FS, 40_000);
        let ends = 2 * FS as usize;
        let ground: Vec<f64> = (0..40_000)
            .map(|i| (std::f64::consts::TAU * 350.0 * i as f64 / FS).sin() * 200.0)
            .collect();

        // The same log, but sitting on the ground for its first and last two
        // seconds.
        let mut raw = log.gyro_raw(Axis::Roll).unwrap().to_vec();
        raw[..ends].copy_from_slice(&ground[..ends]);
        let tail = raw.len() - ends;
        raw[tail..].copy_from_slice(&ground[tail..]);
        log = log.with_channel(Channel::RawGyro(Axis::Roll), raw);

        let peaks_near = |trim_s, hz: f64| {
            GyroNoiseAnalyzer {
                trim_s,
                ..Default::default()
            }
            .analyze(&log, &Metadata::default())
            .axis(Axis::Roll)
            .expect("roll analysed")
            .peaks
            .iter()
            .any(|p| (p.freq_hz - hz).abs() < 10.0)
        };

        assert!(peaks_near(0.0, 350.0), "untrimmed misses the ground tone");
        assert!(!peaks_near(2.0, 350.0), "the ground tone survived trimming");
        assert!(peaks_near(2.0, 200.0), "the flight tone was trimmed away");
    }

    #[test]
    fn axes_without_raw_gyro_are_not_analysed() {
        let analysis = GyroNoiseAnalyzer::default()
            .analyze(&one_axis_log(200.0, 2000.0, 2048), &Metadata::default());

        assert!(analysis.axis(Axis::Pitch).is_none());
        assert!(analysis.axis(Axis::Yaw).is_none());
    }

    /// A peak the dynamic notch can never reach is the panel's warning and
    /// its prose count, so the classification is made here once.
    #[test]
    fn peaks_are_classified_against_the_dynamic_notch_range() {
        let metadata = Metadata {
            filters: crate::parser::metadata::FilterConfig {
                dyn_notch: Some(crate::parser::metadata::DynNotchConfig {
                    min_hz: 100.0,
                    max_hz: 150.0,
                    count: 1,
                    q: 5.0,
                }),
                ..Default::default()
            },
            ..Default::default()
        };

        let roll = GyroNoiseAnalyzer::default()
            .analyze(&one_axis_log(200.0, 2000.0, 8192), &metadata)
            .axis(Axis::Roll)
            .expect("roll analysed")
            .clone();

        let loudest = roll
            .peaks
            .iter()
            .max_by(|a, b| a.amplitude_db.total_cmp(&b.amplitude_db))
            .expect("a peak was found");

        assert_eq!(loudest.dyn_notch_reach, Some(DynNotchReach::AboveMax));
        assert!(roll.peaks_reaching(DynNotchReach::AboveMax) > 0);
    }

    /// No dynamic notch configured is not the same claim as a peak the
    /// tracker chose not to reach, and must not be counted as one.
    #[test]
    fn without_a_dynamic_notch_no_peak_is_out_of_range() {
        let roll = GyroNoiseAnalyzer::default()
            .analyze(&one_axis_log(200.0, 2000.0, 8192), &Metadata::default())
            .axis(Axis::Roll)
            .expect("roll analysed")
            .clone();

        assert!(roll.peaks.iter().all(|p| p.dyn_notch_reach.is_none()));
        assert_eq!(roll.peaks_reaching(DynNotchReach::AboveMax), 0);
    }

    #[test]
    fn throttle_map_absent_when_throttle_not_logged() {
        let analysis = GyroNoiseAnalyzer::default()
            .analyze(&one_axis_log(200.0, 2000.0, 2048), &Metadata::default());

        assert!(analysis.axis(Axis::Roll).unwrap().throttle_map.is_none());
    }
}

fn median(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    sorted[sorted.len() / 2]
}
