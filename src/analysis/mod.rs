pub mod spectral;
pub mod step_response;

pub use spectral::SpectralResult;
pub use step_response::StepResponseResult;

use crate::parser::FlightData;

#[derive(Debug, Clone, Default)]
pub struct AnalysisResult {
    /// Raw pre-filter gyro (gyroUnfilt). Falls back to gyroADC if unavailable.
    pub spectral: [SpectralResult; 3],
    /// Filtered gyro (gyroADC).
    pub spectral_filtered: [SpectralResult; 3],
    pub step_response: [StepResponseResult; 3],
}

/// Compute the actual blackbox sample rate from frame timestamps.
/// Betaflight's `looptime` header field is the FC main loop rate, which may be
/// decimated for blackbox logging. The actual log rate is always derivable from
/// consecutive frame timestamps.
pub(crate) fn sample_rate_from_timestamps(time_us: &[u64]) -> Option<f32> {
    if time_us.len() < 2 {
        return None;
    }
    let mut diffs: Vec<u64> = time_us.windows(2).map(|w| w[1] - w[0]).collect();
    diffs.sort_unstable();
    let median_dt = diffs[diffs.len() / 2];
    if median_dt == 0 {
        return None;
    }
    Some((1_000_000.0 / median_dt as f32).clamp(100.0, 50_000.0))
}

pub fn analyse(data: &FlightData, nominal_hz: f32) -> AnalysisResult {
    let sample_rate_hz = sample_rate_from_timestamps(&data.time_us).unwrap_or(nominal_hz);
    let throttle = data.setpoint_throttle.as_deref();

    let spectral = std::array::from_fn(|i| {
        let signal = match i {
            0 => data.gyro_unfilt_roll.as_deref().or(data.gyro_adc_roll.as_deref()),
            1 => data.gyro_unfilt_pitch.as_deref().or(data.gyro_adc_pitch.as_deref()),
            _ => data.gyro_unfilt_yaw.as_deref().or(data.gyro_adc_yaw.as_deref()),
        };
        signal.map_or_else(SpectralResult::default, |s| {
            spectral::compute_spectral(s, throttle, sample_rate_hz)
        })
    });

    let spectral_filtered = std::array::from_fn(|i| {
        let signal = match i {
            0 => data.gyro_adc_roll.as_deref(),
            1 => data.gyro_adc_pitch.as_deref(),
            _ => data.gyro_adc_yaw.as_deref(),
        };
        signal.map_or_else(SpectralResult::default, |s| {
            spectral::compute_spectral(s, throttle, sample_rate_hz)
        })
    });

    // setpoint and gyroADC are both in deg/s — unit-consistent for normalisation.
    let step_response = std::array::from_fn(|i| {
        let (cmd, resp) = match i {
            0 => (data.setpoint_roll.as_deref(), data.gyro_adc_roll.as_deref()),
            1 => (data.setpoint_pitch.as_deref(), data.gyro_adc_pitch.as_deref()),
            _ => (data.setpoint_yaw.as_deref(), data.gyro_adc_yaw.as_deref()),
        };
        match (cmd, resp) {
            (Some(c), Some(r)) => {
                step_response::compute_step_response(c, r, throttle, 0.0, 1.0, sample_rate_hz)
            }
            _ => StepResponseResult::default(),
        }
    });

    AnalysisResult { spectral, spectral_filtered, step_response }
}
