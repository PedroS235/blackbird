use std::sync::Arc;

use crate::parser::SampleRateEstimate;

#[derive(Debug, Default)]
pub struct FlightData {
    pub(super) time_us: Arc<Vec<u64>>,
    pub(super) sample_rate: SampleRateEstimate,

    pub(super) raw_gyro: [Option<Vec<f64>>; 3],
    pub(super) gyro: [Option<Vec<f64>>; 3],
    pub(super) acceleration: [Option<Vec<f64>>; 3],

    pub(super) setpoint: [Option<Vec<f64>>; 4],
    pub(super) rc_command: [Option<Vec<f64>>; 4],

    pub(super) motors: Vec<Vec<f64>>,
    pub(super) rpm: Vec<Vec<f64>>,

    pub(super) vbat: Option<Vec<f64>>,
    pub(super) current: Option<Vec<f64>>,
    pub(super) rssi: Option<Vec<f64>>,

    pub(super) debug: [Option<Vec<f64>>; 8],
}

/// Betaflight axis order — the discriminant *is* the index into every
/// per-axis array, so an `Axis` can never point outside one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum Axis {
    Roll = 0,
    Pitch,
    Yaw,
}

impl Axis {
    pub const ALL: [Axis; 3] = [Axis::Roll, Axis::Pitch, Axis::Yaw];

    pub const fn index(self) -> usize {
        self as usize
    }

    pub const fn name(self) -> &'static str {
        match self {
            Axis::Roll => "roll",
            Axis::Pitch => "pitch",
            Axis::Yaw => "yaw",
        }
    }
}

/// Three values, one per axis, indexed by `Axis` instead of by number.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PerAxis<T>(pub [T; 3]);

impl<T> PerAxis<T> {
    pub const fn splat(value: T) -> Self
    where
        T: Copy,
    {
        Self([value; 3])
    }
}

impl<T> std::ops::Index<Axis> for PerAxis<T> {
    type Output = T;

    fn index(&self, axis: Axis) -> &T {
        &self.0[axis.index()]
    }
}

impl<T> std::ops::IndexMut<Axis> for PerAxis<T> {
    fn index_mut(&mut self, axis: Axis) -> &mut T {
        &mut self.0[axis.index()]
    }
}

/// Betaflight logs throttle as the fourth stick channel.
const THROTTLE: usize = 3;

/// Names a single logged stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    RawGyro(Axis),
    Gyro(Axis),
    RcCommand(Axis),
    Setpoint(Axis),
    Acceleration(Axis),
    /// `rcCommand[3]` — the fourth stick channel, which has no axis.
    Throttle,
    Motor(usize),
    Debug(usize),
    Vbat,
    Current,
    Rssi,
}

impl FlightData {
    pub fn with_channel(mut self, channel: Channel, samples: Vec<f64>) -> Self {
        let slot = match channel {
            Channel::RawGyro(axis) => &mut self.raw_gyro[axis.index()],
            Channel::Gyro(axis) => &mut self.gyro[axis.index()],
            Channel::Acceleration(axis) => &mut self.acceleration[axis.index()],
            Channel::Setpoint(axis) => &mut self.setpoint[axis.index()],
            Channel::RcCommand(axis) => &mut self.rc_command[axis.index()],
            Channel::Throttle => &mut self.rc_command[THROTTLE],
            Channel::Vbat => &mut self.vbat,
            Channel::Current => &mut self.current,
            Channel::Rssi => &mut self.rssi,
            Channel::Debug(i) => match self.debug.get_mut(i) {
                Some(slot) => slot,
                None => return self,
            },
            Channel::Motor(i) => {
                if self.motors.len() <= i {
                    self.motors.resize(i + 1, Vec::new());
                }
                self.motors[i] = samples;
                return self;
            }
        };
        *slot = Some(samples);
        self
    }

    pub fn channel(&self, channel: Channel) -> Option<&[f64]> {
        match channel {
            Channel::RawGyro(axis) => self.raw_gyro[axis.index()].as_deref(),
            Channel::Gyro(axis) => self.gyro[axis.index()].as_deref(),
            Channel::Acceleration(axis) => self.acceleration[axis.index()].as_deref(),
            Channel::Setpoint(axis) => self.setpoint[axis.index()].as_deref(),
            Channel::RcCommand(axis) => self.rc_command[axis.index()].as_deref(),
            Channel::Throttle => self.rc_command[THROTTLE].as_deref(),
            Channel::Debug(i) => self.debug.get(i)?.as_deref(),
            Channel::Motor(i) => self.motors.get(i).map(Vec::as_slice),
            Channel::Vbat => self.vbat.as_deref(),
            Channel::Current => self.current.as_deref(),
            Channel::Rssi => self.rssi.as_deref(),
        }
    }

    /// Battery telemetry — either voltage or current is enough to draw the tab.
    pub fn has_power(&self) -> bool {
        self.channel(Channel::Vbat).is_some() || self.channel(Channel::Current).is_some()
    }

    pub fn has_rssi(&self) -> bool {
        self.channel(Channel::Rssi).is_some()
    }

    /// `debug[0..3]` populated — under debug mode `FFT_FREQ` that's the dynamic
    /// notch tracker's per-axis center frequency.
    pub fn has_debug_axes(&self) -> bool {
        Axis::ALL.iter().any(|&a| self.debug_axis(a).is_some())
    }

    /// Sets the time axis and the sample rate together — they're derived from
    /// the same timestamps, so they can never disagree.
    pub fn with_time(mut self, time_us: Vec<u64>) -> Self {
        self.sample_rate = SampleRateEstimate::from_timestamps(&time_us);
        self.time_us = Arc::new(time_us);
        self
    }

    pub fn sample_rate_hz(&self) -> f64 {
        self.sample_rate.rate_hz as f64
    }

    pub fn time_us(&self) -> &[u64] {
        &self.time_us
    }

    /// Flight-controller uptime at the first sample — logs never start at zero,
    /// so every time axis is drawn relative to this.
    pub fn start_us(&self) -> u64 {
        self.time_us.first().copied().unwrap_or(0)
    }

    pub fn duration_s(&self) -> f64 {
        self.time_us
            .last()
            .map(|&last| last.saturating_sub(self.start_us()) as f64 / 1e6)
            .unwrap_or(0.0)
    }

    /// Sample timestamps in seconds since the start of the log.
    pub fn time_s(&self) -> Vec<f64> {
        let t0 = self.start_us();
        self.time_us
            .iter()
            .map(|&t| t.saturating_sub(t0) as f64 / 1e6)
            .collect()
    }

    /// Pre-filter gyro — what the noise analysis wants.
    pub fn gyro_raw(&self, axis: Axis) -> Option<&[f64]> {
        self.channel(Channel::RawGyro(axis))
    }

    /// Post-filter gyro, i.e. `gyroADC` — what the PID loop actually saw.
    pub fn gyro(&self, axis: Axis) -> Option<&[f64]> {
        self.channel(Channel::Gyro(axis))
    }

    pub fn setpoint(&self, axis: Axis) -> Option<&[f64]> {
        self.channel(Channel::Setpoint(axis))
    }

    pub fn debug(&self, index: usize) -> Option<&[f64]> {
        self.channel(Channel::Debug(index))
    }

    /// Under debug mode `FFT_FREQ`, `debug[0..3]` is the dynamic notch
    /// tracker's center frequency for that axis.
    pub fn debug_axis(&self, axis: Axis) -> Option<&[f64]> {
        self.debug(axis.index())
    }

    pub fn vbat(&self) -> Option<&[f64]> {
        self.channel(Channel::Vbat)
    }

    pub fn current(&self) -> Option<&[f64]> {
        self.channel(Channel::Current)
    }

    pub fn rssi(&self) -> Option<&[f64]> {
        self.channel(Channel::Rssi)
    }

    pub fn throttle(&self) -> Option<&[f64]> {
        self.channel(Channel::Throttle)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    /// Throttle shares storage with the stick channels — it is `rcCommand[3]`.
    #[test]
    fn throttle_is_the_fourth_stick_channel() {
        let fd = FlightData::default().with_channel(Channel::Throttle, vec![0.0, 500.0, 1000.0]);
        assert_eq!(fd.throttle(), Some(&[0.0, 500.0, 1000.0][..]));
        assert_eq!(fd.channel(Channel::RcCommand(Axis::Roll)), None);
    }

    #[test]
    fn throttle_absent_when_not_logged() {
        assert_eq!(FlightData::default().throttle(), None);
    }

    #[test]
    fn raw_and_filtered_gyro_are_separate_channels_per_axis() {
        let fd = FlightData::default()
            .with_channel(Channel::RawGyro(Axis::Pitch), vec![10.0])
            .with_channel(Channel::Gyro(Axis::Pitch), vec![8.0]);

        assert_eq!(fd.gyro_raw(Axis::Pitch), Some(&[10.0][..]));
        assert_eq!(fd.gyro(Axis::Pitch), Some(&[8.0][..]));
        assert_eq!(fd.gyro_raw(Axis::Roll), None);
    }

    /// Only the index-carrying channels can be out of range; `Axis` cannot.
    #[test]
    fn channel_out_of_range_is_absent() {
        let fd = FlightData::default().with_channel(Channel::Debug(99), vec![1.0]);
        assert_eq!(fd.debug(99), None);
        assert_eq!(fd.channel(Channel::Motor(9)), None);
    }

    /// Logs start at an arbitrary flight-controller uptime, not at zero.
    #[test]
    fn time_is_relative_to_first_sample() {
        let fd = FlightData::default().with_time(vec![5_000_000, 5_500_000, 6_000_000]);

        assert_eq!(fd.start_us(), 5_000_000);
        assert_eq!(fd.duration_s(), 1.0);
        assert_eq!(fd.time_s(), vec![0.0, 0.5, 1.0]);
    }

    #[test]
    fn power_present_when_either_vbat_or_current_logged() {
        let fd = FlightData::default();
        assert!(!fd.has_power());
        assert!(fd.with_channel(Channel::Current, vec![12.0]).has_power());
    }

    #[test]
    fn rssi_and_debug_presence() {
        let fd = FlightData::default();
        assert!(!fd.has_rssi());
        assert!(!fd.has_debug_axes());

        let fd = fd
            .with_channel(Channel::Rssi, vec![90.0])
            .with_channel(Channel::Debug(2), vec![320.0]);
        assert!(fd.has_rssi());
        assert!(fd.has_debug_axes());
    }

    /// The dyn-notch tracker only writes debug[0..3]; a trace in debug[7] is
    /// something else entirely.
    #[test]
    fn debug_axes_are_the_first_three_channels_only() {
        let fd = FlightData::default().with_channel(Channel::Debug(7), vec![1.0]);
        assert!(!fd.has_debug_axes());
    }

    #[test]
    fn empty_log_has_zero_duration() {
        let fd = FlightData::default();
        assert_eq!(fd.start_us(), 0);
        assert_eq!(fd.duration_s(), 0.0);
        assert!(fd.time_s().is_empty());
    }
}
