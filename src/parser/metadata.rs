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
    /// Shapes how the cutoff scales across the dynamic range, 0..=100. Both
    /// loops have one — it used to sit on a D-term-only wrapper type, which
    /// left the gyro's unread and its dynamic curve wrong.
    pub dyn_expo: f32,
    pub filter_type: FilterType,
}

impl LowpassConfig {
    pub fn is_dynamic(&self) -> bool {
        self.static_hz == 0.0
    }

    /// The cutoff this stage ran at, for a throttle in 0..=1.
    ///
    /// Betaflight's `dynLpfCutoffFreq`, with the pre-expo `dynThrottle` curve
    /// for the builds and configs that set no expo. A static stage ignores the
    /// throttle and answers its one cutoff.
    pub fn cutoff_at(&self, throttle: f32) -> f32 {
        if !self.is_dynamic() {
            return self.static_hz;
        }
        let throttle = throttle.clamp(0.0, 1.0);
        let (min, max) = (self.dyn_min_hz, self.dyn_max_hz);

        match self.dyn_expo > 0.0 {
            true => {
                let curve = throttle * (1.0 - throttle) * (self.dyn_expo / 10.0) + throttle;
                (max - min) * curve + min
            }
            false => {
                let curve = throttle * (1.0 - throttle * throttle * throttle / 3.0) * 1.5;
                (curve * max).max(min)
            }
        }
    }
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
    pub dterm_lpf1: Option<LowpassConfig>,
    pub dterm_lpf2: Option<StaticLowpassConfig>,
    pub gyro_notches: Vec<NotchConfig>,
    pub dterm_notches: Vec<NotchConfig>,
    pub dyn_notch: Option<DynNotchConfig>,
    pub rpm_filter: Option<RpmFilterConfig>,
}

/// Betaflight's rate curve families, decoded from the `rates_type` header the
/// same way filter types are decoded from theirs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RateType {
    #[default]
    Betaflight,
    Raceflight,
    Kiss,
    Actual,
    Quick,
    /// A build we do not know the curve for. Rendered as the raw code rather
    /// than guessed at — a wrong conversion reads as fact.
    Unknown(u32),
    /// The header is there but is not a code at all. Named rather than
    /// defaulted, for the same reason as `Unknown`.
    Unreadable,
}

impl RateType {
    /// Decode Betaflight's `rates_type` header code (`rateTypeNames` order).
    pub fn from_bf_code(code: u32) -> Self {
        match code {
            0 => Self::Betaflight,
            1 => Self::Raceflight,
            2 => Self::Kiss,
            3 => Self::Actual,
            4 => Self::Quick,
            code => Self::Unknown(code),
        }
    }
}

impl std::fmt::Display for RateType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Betaflight => f.write_str("Betaflight"),
            Self::Raceflight => f.write_str("Raceflight"),
            Self::Kiss => f.write_str("KISS"),
            Self::Actual => f.write_str("Actual"),
            Self::Quick => f.write_str("Quick"),
            Self::Unknown(code) => write!(f, "rates type {code}"),
            Self::Unreadable => f.write_str("unrecognised rates"),
        }
    }
}

/// The craft's rate curve as the log records it: the raw header values and the
/// curve family, per axis in roll/pitch/yaw order. No centre-sensitivity or
/// maximum-rate maths — each rate type needs its own formula and none are
/// verified yet, so this is the typed place that work lands.
///
/// `rc_rates` and `expo` are `None` when the log did not record them, so that
/// the maths landing here later cannot derive a rate from a fabricated zero.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RateConfig {
    pub rate_type: RateType,
    pub rc_rates: Option<[f32; 3]>,
    pub rates: [f32; 3],
    pub expo: Option<[f32; 3]>,
}

impl std::fmt::Display for RateConfig {
    /// `Actual 67/67/67` — the type and the raw per-axis rate values, as the
    /// log records them. Not deg/s: under Actual and Quick rates the
    /// configurator shows roughly ten times these numbers, and under
    /// Betaflight rates something else again.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let [roll, pitch, yaw] = self.rates;
        write!(f, "{} {roll:.0}/{pitch:.0}/{yaw:.0}", self.rate_type)
    }
}

/// Betaflight's default, and what both repository fixtures were flown on. A
/// wrong pole count is wrong by an obvious integer factor, which a pilot
/// spots; refusing to draw the harmonics teaches nothing.
pub const DEFAULT_MOTOR_POLES: f32 = 14.0;

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
    /// `None` when the log carries no rate headers at all — better than
    /// showing a pilot zeroes as if they were their rates.
    pub rates: Option<RateConfig>,
    /// Betaflight debug mode name (e.g. "FFT_FREQ"), or "NONE" if unset.
    pub debug_mode: String,
    /// Passthrough for all non-standard headers (PIDs, filter settings, rates, etc.)
    pub raw_headers: HashMap<String, String>,
}

impl Metadata {
    /// Motor pole count, from the raw header passthrough — the first consumer
    /// of it, and not worth a typed field until there is a second.
    pub fn motor_poles(&self) -> f32 {
        self.raw_headers
            .get("motor_poles")
            .and_then(|v| v.trim().parse::<f32>().ok())
            .filter(|&p| p > 0.0)
            .unwrap_or(DEFAULT_MOTOR_POLES)
    }

    /// The rate the filters actually run at, which is the PID loop's, not the
    /// blackbox's — a log recorded at every second frame halves the latter and
    /// would model every stage as rolling off far earlier than it does.
    ///
    /// Falls back to the logged rate where the header is missing.
    pub fn filter_rate_hz(&self, logged_hz: f64) -> f64 {
        self.looptime_us
            .map(|us| 1e6 / us as f64)
            .filter(|hz| *hz > 0.0)
            .unwrap_or(logged_hz)
    }

    /// Whether `debug[0..3]` is the dynamic notch tracker's centre frequency.
    /// One rule, read by the spectrogram overlay and the PSD's traced centre
    /// alike — two copies of it would drift apart.
    pub fn logs_dyn_notch_trace(&self) -> bool {
        self.debug_mode == "FFT_FREQ"
    }

    /// `eRPM` is electrical RPM in hundreds; a motor turns once per pole pair,
    /// and the spectrum is in hertz.
    pub fn erpm_to_hz(&self, erpm: f64) -> f64 {
        erpm * 100.0 / (self.motor_poles() as f64 / 2.0) / 60.0
    }
}

#[cfg(test)]
mod test {
    use super::*;

    /// The filters run at the loop rate. A log written every second frame is
    /// half that, and modelling a stage at the logging rate would draw it
    /// rolling off far earlier than it does.
    #[test]
    fn the_filter_rate_is_the_loop_rate_not_the_logging_rate() {
        let metadata = Metadata {
            looptime_us: Some(125),
            ..Default::default()
        };

        assert!((metadata.filter_rate_hz(4000.0) - 8000.0).abs() < 1.0);
        assert_eq!(Metadata::default().filter_rate_hz(4000.0), 4000.0);
    }

    fn dynamic(dyn_expo: f32) -> LowpassConfig {
        LowpassConfig {
            static_hz: 0.0,
            dyn_min_hz: 250.0,
            dyn_max_hz: 500.0,
            dyn_expo,
            filter_type: FilterType::Pt1,
        }
    }

    /// The ends of the throttle range pin the ends of the cutoff range,
    /// whatever the expo does between them.
    #[test]
    fn a_dynamic_cutoff_spans_its_configured_range() {
        let lpf = dynamic(5.0);

        assert!((lpf.cutoff_at(0.0) - 250.0).abs() < 1e-3);
        assert!((lpf.cutoff_at(1.0) - 500.0).abs() < 1e-3);
        assert!((250.0..=500.0).contains(&lpf.cutoff_at(0.5)));
    }

    /// Expo bows the curve upward, so mid throttle sits above the straight
    /// line between the ends.
    #[test]
    fn expo_raises_the_cutoff_at_mid_throttle() {
        assert!(dynamic(5.0).cutoff_at(0.5) > dynamic(0.0).cutoff_at(0.5));
    }

    /// A static stage answers its one cutoff whatever the throttle did.
    #[test]
    fn a_static_lowpass_ignores_the_throttle() {
        let lpf = LowpassConfig {
            static_hz: 120.0,
            ..dynamic(5.0)
        };

        assert_eq!(lpf.cutoff_at(0.0), 120.0);
        assert_eq!(lpf.cutoff_at(1.0), 120.0);
    }

    fn with_headers(pairs: &[(&str, &str)]) -> Metadata {
        Metadata {
            raw_headers: pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            ..Default::default()
        }
    }

    /// 4000 eRPM on a 14-pole motor: 400 000 electrical RPM over seven pole
    /// pairs is 57 143 mechanical RPM, or 952 Hz.
    #[test]
    fn erpm_converts_to_hertz_through_the_pole_count() {
        let hz = with_headers(&[("motor_poles", "14")]).erpm_to_hz(4000.0);
        assert!((hz - 952.38).abs() < 0.01, "{hz} Hz");
    }

    /// Half the poles, twice the mechanical speed for the same eRPM.
    #[test]
    fn a_seven_pole_pair_motor_and_a_three_and_a_half_differ_by_the_ratio() {
        let seven = with_headers(&[("motor_poles", "14")]).erpm_to_hz(4000.0);
        let fourteen = with_headers(&[("motor_poles", "7")]).erpm_to_hz(4000.0);

        assert!((fourteen / seven - 2.0).abs() < 1e-6);
    }

    #[test]
    fn a_missing_or_zero_pole_count_falls_back_to_the_betaflight_default() {
        assert_eq!(Metadata::default().motor_poles(), DEFAULT_MOTOR_POLES);
        assert_eq!(
            with_headers(&[("motor_poles", "0")]).motor_poles(),
            DEFAULT_MOTOR_POLES
        );
    }
}
