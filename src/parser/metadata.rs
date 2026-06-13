use std::collections::HashMap;
use std::time::Duration;

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
pub struct Metadata {
    pub file_name: String,
    pub craft_name: String,
    pub firmware: String,
    pub board: String,
    /// Raw looptime from the log header (what the pilot configured in Betaflight).
    pub looptime_us: Option<u32>,
    /// Total flight duration, derived from first/last frame timestamp.
    pub duration: Duration,
    pub rpm_filter: Option<RpmFilterConfig>,
    /// Passthrough for all non-standard headers (PIDs, filter settings, rates, etc.)
    pub raw_headers: HashMap<String, String>,
}
