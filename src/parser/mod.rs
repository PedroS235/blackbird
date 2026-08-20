mod flight_data;
pub mod metadata;
mod sample_rate;

use flight_data::set_at;
pub use flight_data::{Axis, Channel, FlightData, PerAxis, Trimmed};
pub use metadata::Metadata;
pub use sample_rate::SampleRateEstimate;

use blackbox_log::frame::{Frame as _, FrameDef as _, MainValue};
use blackbox_log::{ParserEvent, headers};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Default)]
pub struct ParsedLog {
    pub metadata: Metadata,
    pub flight_data: FlightData,
    pub log_index: usize,
}

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("Failed to read file: {0}")]
    Io(String),
    #[error("Log index {index:?} out of range (file has {count:?} logs)")]
    InvalidLogIndex { index: usize, count: usize },
    #[error("Failed to read header from log: {0}")]
    InvalidHeader(String),
    #[error("Firmware version {0} is not yet supported")]
    UnsupportedFirmwareVersion(String),
    #[error("Firmware {0} is not supported")]
    UnsupportedFirmware(String),
    #[error("Corrupt log: {0}")]
    Corrupt(String),
    #[error("Parse cancelled")]
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct LogFile {
    bytes: Arc<Vec<u8>>,
    pub file_name: String,
}

impl LogFile {
    pub fn open(path: &Path) -> Result<Self, ParseError> {
        let bytes = std::fs::read(path).map_err(|e| ParseError::Io(e.to_string()))?;
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        Ok(Self {
            bytes: Arc::new(bytes),
            file_name,
        })
    }

    pub fn log_count(&self) -> usize {
        blackbox_log::File::new(&self.bytes).log_count()
    }

    pub fn parse_logs(&self) -> Result<Vec<ParsedLog>, ParseError> {
        (0..self.log_count()).map(|i| self.parse_log(i)).collect()
    }

    pub fn parse_log(&self, log_index: usize) -> Result<ParsedLog, ParseError> {
        self.parse_log_with_progress(log_index, |_| true)
    }

    /// `on_progress` is called with the fraction of this log's data decoded so
    /// far (0..=1) every few thousand frames — often enough to drive a
    /// progress bar, rare enough to be free. Returning `false` abandons the
    /// parse with [`ParseError::Cancelled`].
    pub fn parse_log_with_progress(
        &self,
        log_index: usize,
        on_progress: impl FnMut(f32) -> bool,
    ) -> Result<ParsedLog, ParseError> {
        let file = blackbox_log::File::new(&self.bytes);
        let count = file.log_count();

        let Some(parsed_header) = file.parse(log_index) else {
            return Err(ParseError::InvalidLogIndex {
                index: log_index,
                count,
            });
        };

        match parsed_header {
            Ok(header) => {
                let mut metadata = build_metadata(&header, &self.file_name);
                let mut parser = header.data_parser();

                let field_names: Vec<String> = parser
                    .main_frame_def()
                    .iter()
                    .map(|f| f.name.to_owned())
                    .collect();

                let flight_data = build_flight_data(&mut parser, &field_names, on_progress)
                    .ok_or(ParseError::Cancelled)?;
                metadata.duration = flight_data
                    .time_us
                    .first()
                    .zip(flight_data.time_us.last())
                    .map(|(f, l)| Duration::from_micros(l.saturating_sub(*f)))
                    .unwrap_or(Duration::ZERO);

                Ok(ParsedLog {
                    metadata,
                    flight_data,
                    log_index,
                })
            }
            Err(e) => match e {
                headers::ParseError::UnsupportedFirmwareVersion(firmware) => {
                    Err(ParseError::UnsupportedFirmwareVersion(format!(
                        "{} {}",
                        firmware.name(),
                        firmware.version()
                    )))
                }
                headers::ParseError::InvalidFirmware(rev) => {
                    Err(ParseError::UnsupportedFirmware(rev))
                }
                headers::ParseError::InvalidHeader { header, value: _ } => {
                    Err(ParseError::InvalidHeader(header))
                }
                e => Err(ParseError::Corrupt(e.to_string())),
            },
        }
    }
}

fn build_metadata(headers: &blackbox_log::Headers<'_>, file_name: &str) -> Metadata {
    let craft_name = headers.craft_name().unwrap_or("Unknown").to_owned();
    let board = headers.board_info().unwrap_or("Unknown").to_owned();
    let firmware = {
        let fw = headers.firmware();
        format!("{} {}", fw.name(), fw.version())
    };
    let raw_headers: std::collections::HashMap<String, String> = headers
        .unknown()
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let looptime_us = raw_headers
        .get("looptime")
        .and_then(|v| v.trim().parse::<u32>().ok())
        .filter(|&t| t > 0);
    let filters = parse_filter_config(&raw_headers);
    let rates = parse_rate_config(&raw_headers);
    let debug_mode = headers.debug_mode().to_string();

    Metadata {
        file_name: file_name.to_owned(),
        craft_name,
        firmware,
        board,
        looptime_us,
        duration: Duration::ZERO,
        filters,
        rates,
        debug_mode,
        raw_headers,
    }
}

/// `None` when the log records no rate curve — an older or partial header set,
/// where zeroes would read as the pilot's actual rates.
fn parse_rate_config(
    h: &std::collections::HashMap<String, String>,
) -> Option<metadata::RateConfig> {
    let triple = |key: &str| -> Option<[f32; 3]> {
        let mut parts = h.get(key)?.split(',').map(|s| s.trim().parse().ok());
        Some([parts.next()??, parts.next()??, parts.next()??])
    };

    // A log without `rates_type` predates the setting, where Betaflight rates
    // were the only curve. One we cannot read is not that — it says so rather
    // than being shown as Betaflight rates the craft may never have flown.
    let rate_type = match h.get("rates_type") {
        None => metadata::RateType::default(),
        Some(raw) => raw.trim().parse().map_or(
            metadata::RateType::Unreadable,
            metadata::RateType::from_bf_code,
        ),
    };

    Some(metadata::RateConfig {
        rate_type,
        rc_rates: triple("rc_rates"),
        rates: triple("rates")?,
        expo: triple("rc_expo"),
    })
}

fn parse_filter_config(h: &std::collections::HashMap<String, String>) -> metadata::FilterConfig {
    metadata::FilterConfig {
        gyro_lpf1: parse_lowpass(
            h,
            "gyro_lpf1_static_hz",
            "gyro_lpf1_dyn_hz",
            "gyro_lpf1_type",
        ),
        gyro_lpf2: parse_static_lowpass(h, "gyro_lpf2_static_hz", "gyro_lpf2_type"),
        dterm_lpf1: parse_dterm_lpf1(h),
        dterm_lpf2: parse_static_lowpass(h, "dterm_lpf2_static_hz", "dterm_lpf2_type"),
        gyro_notches: parse_notches(h, "gyro_notch_hz", "gyro_notch_cutoff"),
        dterm_notches: parse_notches(h, "dterm_notch_hz", "dterm_notch_cutoff"),
        dyn_notch: parse_dyn_notch(h),
        rpm_filter: parse_rpm_filter(h),
    }
}

fn parse_filter_type(
    h: &std::collections::HashMap<String, String>,
    type_key: &str,
) -> metadata::FilterType {
    h.get(type_key)
        .and_then(|v| v.trim().parse::<u8>().ok())
        .map(metadata::FilterType::from_bf_code)
        .unwrap_or_default()
}

fn parse_lowpass(
    h: &std::collections::HashMap<String, String>,
    static_key: &str,
    dyn_key: &str,
    type_key: &str,
) -> Option<metadata::LowpassConfig> {
    let static_hz = h
        .get(static_key)
        .and_then(|v| v.trim().parse::<f32>().ok())
        .unwrap_or(0.0);
    let (dyn_min_hz, dyn_max_hz) = h
        .get(dyn_key)
        .and_then(|v| {
            let mut parts = v.split(',').filter_map(|s| s.trim().parse::<f32>().ok());
            Some((parts.next()?, parts.next()?))
        })
        .unwrap_or((0.0, 0.0));

    if static_hz == 0.0 && dyn_max_hz == 0.0 {
        return None;
    }
    Some(metadata::LowpassConfig {
        static_hz,
        dyn_min_hz,
        dyn_max_hz,
        filter_type: parse_filter_type(h, type_key),
    })
}

fn parse_static_lowpass(
    h: &std::collections::HashMap<String, String>,
    static_key: &str,
    type_key: &str,
) -> Option<metadata::StaticLowpassConfig> {
    let cutoff_hz = h
        .get(static_key)
        .and_then(|v| v.trim().parse::<f32>().ok())
        .unwrap_or(0.0);
    if cutoff_hz == 0.0 {
        return None;
    }
    Some(metadata::StaticLowpassConfig {
        cutoff_hz,
        filter_type: parse_filter_type(h, type_key),
    })
}

fn parse_dterm_lpf1(
    h: &std::collections::HashMap<String, String>,
) -> Option<metadata::DtermLowpass1Config> {
    let lowpass = parse_lowpass(
        h,
        "dterm_lpf1_static_hz",
        "dterm_lpf1_dyn_hz",
        "dterm_lpf1_type",
    )?;
    let dyn_expo = h
        .get("dterm_lpf1_dyn_expo")
        .and_then(|v| v.trim().parse::<f32>().ok())
        .unwrap_or(0.0);
    Some(metadata::DtermLowpass1Config { lowpass, dyn_expo })
}

fn parse_notches(
    h: &std::collections::HashMap<String, String>,
    hz_key: &str,
    cutoff_key: &str,
) -> Vec<metadata::NotchConfig> {
    let parse_list = |key: &str| -> Vec<f32> {
        h.get(key)
            .map(|v| v.split(',').filter_map(|s| s.trim().parse().ok()).collect())
            .unwrap_or_default()
    };

    parse_list(hz_key)
        .into_iter()
        .zip(parse_list(cutoff_key))
        .filter(|&(center_hz, _)| center_hz > 0.0)
        .map(|(center_hz, cutoff_hz)| metadata::NotchConfig {
            center_hz,
            cutoff_hz,
        })
        .collect()
}

fn parse_dyn_notch(
    h: &std::collections::HashMap<String, String>,
) -> Option<metadata::DynNotchConfig> {
    let count = h.get("dyn_notch_count")?.trim().parse::<u32>().ok()?;
    if count == 0 {
        return None;
    }
    let min_hz = h
        .get("dyn_notch_min_hz")
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(90.0);
    let max_hz = h
        .get("dyn_notch_max_hz")
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(400.0);
    let q = h
        .get("dyn_notch_q")
        .and_then(|v| v.trim().parse::<f32>().ok())
        .map(|q100| q100 / 100.0)
        .unwrap_or(3.0);
    Some(metadata::DynNotchConfig {
        min_hz,
        max_hz,
        count,
        q,
    })
}

fn parse_rpm_filter(
    h: &std::collections::HashMap<String, String>,
) -> Option<metadata::RpmFilterConfig> {
    let harmonics = h.get("rpm_filter_harmonics")?.trim().parse::<u32>().ok()?;
    if harmonics == 0 {
        return None;
    }
    let min_hz = h
        .get("rpm_filter_min_hz")
        .and_then(|v| v.trim().parse::<f32>().ok())
        .unwrap_or(150.0);
    let fade_range_hz = h
        .get("rpm_filter_fade_range_hz")
        .and_then(|v| v.trim().parse::<f32>().ok())
        .unwrap_or(50.0);
    let q = h
        .get("rpm_filter_q")
        .and_then(|v| v.trim().parse::<f32>().ok())
        .map(|q100| q100 / 100.0)
        .unwrap_or(5.0);
    let weights = h
        .get("rpm_filter_weights")
        .map(|v| {
            v.split(',')
                .filter_map(|s| s.trim().parse::<f32>().ok())
                .map(|w| w / 100.0)
                .collect()
        })
        .unwrap_or_else(|| vec![1.0; harmonics as usize]);
    Some(metadata::RpmFilterConfig {
        harmonics,
        min_hz,
        fade_range_hz,
        q,
        weights,
    })
}

fn split_field_name(name: &str) -> (&str, usize) {
    name.split_once('[')
        .and_then(|(base, rest)| {
            rest.strip_suffix(']')
                .and_then(|idx| idx.parse().ok())
                .map(|axis| (base, axis))
        })
        .unwrap_or((name, 0))
}

fn main_value_to_f64(value: MainValue) -> f64 {
    use blackbox_log::units::si::{
        acceleration::meter_per_second_squared, angular_velocity::degree_per_second,
        electric_current::ampere, electric_potential::volt,
    };
    match value {
        MainValue::Rotation(r) => r.get::<degree_per_second>(),
        MainValue::Signed(i) => i as f64,
        MainValue::Unsigned(u) => u as f64,
        MainValue::Acceleration(a) => a.get::<meter_per_second_squared>(),
        MainValue::Voltage(v) => v.get::<volt>(),
        MainValue::Amperage(a) => a.get::<ampere>(),
    }
}

#[derive(Debug)]
struct FieldIndices {
    raw_gyro: [Option<usize>; 3],
    gyro: [Option<usize>; 3],
    acceleration: [Option<usize>; 3],
    setpoint: [Option<usize>; 4],
    rc_command: [Option<usize>; 4],
    motors: Vec<Option<usize>>,
    rpm: Vec<Option<usize>>,
    vbat: Option<usize>,
    current: Option<usize>,
    rssi: Option<usize>,
    debug: [Option<usize>; 8],
}

fn build_field_indices(field_names: &[String]) -> FieldIndices {
    let mut idx = FieldIndices {
        raw_gyro: [None; 3],
        gyro: [None; 3],
        acceleration: [None; 3],
        setpoint: [None; 4],
        rc_command: [None; 4],
        motors: Vec::new(),
        rpm: Vec::new(),
        vbat: None,
        current: None,
        rssi: None,
        debug: [None; 8],
    };
    for (col, name) in field_names.iter().enumerate() {
        let (base, axis) = split_field_name(name);
        match base {
            "gyroUnfilt" if axis < 3 => idx.raw_gyro[axis] = Some(col),
            "gyroADC" if axis < 3 => idx.gyro[axis] = Some(col),
            "accSmooth" if axis < 3 => idx.acceleration[axis] = Some(col),
            "setpoint" if axis < 4 => idx.setpoint[axis] = Some(col),
            "rcCommand" if axis < 4 => idx.rc_command[axis] = Some(col),
            "motor" => set_at(&mut idx.motors, axis, Some(col)),
            "eRPM" => set_at(&mut idx.rpm, axis, Some(col)),
            "debug" if axis < 8 => idx.debug[axis] = Some(col),
            "vbatLatest" | "vbat" => idx.vbat = Some(col),
            "amperageLatest" | "amperage" => idx.current = Some(col),
            "rssi" => idx.rssi = Some(col),
            _ => {}
        }
    }
    idx
}

/// Frames between `on_progress` calls — the decode is fast enough that
/// reporting every frame would cost more than the parse.
const PROGRESS_INTERVAL_FRAMES: usize = 4096;

/// `None` when `on_progress` asked to stop.
fn build_flight_data(
    parser: &mut blackbox_log::DataParser<'_, '_>,
    field_names: &[String],
    mut on_progress: impl FnMut(f32) -> bool,
) -> Option<FlightData> {
    let idx = build_field_indices(field_names);

    let mut time_buf: Vec<u64> = Vec::new();
    let mut gyro_unfilt_bufs: [Option<Vec<f64>>; 3] = idx.raw_gyro.map(|o| o.map(|_| Vec::new()));
    let mut gyro_adc_bufs: [Option<Vec<f64>>; 3] = idx.gyro.map(|o| o.map(|_| Vec::new()));
    let mut acc_bufs: [Option<Vec<f64>>; 3] = idx.acceleration.map(|o| o.map(|_| Vec::new()));
    let mut setpoint_bufs: [Option<Vec<f64>>; 4] = idx.setpoint.map(|o| o.map(|_| Vec::new()));
    let mut rc_command_bufs: [Option<Vec<f64>>; 4] = idx.rc_command.map(|o| o.map(|_| Vec::new()));
    let mut motor_bufs: Vec<Vec<f64>> = vec![Vec::new(); idx.motors.len()];
    let mut rpm_bufs: Vec<Vec<f64>> = vec![Vec::new(); idx.rpm.len()];
    let mut vbat_buf: Option<Vec<f64>> = idx.vbat.map(|_| Vec::new());
    let mut current_buf: Option<Vec<f64>> = idx.current.map(|_| Vec::new());
    let mut rssi_buf: Option<Vec<f64>> = idx.rssi.map(|_| Vec::new());
    let mut debug_bufs: [Option<Vec<f64>>; 8] = idx.debug.map(|o| o.map(|_| Vec::new()));

    let mut vals: Vec<f64> = Vec::with_capacity(field_names.len());
    while let Some(event) = parser.next() {
        let ParserEvent::Main(frame) = event else {
            continue;
        };

        time_buf.push(frame.time_raw());
        vals.clear();
        vals.extend(frame.iter().map(main_value_to_f64));

        let push = |buf: &mut Option<Vec<f64>>, col: Option<usize>| {
            if let Some((b, c)) = buf.as_mut().zip(col) {
                b.push(vals[c]);
            }
        };
        gyro_unfilt_bufs
            .iter_mut()
            .zip(idx.raw_gyro)
            .for_each(|(b, c)| push(b, c));
        gyro_adc_bufs
            .iter_mut()
            .zip(idx.gyro)
            .for_each(|(b, c)| push(b, c));
        acc_bufs
            .iter_mut()
            .zip(idx.acceleration)
            .for_each(|(b, c)| push(b, c));
        setpoint_bufs
            .iter_mut()
            .zip(idx.setpoint)
            .for_each(|(b, c)| push(b, c));
        rc_command_bufs
            .iter_mut()
            .zip(idx.rc_command)
            .for_each(|(b, c)| push(b, c));
        let push_indexed = |bufs: &mut Vec<Vec<f64>>, cols: &[Option<usize>]| {
            bufs.iter_mut().zip(cols).for_each(|(buf, col)| {
                if let Some(c) = *col {
                    buf.push(vals[c]);
                }
            });
        };
        push_indexed(&mut motor_bufs, &idx.motors);
        push_indexed(&mut rpm_bufs, &idx.rpm);
        push(&mut vbat_buf, idx.vbat);
        push(&mut current_buf, idx.current);
        push(&mut rssi_buf, idx.rssi);
        debug_bufs
            .iter_mut()
            .zip(idx.debug)
            .for_each(|(b, c)| push(b, c));

        if time_buf.len().is_multiple_of(PROGRESS_INTERVAL_FRAMES)
            && !on_progress(parser.stats().progress)
        {
            return None;
        }
    }

    let sample_rate = SampleRateEstimate::from_timestamps(&time_buf);

    // Betaflight logs rssi on its internal 0..=1023 scale, not 0..=100.
    if let Some(buf) = &mut rssi_buf {
        for v in buf.iter_mut() {
            *v = *v * 100.0 / 1023.0;
        }
    }

    Some(FlightData {
        time_us: Arc::new(time_buf),
        sample_rate,
        raw_gyro: gyro_unfilt_bufs,
        gyro: gyro_adc_bufs,
        acceleration: acc_bufs,
        setpoint: setpoint_bufs,
        rc_command: rc_command_bufs,
        motors: motor_bufs,
        rpm: rpm_bufs,
        vbat: vbat_buf,
        current: current_buf,
        rssi: rssi_buf,
        debug: debug_bufs,
    })
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::parser::{LogFile, ParseError};
    use std::collections::HashMap;
    use std::{io::Write, path::Path};

    #[test]
    fn test_valid_path() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(b"H Product:Blackbox flight data recorder by Nicholas Sherlock")
            .unwrap();

        let path = file.path();
        let log_file = LogFile::open(path).unwrap();

        let expected_name = path.file_name().unwrap().to_string_lossy();
        assert_eq!(log_file.file_name, expected_name.as_ref());
    }

    #[test]
    fn test_invalid_path() {
        let err = LogFile::open(Path::new("/nonexistent/file.bbl")).unwrap_err();
        assert!(matches!(err, ParseError::Io(_)));
    }

    #[test]
    fn split_field_name_with_index() {
        assert_eq!(split_field_name("gyroADC[0]"), ("gyroADC", 0));
        assert_eq!(split_field_name("motor[3]"), ("motor", 3));
        assert_eq!(split_field_name("debug[7]"), ("debug", 7));
    }

    /// Without bidirectional DShot there is no `eRPM` at all, which is what
    /// greys the harmonics overlay out rather than drawing bands at zero.
    #[test]
    fn field_indices_maps_erpm_per_motor() {
        let names: Vec<String> = vec![
            "motor[0]".into(),
            "eRPM[0]".into(),
            "eRPM[1]".into(),
            "eRPM[3]".into(),
        ];
        let idx = build_field_indices(&names);

        assert_eq!(idx.rpm, vec![Some(1), Some(2), None, Some(3)]);
        assert!(
            build_field_indices(&["motor[0]".to_string()])
                .rpm
                .is_empty()
        );
    }

    #[test]
    fn split_field_name_no_index() {
        assert_eq!(split_field_name("vbatLatest"), ("vbatLatest", 0));
    }

    #[test]
    fn rpm_filter_none_when_missing() {
        assert!(parse_rpm_filter(&HashMap::new()).is_none());
    }

    #[test]
    fn rpm_filter_none_when_harmonics_zero() {
        let h = HashMap::from([("rpm_filter_harmonics".into(), "0".into())]);
        assert!(parse_rpm_filter(&h).is_none());
    }

    #[test]
    fn rpm_filter_parses_values() {
        let h = HashMap::from([
            ("rpm_filter_harmonics".into(), "3".into()),
            ("rpm_filter_min_hz".into(), "100".into()),
            ("rpm_filter_q".into(), "500".into()),
        ]);
        let cfg = parse_rpm_filter(&h).unwrap();
        assert_eq!(cfg.harmonics, 3);
        assert_eq!(cfg.min_hz, 100.0);
        assert_eq!(cfg.q, 5.0);
    }

    /// A rate type this build does not know renders as its code rather than
    /// being guessed at as one of the curves we do know.
    #[test]
    fn an_unrecognised_rate_type_keeps_its_code() {
        let h = HashMap::from([
            ("rates_type".into(), "9".into()),
            ("rates".into(), "67,67,67".into()),
        ]);
        let cfg = parse_rate_config(&h).unwrap();

        assert_eq!(cfg.rate_type, metadata::RateType::Unknown(9));
        assert_eq!(cfg.to_string(), "rates type 9 67/67/67");
    }

    /// A rate type that is not a code at all is not silently read as the
    /// default curve — the pilot is told it could not be read.
    #[test]
    fn an_unreadable_rate_type_is_not_shown_as_betaflight_rates() {
        let h = HashMap::from([
            ("rates_type".into(), "ACTUAL".into()),
            ("rates".into(), "67,67,67".into()),
        ]);

        assert_eq!(
            parse_rate_config(&h).unwrap().rate_type,
            metadata::RateType::Unreadable
        );
    }

    /// Rate values the log never recorded stay absent rather than becoming
    /// zeroes indistinguishable from a craft configured at zero.
    #[test]
    fn rate_values_the_log_omits_are_absent_rather_than_zero() {
        let h = HashMap::from([("rates".into(), "67,67,67".into())]);
        let cfg = parse_rate_config(&h).unwrap();

        assert_eq!(cfg.rc_rates, None);
        assert_eq!(cfg.expo, None);
        assert_eq!(cfg.rates, [67.0, 67.0, 67.0]);
    }

    #[test]
    fn rate_config_none_without_rate_headers() {
        assert!(parse_rate_config(&HashMap::new()).is_none());
    }

    #[test]
    fn sample_rate_empty_timestamps() {
        let s = SampleRateEstimate::from_timestamps(&[]);
        assert_eq!(s.rate_hz, 0.0);
    }

    #[test]
    fn sample_rate_uniform_8khz() {
        let times: Vec<u64> = (0..100).map(|i| i * 125).collect();
        let s = SampleRateEstimate::from_timestamps(&times);
        assert!((s.rate_hz - 8000.0).abs() < 1.0);
        assert_eq!(s.median_dt, 125);
        assert_eq!(s.jitter_std, 0.0);
    }

    #[test]
    fn sample_rate_from_looptime_zero() {
        let s = SampleRateEstimate::from_looptime(0);
        assert_eq!(s.rate_hz, 0.0);
    }

    #[test]
    fn field_indices_maps_gyro_channels() {
        let names: Vec<String> = vec![
            "gyroUnfilt[0]".into(),
            "gyroUnfilt[1]".into(),
            "gyroUnfilt[2]".into(),
            "gyroADC[0]".into(),
        ];
        let idx = build_field_indices(&names);
        assert_eq!(idx.raw_gyro, [Some(0), Some(1), Some(2)]);
        assert_eq!(idx.gyro[0], Some(3));
    }

    fn fixture(name: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    #[test]
    fn bbl_log_count() {
        let lf = LogFile::open(&fixture("eight_logs_in_one.bbl")).unwrap();
        assert_eq!(lf.log_count(), 8);
    }

    #[test]
    #[ignore]
    fn bbl_all_logs_parse() {
        let lf = LogFile::open(&fixture("eight_logs_in_one.bbl")).unwrap();
        assert!(lf.parse_logs().is_ok());
    }

    #[test]
    #[ignore]
    fn bbl_metadata() {
        let lf = LogFile::open(&fixture("eight_logs_in_one.bbl")).unwrap();
        let log = lf.parse_log(0).unwrap();
        assert_eq!(log.metadata.firmware, "Betaflight 4.5.1");
        assert_eq!(log.metadata.board, "GEPR GEPRC_F722_AIO");
        assert_eq!(log.metadata.file_name, "eight_logs_in_one.bbl");
    }

    #[test]
    fn bbl_out_of_range() {
        let lf = LogFile::open(&fixture("eight_logs_in_one.bbl")).unwrap();
        let err = lf.parse_log(8).unwrap_err();
        assert!(matches!(
            err,
            ParseError::InvalidLogIndex { index: 8, count: 8 }
        ));
    }

    #[test]
    fn bfl_metadata() {
        let lf = LogFile::open(&fixture("new202612_BF_steadyhover.BFL")).unwrap();
        let log = lf.parse_log(0).unwrap();
        assert_eq!(log.metadata.craft_name, "Mario 5");
        assert_eq!(log.metadata.firmware, "Betaflight 2025.12.2");
        assert_eq!(log.metadata.board, "SPBE SPEEDYBEEF7V3");
    }

    #[test]
    fn bfl_duration_nonzero() {
        let lf = LogFile::open(&fixture("new202612_BF_steadyhover.BFL")).unwrap();
        let log = lf.parse_log(0).unwrap();
        assert!(log.metadata.duration.as_secs() > 0);
    }
}
