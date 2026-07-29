use egui::{Color32, RichText, Ui};

use crate::parser::FlightData;

use super::BlackbirdApp;
use super::ui::timeseries_plot::{Series, TimeseriesPlot};

const GYRO_AXIS_NAMES: [&str; 3] = ["roll", "pitch", "yaw"];
const GYRO_AXIS_COLORS: [Color32; 3] = [Color32::RED, Color32::GREEN, Color32::BLUE];
const GYRO_RAW_COLOR: Color32 = Color32::GRAY;
const VBAT_COLOR: Color32 = Color32::from_rgb(255, 202, 40);
const CURRENT_COLOR: Color32 = Color32::from_rgb(255, 111, 97);
const SETPOINT_COLOR: Color32 = Color32::WHITE;
const RSSI_COLOR: Color32 = Color32::from_rgb(100, 200, 255);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum MainTab {
    #[default]
    Timeseries,
    FilterAnalysis,
    PidAnalysis,
    AutoTune,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum TimeseriesTab {
    #[default]
    Gyro,
    PowerBattery,
    Rssi,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum PidAnalysisTab {
    #[default]
    GyroVsSetpoint,
    StepResponse,
}

impl BlackbirdApp {
    pub(super) fn show_mainview(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default().show_inside(ui, |ui| {
            self.show_main_tabs(ui);
            ui.separator();

            match self.main_tab {
                MainTab::Timeseries => self.show_timeseries_tab(ui),
                MainTab::FilterAnalysis => {
                    ui.label("Filter Analysis - coming soon");
                }
                MainTab::PidAnalysis => self.show_pidanalysis_tab(ui),
                MainTab::AutoTune => {
                    ui.label("Auto Tune - coming soon");
                }
            }
        });
    }

    fn show_main_tabs(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for (tab, label) in [
                (MainTab::Timeseries, "Timeseries"),
                (MainTab::FilterAnalysis, "Filter Analysis"),
                (MainTab::PidAnalysis, "PID Analysis"),
                (MainTab::AutoTune, "Auto Tune"),
            ] {
                if ui.selectable_label(self.main_tab == tab, label).clicked() {
                    self.main_tab = tab;
                }
            }
        });
    }

    fn show_timeseries_tab(&mut self, ui: &mut egui::Ui) {
        let selected_fd = self
            .logs
            .iter()
            .find(|l| l.selected)
            .and_then(|loaded| loaded.log.get(loaded.active_sublog))
            .map(|parsed| &parsed.flight_data);

        let has_battery_data =
            selected_fd.is_some_and(|fd| fd.vbat.is_some() || fd.current.is_some());
        let has_rssi_data = selected_fd.is_some_and(|fd| fd.rssi.is_some());

        if self.timeseries_tab == TimeseriesTab::PowerBattery && !has_battery_data {
            self.timeseries_tab = TimeseriesTab::Gyro;
        }
        if self.timeseries_tab == TimeseriesTab::Rssi && !has_rssi_data {
            self.timeseries_tab = TimeseriesTab::Gyro;
        }

        ui.horizontal(|ui| {
            for (tab, label, enabled) in [
                (TimeseriesTab::Gyro, "Gyro", true),
                (
                    TimeseriesTab::PowerBattery,
                    "Power & Battery",
                    has_battery_data,
                ),
                (TimeseriesTab::Rssi, "Receiver RSSI", has_rssi_data),
            ] {
                let selectable = egui::Button::selectable(self.timeseries_tab == tab, label);
                if ui.add_enabled(enabled, selectable).clicked() {
                    self.timeseries_tab = tab;
                }
            }
        });
        ui.add_space(4.0);

        let Some(loaded) = self.logs.iter().find(|l| l.selected) else {
            ui.label("No log selected");
            return;
        };
        let Some(parsed) = loaded.log.get(loaded.active_sublog) else {
            return;
        };
        let fd = &parsed.flight_data;

        match self.timeseries_tab {
            TimeseriesTab::Gyro => Self::show_gyro_plots(
                ui,
                fd,
                &mut self.gyro_filtered_visible,
                &mut self.gyro_raw_visible,
            ),
            TimeseriesTab::PowerBattery => Self::show_power_battery_plots(
                ui,
                fd,
                &mut self.vbat_visible,
                &mut self.current_visible,
            ),
            TimeseriesTab::Rssi => Self::show_rssi_plot(ui, fd, &mut self.rssi_visible),
        }
    }

    fn show_pidanalysis_tab(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            for (tab, label, enabled) in [
                (PidAnalysisTab::GyroVsSetpoint, "Gyro Vs Setpoint", true),
                (PidAnalysisTab::StepResponse, "Step Response", false),
            ] {
                let selectable = egui::Button::selectable(self.pidanalysis_tab == tab, label);
                if ui.add_enabled(enabled, selectable).clicked() {
                    self.pidanalysis_tab = tab;
                }
            }
        });
        ui.add_space(4.0);
        let Some(loaded) = self.logs.iter().find(|l| l.selected) else {
            ui.label("No log selected");
            return;
        };
        let Some(parsed) = loaded.log.get(loaded.active_sublog) else {
            return;
        };
        let fd = &parsed.flight_data;

        match self.pidanalysis_tab {
            PidAnalysisTab::GyroVsSetpoint => Self::show_gyro_vs_setpoint_plots(
                ui,
                fd,
                &mut self.gyro_filtered_visible,
                &mut self.setpoint_visible,
            ),
            PidAnalysisTab::StepResponse => {
                ui.label("Step Response - coming soon");
            }
        }
    }

    fn show_gyro_plots(
        ui: &mut egui::Ui,
        fd: &FlightData,
        filtered_visible: &mut [bool; 3],
        raw_visible: &mut [bool; 3],
    ) {
        let t0 = fd.time_us.first().copied().unwrap_or(0);
        let duration_s = fd
            .time_us
            .last()
            .map(|&last| last.saturating_sub(t0) as f64 / 1_000_000.0)
            .unwrap_or(1.0)
            .max(f64::MIN_POSITIVE);

        let plot_height = (ui.available_height() / 3.0 - 24.0).max(80.0);

        for i in 0..3 {
            let Some(raw) = &fd.raw_gyro[i] else {
                continue;
            };

            ui.label(RichText::new(GYRO_AXIS_NAMES[i]).strong());

            let mut series = vec![Series {
                label: format!("{} (raw)", GYRO_AXIS_NAMES[i]),
                color: GYRO_RAW_COLOR,
                time_us: fd.time_us.as_slice(),
                samples: raw.as_slice(),
                visible: raw_visible[i],
            }];

            if let Some(filtered) = &fd.gyro[i] {
                series.push(Series {
                    label: format!("{} (filtered)", GYRO_AXIS_NAMES[i]),
                    color: GYRO_AXIS_COLORS[i],
                    time_us: fd.time_us.as_slice(),
                    samples: filtered.as_slice(),
                    visible: filtered_visible[i],
                });
            }

            let mut plot = TimeseriesPlot {
                id: format!("gyro_plot_{}", GYRO_AXIS_NAMES[i]),
                y_label: "deg/s".to_string(),
                t0,
                series,
                default_x_range: Some((0.0, duration_s)),
                height: Some(plot_height),
            };
            plot.show(ui);

            filtered_visible[i] = plot.series[0].visible;
            if let Some(raw_series) = plot.series.get(1) {
                raw_visible[i] = raw_series.visible;
            }
        }
    }

    fn show_gyro_vs_setpoint_plots(
        ui: &mut egui::Ui,
        fd: &FlightData,
        gyro_visible: &mut [bool; 3],
        setpoint_visible: &mut [bool; 3],
    ) {
        let t0 = fd.time_us.first().copied().unwrap_or(0);
        let duration_s = fd
            .time_us
            .last()
            .map(|&last| last.saturating_sub(t0) as f64 / 1_000_000.0)
            .unwrap_or(1.0)
            .max(f64::MIN_POSITIVE);

        let plot_height = (ui.available_height() / 3.0 - 24.0).max(80.0);

        for i in 0..3 {
            let Some(gyro) = &fd.gyro[i] else {
                continue;
            };

            ui.label(RichText::new(GYRO_AXIS_NAMES[i]).strong());

            let mut series = vec![Series {
                label: format!("{} (gyro)", GYRO_AXIS_NAMES[i]),
                color: GYRO_AXIS_COLORS[i],
                time_us: fd.time_us.as_slice(),
                samples: gyro.as_slice(),
                visible: gyro_visible[i],
            }];

            if let Some(setpoint) = &fd.setpoint[i] {
                series.push(Series {
                    label: format!("{} (setpoint)", GYRO_AXIS_NAMES[i]),
                    color: SETPOINT_COLOR,
                    time_us: fd.time_us.as_slice(),
                    samples: setpoint.as_slice(),
                    visible: setpoint_visible[i],
                });
            }

            let mut plot = TimeseriesPlot {
                id: format!("gyro_vs_setpoint_plot_{}", GYRO_AXIS_NAMES[i]),
                y_label: "deg/s".to_string(),
                t0,
                series,
                default_x_range: Some((0.0, duration_s)),
                height: Some(plot_height),
            };
            plot.show(ui);

            gyro_visible[i] = plot.series[0].visible;
            if let Some(setpoint_series) = plot.series.get(1) {
                setpoint_visible[i] = setpoint_series.visible;
            }
        }
    }

    fn show_power_battery_plots(
        ui: &mut egui::Ui,
        fd: &FlightData,
        vbat_visible: &mut bool,
        current_visible: &mut bool,
    ) {
        let t0 = fd.time_us.first().copied().unwrap_or(0);
        let duration_s = fd
            .time_us
            .last()
            .map(|&last| last.saturating_sub(t0) as f64 / 1_000_000.0)
            .unwrap_or(1.0)
            .max(f64::MIN_POSITIVE);

        let metric_count = [fd.vbat.is_some(), fd.current.is_some()]
            .iter()
            .filter(|present| **present)
            .count()
            .max(1);
        let plot_height = (ui.available_height() / metric_count as f32 - 24.0).max(80.0);

        if let Some(vbat) = &fd.vbat {
            ui.label(RichText::new("Battery Voltage").strong());
            let mut plot = TimeseriesPlot {
                id: "power_vbat_plot".to_string(),
                y_label: "V".to_string(),
                t0,
                series: vec![Series {
                    label: "vbat".to_string(),
                    color: VBAT_COLOR,
                    time_us: fd.time_us.as_slice(),
                    samples: vbat.as_slice(),
                    visible: *vbat_visible,
                }],
                default_x_range: Some((0.0, duration_s)),
                height: Some(plot_height),
            };
            plot.show(ui);
            *vbat_visible = plot.series[0].visible;
        }

        if let Some(current) = &fd.current {
            ui.label(RichText::new("Current").strong());
            let mut plot = TimeseriesPlot {
                id: "power_current_plot".to_string(),
                y_label: "A".to_string(),
                t0,
                series: vec![Series {
                    label: "current".to_string(),
                    color: CURRENT_COLOR,
                    time_us: fd.time_us.as_slice(),
                    samples: current.as_slice(),
                    visible: *current_visible,
                }],
                default_x_range: Some((0.0, duration_s)),
                height: Some(plot_height),
            };
            plot.show(ui);
            *current_visible = plot.series[0].visible;
        }
    }

    fn show_rssi_plot(ui: &mut egui::Ui, fd: &FlightData, rssi_visible: &mut bool) {
        let Some(rssi) = &fd.rssi else {
            return;
        };

        let t0 = fd.time_us.first().copied().unwrap_or(0);
        let duration_s = fd
            .time_us
            .last()
            .map(|&last| last.saturating_sub(t0) as f64 / 1_000_000.0)
            .unwrap_or(1.0)
            .max(f64::MIN_POSITIVE);

        ui.label(RichText::new("RSSI").strong());
        let mut plot = TimeseriesPlot {
            id: "rssi_plot".to_string(),
            y_label: "%".to_string(),
            t0,
            series: vec![Series {
                label: "rssi".to_string(),
                color: RSSI_COLOR,
                time_us: fd.time_us.as_slice(),
                samples: rssi.as_slice(),
                visible: *rssi_visible,
            }],
            default_x_range: Some((0.0, duration_s)),
            height: Some((ui.available_height() - 24.0).max(80.0)),
        };
        plot.show(ui);
        *rssi_visible = plot.series[0].visible;
    }
}
