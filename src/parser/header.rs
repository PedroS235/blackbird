use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct HeaderData {
    pub craft_name: String,
    pub firmware: String,
    pub board: String,
    /// Derived from `looptime` unknown header: 1_000_000 / looptime_us
    pub sample_rate_hz: Option<f32>,
    /// Passthrough for all non-standard headers (PIDs, filter settings, rates, etc.)
    pub raw_headers: HashMap<String, String>,
}
