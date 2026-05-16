mod header;
mod timeseries;

pub use header::HeaderData;
pub use timeseries::FlightData;

use blackbox_log::frame::{Frame as _, FrameDef as _, MainValue};
use blackbox_log::{Filter, FilterSet, ParserEvent};
use thiserror::Error;

#[derive(Debug)]
pub struct ParsedLog {
    pub header: HeaderData,
    pub data: FlightData,
    pub log_index: usize,
}

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("Log index {index:?} out of range (file has {count:?} logs)")]
    InvalidLogIndex { index: usize, count: usize },
    #[error("Corrupt log: {0}")]
    Corrupt(String),
}

// INFO: Fields of interest
const FIELDS: [&str; 5] = ["gyroUnfilt", "gyroADC", "setpoint", "rcCommand", "motor"];

pub fn log_count(bytes: &[u8]) -> usize {
    blackbox_log::File::new(bytes).log_count()
}

pub fn parse(bytes: &[u8], log_index: usize, on_progress: impl Fn(usize)) -> Result<ParsedLog, ParseError> {
    let file = blackbox_log::File::new(bytes);
    let count = file.log_count();

    if log_index >= count {
        return Err(ParseError::InvalidLogIndex {
            index: log_index,
            count,
        });
    }

    let headers = file
        .parse(log_index)
        .unwrap() // safe: index checked above
        .map_err(|e| ParseError::Corrupt(e.to_string()))?;

    let header = build_header(&headers);

    let filters = FilterSet {
        main: Filter::OnlyFields(FIELDS.into()),
        slow: Filter::only_required(),
        gps: Filter::only_required(),
    };
    let mut parser = headers.data_parser_with_filters(&filters);

    let field_names: Vec<String> = parser
        .main_frame_def()
        .iter()
        .map(|f| f.name.to_owned())
        .collect();

    let data = build_flight_data(&mut parser, &field_names, &on_progress);

    Ok(ParsedLog {
        header,
        data,
        log_index,
    })
}

fn build_header(headers: &blackbox_log::Headers<'_>) -> HeaderData {
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
    let sample_rate_hz = raw_headers
        .get("looptime")
        .and_then(|v| v.trim().parse::<f32>().ok())
        .filter(|&t| t > 0.0)
        .map(|looptime_us| 1_000_000.0 / looptime_us);
    let rpm_filter = parse_rpm_filter(&raw_headers);

    HeaderData {
        craft_name,
        firmware,
        board,
        sample_rate_hz,
        rpm_filter,
        raw_headers,
    }
}

fn parse_rpm_filter(h: &std::collections::HashMap<String, String>) -> Option<header::RpmFilterConfig> {
    let harmonics = h.get("rpm_filter_harmonics")?.trim().parse::<u32>().ok()?;
    if harmonics == 0 {
        return None;
    }
    let min_hz = h.get("rpm_filter_min_hz")
        .and_then(|v| v.trim().parse::<f32>().ok())
        .unwrap_or(150.0);
    let fade_range_hz = h.get("rpm_filter_fade_range_hz")
        .and_then(|v| v.trim().parse::<f32>().ok())
        .unwrap_or(50.0);
    let q = h.get("rpm_filter_q")
        .and_then(|v| v.trim().parse::<f32>().ok())
        .map(|q100| q100 / 100.0)
        .unwrap_or(5.0);
    let weights = h.get("rpm_filter_weights")
        .map(|v| {
            v.split(',')
                .filter_map(|s| s.trim().parse::<f32>().ok())
                .map(|w| w / 100.0)
                .collect()
        })
        .unwrap_or_else(|| vec![1.0; harmonics as usize]);
    Some(header::RpmFilterConfig { harmonics, min_hz, fade_range_hz, q, weights })
}

/// Strips `[N]` from field names, returning `(base, axis_index)`.
fn split_field_name(name: &str) -> (&str, usize) {
    name.split_once('[')
        .and_then(|(base, rest)| {
            rest.strip_suffix(']')
                .and_then(|idx| idx.parse().ok())
                .map(|axis| (base, axis))
        })
        .unwrap_or((name, 0))
}

fn main_value_to_f32(value: MainValue) -> f32 {
    use blackbox_log::units::si::{
        acceleration::meter_per_second_squared, angular_velocity::degree_per_second,
        electric_current::ampere, electric_potential::volt,
    };
    match value {
        MainValue::Rotation(r) => r.get::<degree_per_second>() as f32,
        MainValue::Signed(i) => i as f32,
        MainValue::Unsigned(u) => u as f32,
        MainValue::Acceleration(a) => a.get::<meter_per_second_squared>() as f32,
        MainValue::Voltage(v) => v.get::<volt>() as f32,
        MainValue::Amperage(a) => a.get::<ampere>() as f32,
    }
}

struct FieldIndices {
    gyro_unfilt: [Option<usize>; 3],
    gyro_adc: [Option<usize>; 3],
    setpoint: [Option<usize>; 4],
    rc_command: [Option<usize>; 4],
    motor: Vec<Option<usize>>,
}

fn build_field_indices(field_names: &[String]) -> FieldIndices {
    let mut idx = FieldIndices {
        gyro_unfilt: [None; 3],
        gyro_adc: [None; 3],
        setpoint: [None; 4],
        rc_command: [None; 4],
        motor: Vec::new(),
    };
    for (col, name) in field_names.iter().enumerate() {
        let (base, axis) = split_field_name(name);
        match base {
            "gyroUnfilt" if axis < 3 => idx.gyro_unfilt[axis] = Some(col),
            "gyroADC" if axis < 3 => idx.gyro_adc[axis] = Some(col),
            "setpoint" if axis < 4 => idx.setpoint[axis] = Some(col),
            "rcCommand" if axis < 4 => idx.rc_command[axis] = Some(col),
            "motor" => {
                if idx.motor.len() <= axis {
                    idx.motor.resize(axis + 1, None);
                }
                idx.motor[axis] = Some(col);
            }
            _ => {}
        }
    }
    idx
}

fn build_flight_data(
    parser: &mut blackbox_log::DataParser<'_, '_>,
    field_names: &[String],
    on_progress: &impl Fn(usize),
) -> FlightData {
    let idx = build_field_indices(field_names);
    let motor_count = idx.motor.len();

    let mut data = FlightData {
        motor: vec![Vec::new(); motor_count],
        ..FlightData::default()
    };

    macro_rules! init_opt_vecs {
        ($guard:expr, $($field:ident),+) => {
            if $guard {
                $( data.$field = Some(Vec::new()); )+
            }
        };
    }

    init_opt_vecs!(
        idx.gyro_unfilt[0].is_some(),
        gyro_unfilt_roll,
        gyro_unfilt_pitch,
        gyro_unfilt_yaw
    );
    init_opt_vecs!(
        idx.gyro_adc[0].is_some(),
        gyro_adc_roll,
        gyro_adc_pitch,
        gyro_adc_yaw
    );
    init_opt_vecs!(
        idx.setpoint[0].is_some(),
        setpoint_roll,
        setpoint_pitch,
        setpoint_yaw,
        setpoint_throttle
    );
    init_opt_vecs!(
        idx.rc_command[0].is_some(),
        rc_command_roll,
        rc_command_pitch,
        rc_command_yaw,
        rc_command_throttle
    );

    let mut frame_count = 0usize;
    while let Some(event) = parser.next() {
        let ParserEvent::Main(frame) = event else {
            continue;
        };

        frame_count += 1;
        if frame_count % 5_000 == 0 {
            on_progress(frame_count);
        }

        data.time_us.push(frame.time_raw());
        let vals: Vec<f32> = frame.iter().map(main_value_to_f32).collect();

        macro_rules! push {
            ($vec:expr, $col:expr) => {
                if let (Some(v), Some(i)) = ($vec.as_mut(), $col) {
                    v.push(vals[i]);
                }
            };
        }

        push!(data.gyro_unfilt_roll, idx.gyro_unfilt[0]);
        push!(data.gyro_unfilt_pitch, idx.gyro_unfilt[1]);
        push!(data.gyro_unfilt_yaw, idx.gyro_unfilt[2]);

        push!(data.gyro_adc_roll, idx.gyro_adc[0]);
        push!(data.gyro_adc_pitch, idx.gyro_adc[1]);
        push!(data.gyro_adc_yaw, idx.gyro_adc[2]);

        push!(data.setpoint_roll, idx.setpoint[0]);
        push!(data.setpoint_pitch, idx.setpoint[1]);
        push!(data.setpoint_yaw, idx.setpoint[2]);
        push!(data.setpoint_throttle, idx.setpoint[3]);

        push!(data.rc_command_roll, idx.rc_command[0]);
        push!(data.rc_command_pitch, idx.rc_command[1]);
        push!(data.rc_command_yaw, idx.rc_command[2]);
        push!(data.rc_command_throttle, idx.rc_command[3]);

        for (i, motor_col) in idx.motor.iter().enumerate() {
            if let Some(col) = motor_col {
                data.motor[i].push(vals[*col]);
            }
        }
    }

    data
}
