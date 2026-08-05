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

/// Betaflight biquad-family filter implementations, shared by all LPF stages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FilterType {
    #[default]
    Pt1,
    Pt2,
    Pt3,
    Biquad,
}

impl FilterType {
    /// Decode Betaflight's `*_lpfN_type` header code (lowpassFilterTypeName order).
    pub fn from_bf_code(code: u8) -> Self {
        match code {
            1 => Self::Biquad,
            2 => Self::Pt2,
            3 => Self::Pt3,
            _ => Self::Pt1,
        }
    }
}

/// A lowpass stage that may run at a fixed cutoff or scale dynamically with
/// throttle between `dyn_min_hz`..`dyn_max_hz` (Betaflight's dynamic gyro/dterm
/// LPF1). `static_hz == 0.0` means the dynamic range is active.
#[derive(Debug, Clone)]
pub struct LowpassConfig {
    pub static_hz: f32,
    pub dyn_min_hz: f32,
    pub dyn_max_hz: f32,
    pub filter_type: FilterType,
}

impl LowpassConfig {
    pub fn is_dynamic(&self) -> bool {
        self.static_hz == 0.0
    }

    /// Single cutoff estimate for display/markers — the fixed value, or the
    /// dynamic range's ceiling as a worst case.
    pub fn cutoff_hz(&self) -> f32 {
        if self.static_hz > 0.0 {
            self.static_hz
        } else {
            self.dyn_max_hz
        }
    }
}

/// D-term LPF1 adds a dynamic expo curve (0..=100) shaping how the cutoff
/// scales across the dynamic range; unused when the stage is static.
#[derive(Debug, Clone)]
pub struct DtermLowpass1Config {
    pub lowpass: LowpassConfig,
    pub dyn_expo: f32,
}

/// LPF2 stages are always static: a single cutoff and filter type, no dynamic range.
#[derive(Debug, Clone)]
pub struct StaticLowpassConfig {
    pub cutoff_hz: f32,
    pub filter_type: FilterType,
}

#[derive(Debug, Clone)]
pub struct NotchConfig {
    pub center_hz: f32,
    pub cutoff_hz: f32,
}

#[derive(Debug, Clone)]
pub struct DynNotchConfig {
    pub min_hz: f32,
    pub max_hz: f32,
    pub count: u32,
    pub q: f32,
}

#[derive(Debug, Clone, Default)]
pub struct FilterConfig {
    pub gyro_lpf1: Option<LowpassConfig>,
    pub gyro_lpf2: Option<StaticLowpassConfig>,
    pub dterm_lpf1: Option<DtermLowpass1Config>,
    pub dterm_lpf2: Option<StaticLowpassConfig>,
    pub gyro_notches: Vec<NotchConfig>,
    pub dterm_notches: Vec<NotchConfig>,
    pub dyn_notch: Option<DynNotchConfig>,
    pub rpm_filter: Option<RpmFilterConfig>,
}

#[derive(Debug, Clone, Default)]
pub struct Metadata {
    pub file_name: String,
    pub craft_name: String,
    pub firmware: String,
    pub board: String,
    /// Raw looptime from the log header (what the pilot configured in Betaflight).
    pub looptime_us: Option<u32>,
    /// Total flight duration, derived from first/last frame timestamp.
    pub duration: Duration,
    pub filters: FilterConfig,
    /// Betaflight debug mode name (e.g. "FFT_FREQ"), or "NONE" if unset.
    pub debug_mode: String,
    /// Passthrough for all non-standard headers (PIDs, filter settings, rates, etc.)
    pub raw_headers: HashMap<String, String>,
}
