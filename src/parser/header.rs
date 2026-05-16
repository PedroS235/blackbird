use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct RpmFilterConfig {
    pub harmonics: u32,
    pub min_hz: f32,
    pub fade_range_hz: f32,
    /// Actual Q value (BF stores as Q × 100, already divided here).
    pub q: f32,
    /// Per-harmonic weight in 0..=1 (BF stores as 0..=100, already divided).
    pub weights: Vec<f32>,
}

#[derive(Debug, Clone)]
pub struct HeaderData {
    pub craft_name: String,
    pub firmware: String,
    pub board: String,
    /// Derived from `looptime` unknown header: 1_000_000 / looptime_us
    pub sample_rate_hz: Option<f32>,
    pub rpm_filter: Option<RpmFilterConfig>,
    /// Passthrough for all non-standard headers (PIDs, filter settings, rates, etc.)
    pub raw_headers: HashMap<String, String>,
}
