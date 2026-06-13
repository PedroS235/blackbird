pub mod spectral;
pub mod step_response;

pub use spectral::SpectralResult;
pub use step_response::StepResponseResult;

use crate::parser::FlightData;

#[derive(Debug, Clone, Default)]
pub struct AnalysisResult {
    pub spectral: [SpectralResult; 3],
    pub spectral_filtered: [SpectralResult; 3],
    pub step_response: [StepResponseResult; 3],
}

pub fn analyse(data: &FlightData) -> AnalysisResult {
    let sample_rate_hz = data.sample_rate.rate_hz;
    let throttle: Option<&[f64]> = data.setpoint[3].as_deref();

    let spectral = std::array::from_fn(|i| {
        let signal = data.raw_gyro[i].as_deref().or(data.gyro[i].as_deref());
        signal.map_or_else(SpectralResult::default, |s| {
            spectral::compute_spectral(s, throttle, sample_rate_hz)
        })
    });

    let spectral_filtered = std::array::from_fn(|i| {
        let signal = data.gyro[i].as_deref();
        signal.map_or_else(SpectralResult::default, |s| {
            spectral::compute_spectral(s, throttle, sample_rate_hz)
        })
    });

    let step_response = std::array::from_fn(|i| {
        let cmd = data.setpoint[i].as_deref();
        let resp = data.gyro[i].as_deref();
        match (cmd, resp) {
            (Some(c), Some(r)) => {
                step_response::compute_step_response(c, r, throttle, 0.0, 1.0, sample_rate_hz)
            }
            _ => StepResponseResult::default(),
        }
    });

    AnalysisResult {
        spectral,
        spectral_filtered,
        step_response,
    }
}
