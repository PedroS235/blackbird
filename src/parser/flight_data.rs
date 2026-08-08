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

/// Names a single logged stream. The index is the axis (roll/pitch/yaw) for
/// gyro-like channels, or the Betaflight channel number for stick input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    RawGyro(usize),
    Gyro(usize),
    RcCommand(usize),
    Setpoint(usize),
    Acceleration(usize),
    Motor(usize),
    Debug(usize),
    Vbat,
    Current,
    Rssi,
}

impl FlightData {
    pub fn with_channel(mut self, channel: Channel, samples: Vec<f64>) -> Self {
        let slot = match channel {
            Channel::RawGyro(i) => self.raw_gyro.get_mut(i),
            Channel::Gyro(i) => self.gyro.get_mut(i),
            Channel::Acceleration(i) => self.acceleration.get_mut(i),
            Channel::Setpoint(i) => self.setpoint.get_mut(i),
            Channel::RcCommand(i) => self.rc_command.get_mut(i),
            Channel::Debug(i) => self.debug.get_mut(i),
            Channel::Vbat => Some(&mut self.vbat),
            Channel::Current => Some(&mut self.current),
            Channel::Rssi => Some(&mut self.rssi),
            Channel::Motor(i) => {
                if self.motors.len() <= i {
                    self.motors.resize(i + 1, Vec::new());
                }
                self.motors[i] = samples;
                return self;
            }
        };
        if let Some(slot) = slot {
            *slot = Some(samples);
        }
        self
    }

    pub fn channel(&self, channel: Channel) -> Option<&[f64]> {
        match channel {
            Channel::RawGyro(i) => self.raw_gyro.get(i)?.as_deref(),
            Channel::Gyro(i) => self.gyro.get(i)?.as_deref(),
            Channel::Acceleration(i) => self.acceleration.get(i)?.as_deref(),
            Channel::Setpoint(i) => self.setpoint.get(i)?.as_deref(),
            Channel::RcCommand(i) => self.rc_command.get(i)?.as_deref(),
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
        (0..3).any(|i| self.channel(Channel::Debug(i)).is_some())
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
    pub fn gyro_raw(&self, axis: usize) -> Option<&[f64]> {
        self.channel(Channel::RawGyro(axis))
    }

    /// Post-filter gyro, i.e. `gyroADC` — what the PID loop actually saw.
    pub fn gyro(&self, axis: usize) -> Option<&[f64]> {
        self.channel(Channel::Gyro(axis))
    }

    pub fn setpoint(&self, axis: usize) -> Option<&[f64]> {
        self.channel(Channel::Setpoint(axis))
    }

    pub fn debug(&self, index: usize) -> Option<&[f64]> {
        self.channel(Channel::Debug(index))
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

    /// Betaflight logs throttle as `rcCommand[3]`.
    pub fn throttle(&self) -> Option<&[f64]> {
        self.channel(Channel::RcCommand(3))
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn throttle_is_rc_command_channel_3() {
        let fd =
            FlightData::default().with_channel(Channel::RcCommand(3), vec![0.0, 500.0, 1000.0]);
        assert_eq!(fd.throttle(), Some(&[0.0, 500.0, 1000.0][..]));
    }

    #[test]
    fn throttle_absent_when_not_logged() {
        assert_eq!(FlightData::default().throttle(), None);
    }

    #[test]
    fn raw_and_filtered_gyro_are_separate_channels_per_axis() {
        let fd = FlightData::default()
            .with_channel(Channel::RawGyro(1), vec![10.0])
            .with_channel(Channel::Gyro(1), vec![8.0]);

        assert_eq!(fd.gyro_raw(1), Some(&[10.0][..]));
        assert_eq!(fd.gyro(1), Some(&[8.0][..]));
        assert_eq!(fd.gyro_raw(0), None);
    }

    #[test]
    fn channel_out_of_range_is_absent() {
        assert_eq!(FlightData::default().gyro_raw(9), None);
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
