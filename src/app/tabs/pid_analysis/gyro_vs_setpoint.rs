use egui::{Color32, RichText, Ui};

use crate::app::tabs::{GYRO_AXIS_COLORS, stacked_plot_height};
use crate::app::ui::timeseries_plot::{Series, TimeseriesPlot};
use crate::parser::{Axis, FlightData};

const SETPOINT_COLOR: Color32 = Color32::WHITE;

/// The sibling Step Response sub-tab explains every one of its empty exits;
/// this one may not be the odd panel out.
const NO_GYRO: &str = "No gyroADC in this log — nothing recorded how the craft answered the \
                       sticks. Enable the Gyro field in Betaflight's Blackbox tab and fly again.";

pub(super) fn show(ui: &mut Ui, fd: &FlightData) {
    let t0 = fd.start_us();
    let duration_s = fd.duration_s().max(f64::MIN_POSITIVE);
    let drawn = Axis::ALL.iter().filter(|&&a| fd.gyro(a).is_some()).count();
    if drawn == 0 {
        ui.label(NO_GYRO);
        return;
    }

    let plot_height = stacked_plot_height(ui, drawn);

    for axis in Axis::ALL {
        let Some(gyro) = fd.gyro(axis) else {
            continue;
        };

        ui.label(RichText::new(axis.name()).strong());

        let mut series = vec![Series {
            label: format!("{} (gyro)", axis.name()),
            color: GYRO_AXIS_COLORS[axis],
            time_us: fd.time_us(),
            samples: gyro,
        }];

        if let Some(setpoint) = fd.setpoint(axis) {
            series.push(Series {
                label: format!("{} (setpoint)", axis.name()),
                color: SETPOINT_COLOR,
                time_us: fd.time_us(),
                samples: setpoint,
            });
        }

        TimeseriesPlot {
            id: format!("gyro_vs_setpoint_plot_{}", axis.name()),
            y_label: "deg/s".to_string(),
            t0,
            series,
            default_x_range: Some((0.0, duration_s)),
            height: Some(plot_height),
        }
        .show(ui);
    }
}
