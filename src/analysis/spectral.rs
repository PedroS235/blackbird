use crate::parser::metadata::FilterConfig;
use crate::parser::{FlightData, Metadata};
use crate::signal::fft::{self, BinnedSpectrum, Psd, SignalAnalyzer, Spectrum};

/// Runs PSD/peak/harmonic analysis on a gyro axis against its Betaflight
/// filter config. Holds the detection thresholds so they're tunable per call
/// site (e.g. a lower `peak_min_above_floor_db` for a noisier craft) instead
/// of being buried as free-function constants.
#[derive(Debug, Clone)]
pub struct GyroNoiseAnalyzer {
    pub throttle_bins: usize,
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
}

/// A configured filter's position in the spectrum, for drawing over the PSD
/// and for explaining "why" a peak looks the way it does.
#[derive(Debug, Clone)]
pub struct FilterMarker {
    pub label: String,
    pub center_hz: f32,
    pub cutoff_hz: Option<f32>,
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
    pub axes: [Option<AxisSpectral>; 3],
    /// Shared across axes — filter config is global, not per-axis.
    pub filter_markers: Vec<FilterMarker>,
}

impl GyroNoiseAnalyzer {
    pub fn analyze(&self, fd: &FlightData, metadata: &Metadata) -> SpectralAnalysis {
        let fs = fd.sample_rate_hz();
        let throttle = fd.throttle();
        let time_ref = fd.time_s();

        let axes = std::array::from_fn(|i| {
            fd.gyro_raw(i)
                .map(|raw| self.analyze_axis(raw, fd.gyro(i), throttle, &time_ref, fs))
        });

        SpectralAnalysis {
            axes,
            filter_markers: Self::filter_markers(&metadata.filters),
        }
    }

    fn analyze_axis(
        &self,
        raw: &[f64],
        filtered: Option<&[f64]>,
        throttle: Option<&[f64]>,
        time_ref: &[f64],
        fs: f64,
    ) -> AxisSpectral {
        let window = fft::window_size_for(fs, raw.len());
        let analyzer = SignalAnalyzer::new(fs, window, window / 2);

        let raw_psd = analyzer.psd_welch(raw);
        let raw_spectrum = analyzer.magnitude_welch(raw);
        let filtered_psd = filtered.map(|f| analyzer.psd_welch(f));
        let filtered_spectrum = filtered.map(|f| analyzer.magnitude_welch(f));
        let throttle_map = throttle.map(|t| analyzer.psd_binned(raw, t, self.throttle_bins));
        let time_map =
            (!time_ref.is_empty()).then(|| analyzer.psd_binned(raw, time_ref, self.time_bins));

        let noise_floor_db = median(&raw_psd.power_db);
        let peaks = self.find_peaks(&raw_psd, filtered_psd.as_ref(), noise_floor_db);

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

    fn filter_markers(cfg: &FilterConfig) -> Vec<FilterMarker> {
        let mut markers = Vec::new();

        if let Some(lpf) = &cfg.gyro_lpf1 {
            markers.push(FilterMarker {
                label: "Gyro LPF1".to_string(),
                center_hz: lpf.cutoff_hz(),
                cutoff_hz: Some(lpf.cutoff_hz()),
            });
        }
        if let Some(lpf) = &cfg.gyro_lpf2 {
            markers.push(FilterMarker {
                label: "Gyro LPF2".to_string(),
                center_hz: lpf.cutoff_hz,
                cutoff_hz: Some(lpf.cutoff_hz),
            });
        }
        if let Some(lpf) = &cfg.dterm_lpf1 {
            markers.push(FilterMarker {
                label: "Dterm LPF1".to_string(),
                center_hz: lpf.lowpass.cutoff_hz(),
                cutoff_hz: Some(lpf.lowpass.cutoff_hz()),
            });
        }
        if let Some(lpf) = &cfg.dterm_lpf2 {
            markers.push(FilterMarker {
                label: "Dterm LPF2".to_string(),
                center_hz: lpf.cutoff_hz,
                cutoff_hz: Some(lpf.cutoff_hz),
            });
        }

        for (i, n) in cfg.gyro_notches.iter().enumerate() {
            markers.push(FilterMarker {
                label: format!("Gyro notch {}", i + 1),
                center_hz: n.center_hz,
                cutoff_hz: Some(n.cutoff_hz),
            });
        }
        for (i, n) in cfg.dterm_notches.iter().enumerate() {
            markers.push(FilterMarker {
                label: format!("Dterm notch {}", i + 1),
                center_hz: n.center_hz,
                cutoff_hz: Some(n.cutoff_hz),
            });
        }
        if let Some(dyn_notch) = &cfg.dyn_notch {
            markers.push(FilterMarker {
                label: "Dynamic notch range".to_string(),
                center_hz: (dyn_notch.min_hz + dyn_notch.max_hz) / 2.0,
                cutoff_hz: None,
            });
        }

        markers
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::parser::Channel;

    /// 200 Hz sine on roll's pre-filter gyro, nothing on pitch/yaw.
    fn one_axis_log(freq_hz: f64, fs: f64, samples: usize) -> FlightData {
        let dt_us = (1e6 / fs) as u64;
        let sine = (0..samples)
            .map(|i| (std::f64::consts::TAU * freq_hz * i as f64 / fs).sin() * 50.0)
            .collect();
        FlightData::default()
            .with_time((0..samples as u64).map(|i| 7_000_000 + i * dt_us).collect())
            .with_channel(Channel::RawGyro(0), sine)
    }

    #[test]
    fn analyze_reports_the_injected_noise_frequency() {
        let analysis = GyroNoiseAnalyzer::default()
            .analyze(&one_axis_log(200.0, 2000.0, 8192), &Metadata::default());

        let roll = analysis.axes[0].as_ref().expect("roll analysed");
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

    #[test]
    fn axes_without_raw_gyro_are_not_analysed() {
        let analysis = GyroNoiseAnalyzer::default()
            .analyze(&one_axis_log(200.0, 2000.0, 2048), &Metadata::default());

        assert!(analysis.axes[1].is_none());
        assert!(analysis.axes[2].is_none());
    }

    #[test]
    fn throttle_map_absent_when_throttle_not_logged() {
        let analysis = GyroNoiseAnalyzer::default()
            .analyze(&one_axis_log(200.0, 2000.0, 2048), &Metadata::default());

        assert!(analysis.axes[0].as_ref().unwrap().throttle_map.is_none());
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
