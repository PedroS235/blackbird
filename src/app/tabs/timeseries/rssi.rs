use egui::{Color32, RichText, Ui};

use crate::app::ui::timeseries_plot::{Series, TimeseriesPlot};
use crate::parser::FlightData;

const RSSI_COLOR: Color32 = Color32::from_rgb(100, 200, 255);

pub(super) fn show(ui: &mut Ui, fd: &FlightData) {
    let Some(rssi) = fd.rssi() else {
        return;
    };

    ui.label(RichText::new("RSSI").strong());
    TimeseriesPlot {
        id: "rssi_plot".to_string(),
        y_label: "%".to_string(),
        t0: fd.start_us(),
        series: vec![Series {
            label: "rssi".to_string(),
            color: RSSI_COLOR,
            time_us: fd.time_us(),
            samples: rssi,
        }],
        default_x_range: Some((0.0, fd.duration_s().max(f64::MIN_POSITIVE))),
        height: Some((ui.available_height() - 24.0).max(80.0)),
    }
    .show(ui);
}
