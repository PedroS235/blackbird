use rustfft::{FftPlanner, num_complex::Complex};
use std::f64::consts::PI;

pub const N_THROTTLE_BINS: usize = 10;
const WINDOW_SIZE: usize = 1600;
const HOP: usize = 256;

#[derive(Debug, Clone, Default)]
pub struct FrequencyPeak {
    pub freq_hz: f32,
    pub amplitude_db: f32,
}

#[derive(Debug, Clone)]
pub struct SpectralResult {
    pub noise_floor_db: f32,
    pub peaks: Vec<FrequencyPeak>,
    /// `[throttle_bin][freq_bin]` = amplitude in dB; NaN where bin has no samples.
    pub throttle_map: Vec<Vec<f32>>,
    /// Average spectrum across all throttle bins; index = freq bin, value = dB.
    pub average_spectrum: Vec<f32>,
    pub freq_resolution_hz: f32,
}

impl Default for SpectralResult {
    fn default() -> Self {
        Self {
            noise_floor_db: 0.0,
            peaks: Vec::new(),
            throttle_map: vec![vec![]; N_THROTTLE_BINS],
            average_spectrum: Vec::new(),
            freq_resolution_hz: 1.0,
        }
    }
}

pub fn compute_spectral(
    signal: &[f64],
    throttle: Option<&[f64]>,
    sample_rate_hz: f32,
) -> SpectralResult {
    let n = signal.len();
    if n < WINDOW_SIZE {
        return SpectralResult::default();
    }

    let n_freq = WINDOW_SIZE / 2 + 1;
    let freq_resolution_hz = sample_rate_hz / WINDOW_SIZE as f32;

    let hann: Vec<f64> = (0..WINDOW_SIZE)
        .map(|i| 0.5 * (1.0 - (2.0 * PI * i as f64 / (WINDOW_SIZE as f64 - 1.0)).cos()))
        .collect();
    let hann_power: f64 = hann.iter().map(|&w| w * w).sum::<f64>() / WINDOW_SIZE as f64;

    let norm_throttle: Option<Vec<f64>> = throttle.map(|t| {
        let (tmin, tmax) = t
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(mn, mx), &v| {
                (mn.min(v), mx.max(v))
            });
        let range = (tmax - tmin).max(1e-6);
        t.iter()
            .map(|&v| ((v - tmin) / range).clamp(0.0, 1.0))
            .collect()
    });

    let mut planner = FftPlanner::<f64>::new();
    let fft = planner.plan_fft_forward(WINDOW_SIZE);

    let mut mag2_sum = vec![vec![0.0f64; n_freq]; N_THROTTLE_BINS];
    let mut counts = vec![0usize; N_THROTTLE_BINS];
    let mut buf = vec![Complex { re: 0.0f64, im: 0.0f64 }; WINDOW_SIZE];

    let mut start = 0;
    while start + WINDOW_SIZE <= n {
        let mid = start + WINDOW_SIZE / 2;
        let tbin = norm_throttle
            .as_ref()
            .map_or(0, |t| {
                (t.get(mid).copied().unwrap_or(0.0) * N_THROTTLE_BINS as f64) as usize
            })
            .min(N_THROTTLE_BINS - 1);

        for (i, &s) in signal[start..start + WINDOW_SIZE].iter().enumerate() {
            buf[i] = Complex { re: s * hann[i], im: 0.0 };
        }
        fft.process(&mut buf);

        for k in 0..n_freq {
            mag2_sum[tbin][k] += buf[k].norm_sqr() / (WINDOW_SIZE as f64 * hann_power);
        }
        counts[tbin] += 1;
        start += HOP;
    }

    let throttle_map: Vec<Vec<f32>> = mag2_sum
        .into_iter()
        .zip(counts.iter())
        .map(|(row, &cnt)| {
            if cnt == 0 {
                return vec![f32::NAN; n_freq];
            }
            row.iter()
                .map(|&m| (20.0 * (m / cnt as f64).sqrt().max(1e-10).log10()) as f32)
                .collect()
        })
        .collect();

    let mut overall = vec![0.0f32; n_freq];
    let mut overall_cnt = 0usize;
    for row in &throttle_map {
        if row.first().map_or(true, |v| v.is_nan()) {
            continue;
        }
        for (k, &v) in row.iter().enumerate() {
            overall[k] += v;
        }
        overall_cnt += 1;
    }
    if overall_cnt > 0 {
        for v in &mut overall {
            *v /= overall_cnt as f32;
        }
    }

    let peak_db = overall
        .iter()
        .copied()
        .filter(|v| v.is_finite())
        .fold(f32::NEG_INFINITY, f32::max);
    if peak_db.is_finite() {
        for v in &mut overall {
            if v.is_finite() {
                *v -= peak_db;
            }
        }
    }

    let noise_floor_db = overall
        .iter()
        .copied()
        .filter(|v| v.is_finite())
        .fold(f32::INFINITY, f32::min);
    let threshold = noise_floor_db + 10.0;
    let min_bin = (10.0 / freq_resolution_hz).ceil() as usize;

    let mut peaks: Vec<FrequencyPeak> = (min_bin + 1..n_freq.saturating_sub(1))
        .filter(|&k| {
            overall[k] > threshold && overall[k] > overall[k - 1] && overall[k] > overall[k + 1]
        })
        .map(|k| FrequencyPeak {
            freq_hz: k as f32 * freq_resolution_hz,
            amplitude_db: overall[k],
        })
        .collect();
    peaks.sort_by(|a, b| b.amplitude_db.partial_cmp(&a.amplitude_db).unwrap());
    peaks.truncate(10);

    SpectralResult {
        noise_floor_db,
        peaks,
        throttle_map,
        average_spectrum: overall,
        freq_resolution_hz,
    }
}
