use std::f64::consts::PI;
use std::sync::Arc;

use rustfft::{Fft, FftPlanner, num_complex::Complex};

#[derive(Debug, Clone)]
pub struct Psd {
    pub freq_hz: Arc<[f64]>,
    /// dB relative to the peak bin (peak = 0dB).
    pub power_db: Vec<f64>,
}

#[derive(Debug, Clone)]
pub struct Spectrum {
    pub freq_hz: Arc<[f64]>,
    /// Linear one-sided amplitude — no dB, no windowing/averaging.
    pub magnitude: Vec<f64>,
}

#[derive(Debug, Clone)]
pub struct BinnedSpectrum {
    pub freq_hz: Arc<[f64]>,
    /// Bin centers in the reference signal's own units (e.g. 0..1 for throttle, Hz for RPM).
    pub bin_centers: Vec<f64>,
    /// `[bin][freq_bin]`, dB relative to the peak bin across the whole map.
    /// A row is all-NaN if no chunk fell into that bin.
    pub power_db: Vec<Vec<f64>>,
}

/// FFT-based spectral estimation: a single full-length periodogram (`psd_raw`,
/// `fft_magnitude`), or a Welch `pass` that chunks the signal once and hands out
/// PSD, magnitude and any number of maps binned by a synchronised reference
/// signal (throttle, RPM, elapsed time, ...).
///
/// `window_size`/`hop` configure `pass`. `psd_raw`/`fft_magnitude` ignore them —
/// they always run one rectangular FFT over the whole signal.
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

    /// Starts a single chunked pass over `signal`. Every view (PSD, magnitude,
    /// binned maps) is derived from the one shared power array.
    pub fn pass<'a>(&'a self, signal: &'a [f64]) -> SpectralPass<'a> {
        SpectralPass {
            analyzer: self,
            signal,
            refs: Vec::new(),
        }
    }

    /// Index of the Nyquist bin — one-sided spectra are `half + 1` long.
    fn half(&self) -> usize {
        self.window_size / 2
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

/// In-progress spectral pass: accumulates chunk power once, plus one binned
/// accumulator per registered reference signal.
pub struct SpectralPass<'a> {
    analyzer: &'a SignalAnalyzer,
    signal: &'a [f64],
    refs: Vec<BinnedRef<'a>>,
}

impl<'a> SpectralPass<'a> {
    /// Adds a map of the same chunks binned by `reference`, which must share
    /// `signal`'s length and time-alignment (throttle, RPM, elapsed time, ...);
    /// each chunk lands in the bin of the reference value at its midpoint, bin
    /// edges spanning `reference`'s own min..max. Maps come back from
    /// `SpectralView::binned` in the order they were added.
    pub fn binned_by(mut self, reference: &'a [f64], n_bins: usize) -> Self {
        self.refs
            .push(BinnedRef::new(reference, n_bins, self.analyzer.half()));
        self
    }

    /// The one chunked pass: each chunk is FFT'd once and fed to the global
    /// accumulator and every binned accumulator.
    pub fn run(mut self) -> SpectralView {
        let window = self.analyzer.window_size;
        let mut sum = vec![0.0f64; self.analyzer.half() + 1];
        let mut count = 0usize;

        let mut start = 0;
        while start + window <= self.signal.len() {
            let chunk_power = self
                .analyzer
                .chunk_power(&self.signal[start..start + window]);
            accumulate(&mut sum, &chunk_power);
            count += 1;
            for r in &mut self.refs {
                r.accumulate(start + window / 2, &chunk_power);
            }
            start += self.analyzer.hop;
        }

        let freq_hz = freq_axis(self.analyzer.sample_rate_hz, window);
        SpectralView {
            power: sum.iter().map(|&s| s / count.max(1) as f64).collect(),
            binned: self
                .refs
                .into_iter()
                .map(|r| r.finish(freq_hz.clone()))
                .collect(),
            freq_hz,
        }
    }
}

/// One binned accumulator: chunk power summed per reference bin.
struct BinnedRef<'a> {
    reference: &'a [f64],
    min: f64,
    range: f64,
    sum: Vec<Vec<f64>>,
    count: Vec<usize>,
}

impl<'a> BinnedRef<'a> {
    fn new(reference: &'a [f64], n_bins: usize, half: usize) -> Self {
        let (min, max) = reference
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(mn, mx), &v| {
                (mn.min(v), mx.max(v))
            });

        Self {
            reference,
            min,
            range: (max - min).max(f64::MIN_POSITIVE),
            sum: vec![vec![0.0f64; half + 1]; n_bins],
            count: vec![0usize; n_bins],
        }
    }

    fn accumulate(&mut self, mid: usize, chunk_power: &[f64]) {
        let n_bins = self.count.len();
        let v = self.reference.get(mid).copied().unwrap_or(self.min);
        let norm = ((v - self.min) / self.range).clamp(0.0, 1.0);
        let bin = ((norm * n_bins as f64) as usize).min(n_bins - 1);

        accumulate(&mut self.sum[bin], chunk_power);
        self.count[bin] += 1;
    }

    fn finish(self, freq_hz: Arc<[f64]>) -> BinnedSpectrum {
        let n_bins = self.count.len();
        let power: Vec<Vec<f64>> = self
            .sum
            .iter()
            .zip(&self.count)
            .map(|(row, &c)| match c {
                0 => vec![f64::NAN; row.len()],
                c => row.iter().map(|&s| s / c as f64).collect(),
            })
            .collect();

        let global_peak = power
            .iter()
            .filter(|row| !row[0].is_nan())
            .map(|row| peak_db(row))
            .fold(f64::NEG_INFINITY, f64::max);

        BinnedSpectrum {
            freq_hz,
            bin_centers: (0..n_bins)
                .map(|i| self.min + (i as f64 + 0.5) / n_bins as f64 * self.range)
                .collect(),
            power_db: power
                .iter()
                .map(|row| match row[0].is_nan() {
                    true => row.clone(),
                    false => to_relative_db(row, global_peak),
                })
                .collect(),
        }
    }
}

fn accumulate(sum: &mut [f64], chunk_power: &[f64]) {
    for (s, p) in sum.iter_mut().zip(chunk_power) {
        *s += p;
    }
}

/// The result of one pass — every view is derived from `power`.
pub struct SpectralView {
    freq_hz: Arc<[f64]>,
    power: Vec<f64>,
    binned: Vec<BinnedSpectrum>,
}

impl SpectralView {
    pub fn psd(&self) -> Psd {
        Psd {
            freq_hz: self.freq_hz.clone(),
            power_db: to_relative_db(&self.power, peak_db(&self.power)),
        }
    }

    /// Welch-averaged linear magnitude (sqrt of the averaged power) — no dB,
    /// for when the noise floor of a single periodogram needs smoothing out.
    pub fn magnitude(&self) -> Spectrum {
        Spectrum {
            freq_hz: self.freq_hz.clone(),
            magnitude: self.power.iter().map(|p| p.sqrt()).collect(),
        }
    }

    /// The map for the `i`th `binned_by` reference, in the order they were added.
    pub fn binned(&self, i: usize) -> &BinnedSpectrum {
        &self.binned[i]
    }

    pub fn into_binned(self) -> Vec<BinnedSpectrum> {
        self.binned
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

fn freq_axis(sample_rate_hz: f64, window_size: usize) -> Arc<[f64]> {
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

#[cfg(test)]
mod test {
    use super::*;
    use std::f64::consts::TAU;

    const FS: f64 = 2000.0;

    fn sine(freq_hz: f64, n: usize) -> Vec<f64> {
        (0..n)
            .map(|i| (TAU * freq_hz * i as f64 / FS).sin())
            .collect()
    }

    fn analyzer() -> SignalAnalyzer {
        SignalAnalyzer::new(FS, 256, 128)
    }

    fn peak_freq(freq: &[f64], values: &[f64]) -> f64 {
        let k = (0..values.len())
            .max_by(|&a, &b| values[a].total_cmp(&values[b]))
            .expect("non-empty");
        freq[k]
    }

    #[test]
    fn psd_peaks_at_the_injected_frequency() {
        let view = analyzer().pass(&sine(200.0, 8192)).run();
        let psd = view.psd();

        assert!(
            (peak_freq(&psd.freq_hz, &psd.power_db) - 200.0).abs() < 10.0,
            "expected peak near 200 Hz, got {} Hz",
            peak_freq(&psd.freq_hz, &psd.power_db)
        );
        assert_eq!(
            psd.power_db
                .iter()
                .cloned()
                .fold(f64::NEG_INFINITY, f64::max),
            0.0,
            "PSD is dB relative to its own peak"
        );
    }

    #[test]
    fn magnitude_shares_the_pass_frequency_axis_with_the_psd() {
        let view = analyzer().pass(&sine(200.0, 8192)).run();
        let psd = view.psd();
        let mag = view.magnitude();

        assert!(
            Arc::ptr_eq(&psd.freq_hz, &mag.freq_hz),
            "both views hand out the one axis the pass allocated"
        );
        assert!(
            (peak_freq(&mag.freq_hz, &mag.magnitude) - 200.0).abs() < 10.0,
            "magnitude peaks where the PSD does"
        );
        assert!(
            mag.magnitude.iter().all(|&m| m >= 0.0),
            "linear amplitude, not dB"
        );
    }

    #[test]
    fn one_pass_derives_a_map_per_reference() {
        let n = 8192;
        let throttle: Vec<f64> = (0..n).map(|i| i as f64 / n as f64).collect();
        let time: Vec<f64> = (0..n).map(|i| i as f64 / FS).collect();

        let view = analyzer()
            .pass(&sine(200.0, n))
            .binned_by(&throttle, 4)
            .binned_by(&time, 10)
            .run();

        let by_throttle = view.binned(0);
        let by_time = view.binned(1);

        assert_eq!(by_throttle.power_db.len(), 4, "maps come back in ref order");
        assert_eq!(by_time.power_db.len(), 10);
        assert!(
            Arc::ptr_eq(&by_throttle.freq_hz, &view.psd().freq_hz),
            "maps reuse the pass's one axis"
        );
        for row in &by_throttle.power_db {
            assert!(
                (peak_freq(&by_throttle.freq_hz, row) - 200.0).abs() < 10.0,
                "the tone is present in every throttle bin"
            );
        }
    }

    #[test]
    fn bins_no_chunk_fell_into_are_nan() {
        let n = 4096;
        // Reference sits in the bottom half of its range for the whole log.
        let reference: Vec<f64> = (0..n).map(|i| if i < n / 2 { 0.0 } else { 1.0 }).collect();

        let view = analyzer()
            .pass(&sine(200.0, n))
            .binned_by(&reference, 4)
            .run();
        let map = view.binned(0);

        assert!(
            map.power_db[0].iter().all(|v| !v.is_nan()),
            "low bin filled"
        );
        assert!(
            map.power_db[1].iter().all(|v| v.is_nan()),
            "no chunk in bin 1"
        );
        assert!(
            map.power_db[2].iter().all(|v| v.is_nan()),
            "no chunk in bin 2"
        );
    }
}
