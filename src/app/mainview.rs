use egui::Color32;

use super::BlackbirdApp;
use super::ui::timeseries_plot::{Series, TimeseriesPlot};

const GYRO_AXIS_NAMES: [&str; 3] = ["roll", "pitch", "yaw"];
const GYRO_AXIS_COLORS: [Color32; 3] = [Color32::RED, Color32::GREEN, Color32::LIGHT_BLUE];

impl BlackbirdApp {
    pub(super) fn show_mainview(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default().show_inside(ui, |ui| {
            let Some(loaded) = self.logs.iter().find(|l| l.selected) else {
                ui.label("No log selected");
                return;
            };
            let Some(parsed) = loaded.log.get(loaded.active_sublog) else {
                return;
            };

            let fd = &parsed.flight_data;
            let t0 = fd.time_us.first().copied().unwrap_or(0);
            let duration_s = fd
                .time_us
                .last()
                .map(|&last| last.saturating_sub(t0) as f64 / 1_000_000.0)
                .unwrap_or(1.0)
                .max(f64::MIN_POSITIVE);

            let series: Vec<Series> = fd
                .gyro
                .iter()
                .enumerate()
                .filter_map(|(i, axis)| {
                    axis.as_ref().map(|samples| Series {
                        label: GYRO_AXIS_NAMES[i].to_string(),
                        color: GYRO_AXIS_COLORS[i],
                        time_us: fd.time_us.as_slice(),
                        samples: samples.as_slice(),
                        visible: self.gyro_axis_visible[i],
                    })
                })
                .collect();

            let mut plot = TimeseriesPlot {
                id: "gyro_plot".to_string(),
                y_label: "deg/s".to_string(),
                t0,
                series,
                default_x_range: Some((0.0, duration_s)),
            };
            plot.show(ui);

            for (i, s) in plot.series.iter().enumerate() {
                self.gyro_axis_visible[i] = s.visible;
            }
        });
    }
}
