use egui::{Color32, RichText, Ui};

use crate::app::tabs::{GYRO_AXIS_COLORS, stacked_plot_height};
use crate::app::ui::timeseries_plot::{Series, TimeseriesPlot};
use crate::parser::{Axis, FlightData};

const SETPOINT_COLOR: Color32 = Color32::WHITE;

pub(super) fn show(ui: &mut Ui, fd: &FlightData) {
    let t0 = fd.start_us();
    let duration_s = fd.duration_s().max(f64::MIN_POSITIVE);
    let plot_height = stacked_plot_height(ui, 3);

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
