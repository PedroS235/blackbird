use std::sync::Arc;

use crate::parser::SampleRateEstimate;

#[derive(Debug)]
pub struct FlightData {
    pub time_us: Arc<Vec<u64>>,
    pub sample_rate: SampleRateEstimate,

    pub raw_gyro: [Option<Vec<f64>>; 3],
    pub gyro: [Option<Vec<f64>>; 3],
    pub acceleration: [Option<Vec<f64>>; 3],

    pub setpoint: [Option<Vec<f64>>; 4],
    pub rc_command: [Option<Vec<f64>>; 4],

    pub motors: Vec<Vec<f64>>,
    pub rpm: Vec<Vec<f64>>,

    pub vbat: Option<Vec<f64>>,
    pub current: Option<Vec<f64>>,
    pub rssi: Option<Vec<f64>>,

    pub debug: [Option<Vec<f64>>; 8],
}
