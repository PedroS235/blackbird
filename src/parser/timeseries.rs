#[derive(Debug, Default)]
pub struct FlightData {
    /// Microsecond timestamps, one per main frame.
    pub time_us: Vec<u64>,

    // Pre-filter gyro [deg/s]
    pub gyro_unfilt_roll: Option<Vec<f32>>,
    pub gyro_unfilt_pitch: Option<Vec<f32>>,
    pub gyro_unfilt_yaw: Option<Vec<f32>>,

    // Post-filter gyro [deg/s]
    pub gyro_adc_roll: Option<Vec<f32>>,
    pub gyro_adc_pitch: Option<Vec<f32>>,
    pub gyro_adc_yaw: Option<Vec<f32>>,

    // Setpoint [roll, pitch, yaw, throttle]
    pub setpoint_roll: Option<Vec<f32>>,
    pub setpoint_pitch: Option<Vec<f32>>,
    pub setpoint_yaw: Option<Vec<f32>>,
    pub setpoint_throttle: Option<Vec<f32>>,

    // RC commands [roll, pitch, yaw, throttle]
    pub rc_command_roll: Option<Vec<f32>>,
    pub rc_command_pitch: Option<Vec<f32>>,
    pub rc_command_yaw: Option<Vec<f32>>,
    pub rc_command_throttle: Option<Vec<f32>>,

    /// Per-motor output timeseries; `motor[i]` = motor i across time.
    pub motor: Vec<Vec<f32>>,
}
