use egui::{RichText, Ui};

use crate::app::tabs::{GYRO_AXIS_COLORS, GYRO_RAW_COLOR, stacked_plot_height};
use crate::app::ui::timeseries_plot::{Series, TimeseriesPlot};
use crate::parser::{Axis, FlightData};

pub(super) fn show(ui: &mut Ui, fd: &FlightData) {
    let t0 = fd.start_us();
    let duration_s = fd.duration_s().max(f64::MIN_POSITIVE);
    let drawn = Axis::ALL
        .iter()
        .filter(|&&a| fd.gyro_raw(a).is_some())
        .count();
    let plot_height = stacked_plot_height(ui, drawn);

    for axis in Axis::ALL {
        let Some(raw) = fd.gyro_raw(axis) else {
            continue;
        };

        ui.label(RichText::new(axis.name()).strong());

        let mut series = vec![Series {
            label: format!("{} (raw)", axis.name()),
            color: GYRO_RAW_COLOR,
            time_us: fd.time_us(),
            samples: raw,
        }];

        if let Some(filtered) = fd.gyro(axis) {
            series.push(Series {
                label: format!("{} (filtered)", axis.name()),
                color: GYRO_AXIS_COLORS[axis],
                time_us: fd.time_us(),
                samples: filtered,
            });
        }

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
