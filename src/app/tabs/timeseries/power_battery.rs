use egui::{Color32, RichText, Ui};

use crate::app::tabs::stacked_plot_height;
use crate::app::ui::timeseries_plot::{Series, TimeseriesPlot};
use crate::parser::FlightData;

const VBAT_COLOR: Color32 = Color32::from_rgb(255, 202, 40);
const CURRENT_COLOR: Color32 = Color32::from_rgb(255, 111, 97);

pub(super) fn show(ui: &mut Ui, fd: &FlightData) {
    let t0 = fd.start_us();
    let duration_s = fd.duration_s().max(f64::MIN_POSITIVE);

    let metric_count = [fd.vbat().is_some(), fd.current().is_some()]
        .iter()
        .filter(|present| **present)
        .count();
    let plot_height = stacked_plot_height(ui, metric_count);

    if let Some(vbat) = fd.vbat() {
        ui.label(RichText::new("Battery Voltage").strong());
        TimeseriesPlot {
            id: "power_vbat_plot".to_string(),
            y_label: "V".to_string(),
            t0,
            series: vec![Series {
                label: "vbat".to_string(),
                color: VBAT_COLOR,
                time_us: fd.time_us(),
                samples: vbat,
            }],
            default_x_range: Some((0.0, duration_s)),
            height: Some(plot_height),
        }
        .show(ui);
    }

    if let Some(current) = fd.current() {
        ui.label(RichText::new("Current").strong());
        TimeseriesPlot {
            id: "power_current_plot".to_string(),
            y_label: "A".to_string(),
            t0,
            series: vec![Series {
                label: "current".to_string(),
                color: CURRENT_COLOR,
                time_us: fd.time_us(),
                samples: current,
            }],
            default_x_range: Some((0.0, duration_s)),
            height: Some(plot_height),
        }
        .show(ui);
    }
}
