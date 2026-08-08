use std::f64::consts::PI;
use std::sync::Arc;

use rustfft::{Fft, FftPlanner, num_complex::Complex};

#[derive(Debug, Clone)]
pub struct Psd {
    pub freq_hz: Vec<f64>,
    /// dB relative to the peak bin (peak = 0dB).
    pub power_db: Vec<f64>,
}

#[derive(Debug, Clone)]
pub struct Spectrum {
    pub freq_hz: Vec<f64>,
    /// Linear one-sided amplitude — no dB, no windowing/averaging.
    pub magnitude: Vec<f64>,
}

#[derive(Debug, Clone)]
pub struct BinnedSpectrum {
    pub freq_hz: Vec<f64>,
    /// Bin centers in the reference signal's own units (e.g. 0..1 for throttle, Hz for RPM).
    pub bin_centers: Vec<f64>,
    /// `[bin][freq_bin]`, dB relative to the peak bin across the whole map.
    /// A row is all-NaN if no chunk fell into that bin.
    pub power_db: Vec<Vec<f64>>,
}

/// FFT-based PSD estimation: single full-length periodogram, Welch (chunked +
/// averaged), and Welch binned by any synchronised reference signal (throttle,
/// RPM, ...).
///
/// `window_size`/`hop` configure Welch and binned analysis. `psd_raw` ignores
/// them — it always runs one rectangular FFT over the whole signal.
pub struct SignalAnalyzer {
    sample_rate_hz: f64,
    window_size: usize,
    hop: usize,
    hann: Vec<f64>,
    hann_energy: f64,
    fft: Arc<dyn Fft<f64>>,
}

impl SignalAnalyzer {
    pub fn new(sample_rate_hz: f64, window_size: usize, hop: usize) -> Self {
        let hann: Vec<f64> = (0..window_size)
            .map(|i| 0.5 - 0.5 * (2.0 * PI * i as f64 / (window_size - 1) as f64).cos())
            .collect();
        let hann_energy = hann.iter().map(|w| w * w).sum();
        let fft = FftPlanner::new().plan_fft_forward(window_size);

        Self {
            sample_rate_hz,
            window_size,
            hop,
            hann,
            hann_energy,
            fft,
        }
    }

    pub fn psd_raw(&self, signal: &[f64]) -> Psd {
        let n = signal.len();
        let half = n / 2;

        let mut buffer: Vec<Complex<f64>> =
            signal.iter().map(|&s| Complex { re: s, im: 0.0 }).collect();
        FftPlanner::new().plan_fft_forward(n).process(&mut buffer);

        let power: Vec<f64> = (0..=half)
            .map(|k| {
                one_sided_scale(k, half) / (self.sample_rate_hz * n as f64) * buffer[k].norm_sqr()
            })
            .collect();

        Psd {
            freq_hz: freq_axis(self.sample_rate_hz, n),
            power_db: to_relative_db(&power, peak_db(&power)),
        }
    }

    pub fn psd_welch(&self, signal: &[f64]) -> Psd {
        let power = self.welch_power(signal);
        Psd {
            freq_hz: freq_axis(self.sample_rate_hz, self.window_size),
            power_db: to_relative_db(&power, peak_db(&power)),
        }
    }

    /// Welch-averaged linear magnitude (sqrt of the averaged power) — same
    /// chunking/averaging as `psd_welch`, but no dB, for when the noise floor of
    /// a single periodogram (`fft_magnitude`) needs smoothing out.
    pub fn magnitude_welch(&self, signal: &[f64]) -> Spectrum {
        let power = self.welch_power(signal);
        Spectrum {
            freq_hz: freq_axis(self.sample_rate_hz, self.window_size),
            magnitude: power.iter().map(|p| p.sqrt()).collect(),
        }
    }

    fn welch_power(&self, signal: &[f64]) -> Vec<f64> {
        let half = self.window_size / 2;
        let mut sum = vec![0.0f64; half + 1];
        let mut count = 0usize;

        let mut start = 0;
        while start + self.window_size <= signal.len() {
            let chunk_power = self.chunk_power(&signal[start..start + self.window_size]);
            for (s, p) in sum.iter_mut().zip(&chunk_power) {
                *s += p;
            }
            count += 1;
            start += self.hop;
        }

        sum.iter().map(|&s| s / count.max(1) as f64).collect()
    }

    /// `reference` must be the same length and time-alignment as `signal` (e.g.
    /// throttle, RPM); each chunk is binned by the reference value at its midpoint,
    /// bin edges spanning `reference`'s own min..max.
    pub fn psd_binned(&self, signal: &[f64], reference: &[f64], n_bins: usize) -> BinnedSpectrum {
        let (ref_min, ref_max) = reference
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(mn, mx), &v| {
                (mn.min(v), mx.max(v))
            });
        let ref_range = (ref_max - ref_min).max(f64::MIN_POSITIVE);

        let half = self.window_size / 2;
        let mut sum = vec![vec![0.0f64; half + 1]; n_bins];
        let mut count = vec![0usize; n_bins];

        let mut start = 0;
        while start + self.window_size <= signal.len() {
            let mid = start + self.window_size / 2;
            let v = reference.get(mid).copied().unwrap_or(ref_min);
            let norm = ((v - ref_min) / ref_range).clamp(0.0, 1.0);
            let bin = ((norm * n_bins as f64) as usize).min(n_bins - 1);

            let chunk_power = self.chunk_power(&signal[start..start + self.window_size]);
            for (s, p) in sum[bin].iter_mut().zip(&chunk_power) {
                *s += p;
            }
            count[bin] += 1;
            start += self.hop;
        }

        let power: Vec<Vec<f64>> = sum
            .iter()
            .zip(&count)
            .map(|(row, &c)| {
                if c == 0 {
                    vec![f64::NAN; half + 1]
                } else {
                    row.iter().map(|&s| s / c as f64).collect()
                }
            })
            .collect();

        let global_peak = power
            .iter()
            .filter(|row| !row[0].is_nan())
            .map(|row| peak_db(row))
            .fold(f64::NEG_INFINITY, f64::max);

        let power_db = power
            .iter()
            .map(|row| {
                if row[0].is_nan() {
                    row.clone()
                } else {
                    to_relative_db(row, global_peak)
                }
            })
            .collect();

        BinnedSpectrum {
            freq_hz: freq_axis(self.sample_rate_hz, self.window_size),
            bin_centers: (0..n_bins)
                .map(|i| ref_min + (i as f64 + 0.5) / n_bins as f64 * ref_range)
                .collect(),
            power_db,
        }
    }

    /// Single rectangular FFT over the whole signal, one-sided, no dB — the raw
    /// FFT output as-is. Ignores `window_size`/`hop`, like `psd_raw`.
    pub fn fft_magnitude(&self, signal: &[f64]) -> Spectrum {
        let n = signal.len();
        let half = n / 2;

        let mut buffer: Vec<Complex<f64>> =
            signal.iter().map(|&s| Complex { re: s, im: 0.0 }).collect();
        FftPlanner::new().plan_fft_forward(n).process(&mut buffer);

        let magnitude: Vec<f64> = (0..=half)
            .map(|k| one_sided_scale(k, half) / n as f64 * buffer[k].norm())
            .collect();

        Spectrum {
            freq_hz: freq_axis(self.sample_rate_hz, n),
            magnitude,
        }
    }

    /// Hann-windowed, one-sided linear power for one `window_size`-long chunk.
    fn chunk_power(&self, chunk: &[f64]) -> Vec<f64> {
        let half = self.window_size / 2;

        let mut buffer: Vec<Complex<f64>> = chunk
            .iter()
            .zip(&self.hann)
            .map(|(&s, &w)| Complex { re: s * w, im: 0.0 })
            .collect();
        self.fft.process(&mut buffer);

        (0..=half)
            .map(|k| {
                one_sided_scale(k, half) / (self.sample_rate_hz * self.hann_energy)
                    * buffer[k].norm_sqr()
            })
            .collect()
    }
}

/// Welch window length targeting ~128ms, so freq/time resolution stays roughly
/// constant regardless of the log's sample rate.
pub fn window_size_for(sample_rate_hz: f64, signal_len: usize) -> usize {
    ((sample_rate_hz * 0.128) as usize)
        .next_power_of_two()
        .clamp(2, signal_len.max(2))
}

fn one_sided_scale(bin: usize, half: usize) -> f64 {
    if bin == 0 || bin == half { 1.0 } else { 2.0 }
}

fn freq_axis(sample_rate_hz: f64, window_size: usize) -> Vec<f64> {
    (0..=window_size / 2)
        .map(|k| k as f64 * sample_rate_hz / window_size as f64)
        .collect()
}

fn peak_db(power: &[f64]) -> f64 {
    10.0 * power
        .iter()
        .cloned()
        .fold(f64::MIN_POSITIVE, f64::max)
        .log10()
}

fn to_relative_db(power: &[f64], peak_db: f64) -> Vec<f64> {
    power
        .iter()
        .map(|&p| 10.0 * p.max(f64::MIN_POSITIVE).log10() - peak_db)
        .collect()
}
