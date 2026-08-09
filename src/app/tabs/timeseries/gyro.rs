use egui::{RichText, Ui};

use crate::app::tabs::{GYRO_AXIS_COLORS, GYRO_RAW_COLOR, stacked_plot_height};
use crate::app::ui::timeseries_plot::{Series, TimeseriesPlot};
use crate::parser::{Axis, FlightData};

/// Betaflight only records the pre-filter gyro under one debug mode, so a log
/// without it is ordinary rather than broken. This tab is also where `resolve`
/// sends a pilot whose log lacks power or RSSI, so it draws what it has and
/// names what is missing instead of going blank.
const NO_RAW_GYRO: &str = "No gyroUnfilt in this log — these are the filtered traces, which is what \
                           the PID loop saw. Betaflight records the pre-filter gyro only in debug \
                           mode GYRO_SCALED (`set debug_mode = GYRO_SCALED`, or the Debug mode \
                           dropdown in the configurator's Blackbox tab); fly again with it on to \
                           see what the filters are removing.";

const NO_GYRO: &str = "No gyro in this log — neither gyroUnfilt nor gyroADC was recorded, so \
                       there is nothing to draw. Enable the Gyro field in Betaflight's Blackbox \
                       tab and fly again.";

/// Which gyro traces the log can put on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Traces {
    /// Pre- and post-filter together — the full picture.
    RawAndFiltered,
    /// `gyroUnfilt` was never recorded; the filtered trace stands in alone.
    FilteredOnly,
    Nothing,
}

fn traces(fd: &FlightData) -> Traces {
    let raw = Axis::ALL.iter().any(|&axis| fd.gyro_raw(axis).is_some());
    let filtered = Axis::ALL.iter().any(|&axis| fd.gyro(axis).is_some());

    match (raw, filtered) {
        (true, _) => Traces::RawAndFiltered,
        (false, true) => Traces::FilteredOnly,
        (false, false) => Traces::Nothing,
    }
}

pub(super) fn show(ui: &mut Ui, fd: &FlightData) {
    match traces(fd) {
        Traces::Nothing => {
            ui.label(NO_GYRO);
            return;
        }
        Traces::FilteredOnly => {
            ui.label(RichText::new(NO_RAW_GYRO).weak());
            ui.add_space(4.0);
        }
        Traces::RawAndFiltered => {}
    }

    let t0 = fd.start_us();
    let duration_s = fd.duration_s().max(f64::MIN_POSITIVE);
    let drawn = Axis::ALL
        .iter()
        .filter(|&&a| fd.gyro_raw(a).is_some() || fd.gyro(a).is_some())
        .count();
    let plot_height = stacked_plot_height(ui, drawn);

    for axis in Axis::ALL {
        let mut series = Vec::new();

        if let Some(raw) = fd.gyro_raw(axis) {
            series.push(Series {
                label: format!("{} (raw)", axis.name()),
                color: GYRO_RAW_COLOR,
                time_us: fd.time_us(),
                samples: raw,
            });
        }

        if let Some(filtered) = fd.gyro(axis) {
            series.push(Series {
                label: format!("{} (filtered)", axis.name()),
                color: GYRO_AXIS_COLORS[axis],
                time_us: fd.time_us(),
                samples: filtered,
            });
        }

        if series.is_empty() {
            continue;
        }

        ui.label(RichText::new(axis.name()).strong());
        TimeseriesPlot {
            id: format!("gyro_plot_{}", axis.name()),
            y_label: "deg/s".to_string(),
            t0,
            series,
            default_x_range: Some((0.0, duration_s)),
            height: Some(plot_height),
        }
        .show(ui);
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::parser::Channel;

    #[test]
    fn a_log_with_both_gyros_draws_both() {
        let fd = FlightData::default()
            .with_channel(Channel::RawGyro(Axis::Roll), vec![1.0])
            .with_channel(Channel::Gyro(Axis::Roll), vec![0.9]);

        assert_eq!(traces(&fd), Traces::RawAndFiltered);
    }

    /// The regression: this used to draw nothing at all.
    #[test]
    fn a_log_without_gyrounfilt_falls_back_to_the_filtered_trace() {
        let fd = FlightData::default().with_channel(Channel::Gyro(Axis::Roll), vec![0.9]);

        assert_eq!(traces(&fd), Traces::FilteredOnly);
    }

    #[test]
    fn a_log_with_no_gyro_at_all_has_nothing_to_draw() {
        let fd = FlightData::default().with_channel(Channel::Throttle, vec![500.0]);

        assert_eq!(traces(&fd), Traces::Nothing);
    }
}
