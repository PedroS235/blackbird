use egui::{Color32, RichText, Ui};
use egui_plot::{Line, Plot, PlotPoint, PlotPoints, Text, VLine};
use elegance::Slider;

use crate::analysis::SpectralAnalysis;
use crate::parser::{Axis, FlightData, PerAxis};

use super::BlackbirdApp;
use super::ui::heatmap::{Heatmap, HeatmapOrientation, OverlaySeries};
use super::ui::timeseries_plot::{Series, TimeseriesPlot};

const GYRO_AXIS_COLORS: PerAxis<Color32> = PerAxis([Color32::RED, Color32::GREEN, Color32::BLUE]);
const GYRO_RAW_COLOR: Color32 = Color32::GRAY;
const VBAT_COLOR: Color32 = Color32::from_rgb(255, 202, 40);
const CURRENT_COLOR: Color32 = Color32::from_rgb(255, 111, 97);
const SETPOINT_COLOR: Color32 = Color32::WHITE;
const RSSI_COLOR: Color32 = Color32::from_rgb(100, 200, 255);
const PEAK_MARKER_COLOR: Color32 = Color32::from_rgb(255, 215, 0);
const FILTER_MARKER_COLOR: Color32 = Color32::from_rgb(140, 160, 255);

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
pub(super) enum FilterAnalysisTab {
    #[default]
    Psd,
    Frequency,
    VsReference,
    Spectrogram,
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
                MainTab::FilterAnalysis => self.show_filter_analysis_tab(ui),
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
            .current_flight()
            .map(|(parsed, _)| &parsed.flight_data);

        let has_battery_data = selected_fd.is_some_and(|fd| fd.has_power());
        let has_rssi_data = selected_fd.is_some_and(|fd| fd.has_rssi());

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

        let Some(fd) = selected_fd else {
            ui.label("No log selected");
            return;
        };

        match self.timeseries_tab {
            TimeseriesTab::Gyro => Self::show_gyro_plots(
                ui,
                fd,
                &mut self.view_state.timeseries.gyro_filtered_visible,
                &mut self.view_state.timeseries.gyro_raw_visible,
            ),
            TimeseriesTab::PowerBattery => Self::show_power_battery_plots(
                ui,
                fd,
                &mut self.view_state.timeseries.vbat_visible,
                &mut self.view_state.timeseries.current_visible,
            ),
            TimeseriesTab::Rssi => {
                Self::show_rssi_plot(ui, fd, &mut self.view_state.timeseries.rssi_visible)
            }
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
        let Some((parsed, _)) = self.logs.current_flight() else {
            ui.label("No log selected");
            return;
        };
        let fd = &parsed.flight_data;

        match self.pidanalysis_tab {
            PidAnalysisTab::GyroVsSetpoint => Self::show_gyro_vs_setpoint_plots(
                ui,
                fd,
                &mut self.view_state.pid_analysis.gyro_filtered_visible,
                &mut self.view_state.pid_analysis.setpoint_visible,
            ),
            PidAnalysisTab::StepResponse => {
                ui.label("Step Response - coming soon");
            }
        }
    }

    fn show_gyro_plots(
        ui: &mut egui::Ui,
        fd: &FlightData,
        filtered_visible: &mut PerAxis<bool>,
        raw_visible: &mut PerAxis<bool>,
    ) {
        let t0 = fd.start_us();
        let duration_s = fd.duration_s().max(f64::MIN_POSITIVE);

        let plot_height = (ui.available_height() / 3.0 - 24.0).max(80.0);

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
                visible: raw_visible[axis],
            }];

            if let Some(filtered) = fd.gyro(axis) {
                series.push(Series {
                    label: format!("{} (filtered)", axis.name()),
                    color: GYRO_AXIS_COLORS[axis],
                    time_us: fd.time_us(),
                    samples: filtered,
                    visible: filtered_visible[axis],
                });
            }

            let mut plot = TimeseriesPlot {
                id: format!("gyro_plot_{}", axis.name()),
                y_label: "deg/s".to_string(),
                t0,
                series,
                default_x_range: Some((0.0, duration_s)),
                height: Some(plot_height),
            };
            plot.show(ui);

            raw_visible[axis] = plot.series[0].visible;
            filtered_visible[axis] = plot.series[1].visible;
        }
    }

    fn show_gyro_vs_setpoint_plots(
        ui: &mut egui::Ui,
        fd: &FlightData,
        gyro_visible: &mut PerAxis<bool>,
        setpoint_visible: &mut PerAxis<bool>,
    ) {
        let t0 = fd.start_us();
        let duration_s = fd.duration_s().max(f64::MIN_POSITIVE);

        let plot_height = (ui.available_height() / 3.0 - 24.0).max(80.0);

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
                visible: gyro_visible[axis],
            }];

            if let Some(setpoint) = fd.setpoint(axis) {
                series.push(Series {
                    label: format!("{} (setpoint)", axis.name()),
                    color: SETPOINT_COLOR,
                    time_us: fd.time_us(),
                    samples: setpoint,
                    visible: setpoint_visible[axis],
                });
            }

            let mut plot = TimeseriesPlot {
                id: format!("gyro_vs_setpoint_plot_{}", axis.name()),
                y_label: "deg/s".to_string(),
                t0,
                series,
                default_x_range: Some((0.0, duration_s)),
                height: Some(plot_height),
            };
            plot.show(ui);

            gyro_visible[axis] = plot.series[0].visible;
            if let Some(setpoint_series) = plot.series.get(1) {
                setpoint_visible[axis] = setpoint_series.visible;
            }
        }
    }

    fn show_power_battery_plots(
        ui: &mut egui::Ui,
        fd: &FlightData,
        vbat_visible: &mut bool,
        current_visible: &mut bool,
    ) {
        let t0 = fd.start_us();
        let duration_s = fd.duration_s().max(f64::MIN_POSITIVE);

        let metric_count = [fd.vbat().is_some(), fd.current().is_some()]
            .iter()
            .filter(|present| **present)
            .count()
            .max(1);
        let plot_height = (ui.available_height() / metric_count as f32 - 24.0).max(80.0);

        if let Some(vbat) = fd.vbat() {
            ui.label(RichText::new("Battery Voltage").strong());
            let mut plot = TimeseriesPlot {
                id: "power_vbat_plot".to_string(),
                y_label: "V".to_string(),
                t0,
                series: vec![Series {
                    label: "vbat".to_string(),
                    color: VBAT_COLOR,
                    time_us: fd.time_us(),
                    samples: vbat,
                    visible: *vbat_visible,
                }],
                default_x_range: Some((0.0, duration_s)),
                height: Some(plot_height),
            };
            plot.show(ui);
            *vbat_visible = plot.series[0].visible;
        }

        if let Some(current) = fd.current() {
            ui.label(RichText::new("Current").strong());
            let mut plot = TimeseriesPlot {
                id: "power_current_plot".to_string(),
                y_label: "A".to_string(),
                t0,
                series: vec![Series {
                    label: "current".to_string(),
                    color: CURRENT_COLOR,
                    time_us: fd.time_us(),
                    samples: current,
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
        let Some(rssi) = fd.rssi() else {
            return;
        };

        let t0 = fd.start_us();
        let duration_s = fd.duration_s().max(f64::MIN_POSITIVE);

        ui.label(RichText::new("RSSI").strong());
        let mut plot = TimeseriesPlot {
            id: "rssi_plot".to_string(),
            y_label: "%".to_string(),
            t0,
            series: vec![Series {
                label: "rssi".to_string(),
                color: RSSI_COLOR,
                time_us: fd.time_us(),
                samples: rssi,
                visible: *rssi_visible,
            }],
            default_x_range: Some((0.0, duration_s)),
            height: Some((ui.available_height() - 24.0).max(80.0)),
        };
        plot.show(ui);
        *rssi_visible = plot.series[0].visible;
    }

    fn show_filter_analysis_tab(&mut self, ui: &mut Ui) {
        let Some((parsed, analysis)) = self.logs.current_flight() else {
            ui.label("No log selected");
            return;
        };
        let fd = &parsed.flight_data;
        let has_dyn_notch_trace = parsed.metadata.debug_mode == "FFT_FREQ" && fd.has_debug_axes();

        ui.horizontal(|ui| {
            for (tab, label) in [
                (FilterAnalysisTab::Psd, "PSD"),
                (FilterAnalysisTab::Frequency, "Frequency"),
                (FilterAnalysisTab::VsReference, "Vs Reference"),
                (FilterAnalysisTab::Spectrogram, "Spectrogram"),
            ] {
                if ui
                    .selectable_label(self.filteranalysis_tab == tab, label)
                    .clicked()
                {
                    self.filteranalysis_tab = tab;
                }
            }
        });
        ui.add_space(4.0);

        match self.filteranalysis_tab {
            FilterAnalysisTab::Psd => Self::show_psd_tab(
                ui,
                analysis,
                &mut self.view_state.filter_analysis.psd_filtered_visible,
            ),
            FilterAnalysisTab::Frequency => Self::show_frequency_tab(
                ui,
                analysis,
                &mut self.view_state.filter_analysis.frequency_filtered_visible,
                &mut self.view_state.filter_analysis.frequency_peak_min_hz,
            ),
            FilterAnalysisTab::VsReference => Self::show_vs_reference_tab(
                ui,
                analysis,
                &mut self.view_state.filter_analysis.heatmap_floor_db,
            ),
            FilterAnalysisTab::Spectrogram => Self::show_spectrogram_tab(
                ui,
                analysis,
                fd,
                has_dyn_notch_trace,
                &mut self.view_state.filter_analysis.spectrogram_floor_db,
            ),
        }
    }

    /// Per-axis time-vs-frequency waterfall (raw signal power binned by time
    /// instead of throttle). When the log was recorded with debug mode
    /// `FFT_FREQ`, overlays the dynamic notch tracker's live center frequency
    /// (`debug[0..3]`) on top, so a mistracking or clamped tracker is visible
    /// directly against the noise band it's supposed to be following.
    fn show_spectrogram_tab(
        ui: &mut egui::Ui,
        analysis: &SpectralAnalysis,
        fd: &FlightData,
        has_dyn_notch_trace: bool,
        floor_db: &mut f32,
    ) {
        ui.add(
            Slider::new(floor_db, -120.0..=-5.0)
                .label("sensitivity (noise floor dB)")
                .suffix("dB"),
        );
        ui.add_space(4.0);

        let t0 = fd.start_us();
        let plot_height = (ui.available_height() / 3.0 - 24.0).max(80.0);

        for axis in Axis::ALL {
            let Some(spec) = analysis.axis(axis) else {
                continue;
            };
            let Some(time_map) = &spec.time_map else {
                continue;
            };

            ui.label(RichText::new(format!("{} spectrogram", axis.name())).strong());
            let overlay = has_dyn_notch_trace
                .then(|| fd.debug_axis(axis))
                .flatten()
                .map(|samples| OverlaySeries {
                    t0,
                    time_us: fd.time_us(),
                    samples,
                });
            Heatmap {
                id: format!("spectrogram_{}", axis.name()),
                orientation: HeatmapOrientation::VsTime,
                spectrum: time_map,
                height: plot_height,
                floor_db: *floor_db as f64,
                overlay,
            }
            .show(ui);
        }
    }

    fn show_psd_tab(
        ui: &mut egui::Ui,
        analysis: &SpectralAnalysis,
        filtered_visible: &mut PerAxis<bool>,
    ) {
        let plot_height = (ui.available_height() / 3.0 - 24.0).max(80.0);

        for axis in Axis::ALL {
            let Some(spec) = analysis.axis(axis) else {
                continue;
            };

            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{} psd", axis.name())).strong());
                ui.checkbox(&mut filtered_visible[axis], "show filtered");
            });

            Plot::new(format!("psd_plot_{}", axis.name()))
                .height(plot_height)
                .x_axis_label("Hz")
                .y_axis_label("dB")
                .show(ui, |plot_ui| {
                    let raw_points: PlotPoints = spec
                        .raw_psd
                        .freq_hz
                        .iter()
                        .zip(&spec.raw_psd.power_db)
                        .map(|(&f, &v)| [f, v])
                        .collect();
                    plot_ui.line(Line::new("raw", raw_points).color(GYRO_RAW_COLOR));

                    if filtered_visible[axis]
                        && let Some(filtered_psd) = &spec.filtered_psd
                    {
                        let filtered_points: PlotPoints = filtered_psd
                            .freq_hz
                            .iter()
                            .zip(&filtered_psd.power_db)
                            .map(|(&f, &v)| [f, v])
                            .collect();
                        plot_ui.line(
                            Line::new("filtered", filtered_points).color(GYRO_AXIS_COLORS[axis]),
                        );
                    }

                    for marker in analysis
                        .filter_markers
                        .iter()
                        .filter(|m| m.label.starts_with("Gyro"))
                    {
                        plot_ui.vline(
                            VLine::new(marker.label.clone(), marker.center_hz as f64)
                                .color(FILTER_MARKER_COLOR),
                        );
                    }

                    for peak in &spec.peaks {
                        let label = match peak.harmonic_of {
                            Some(_) => format!("{:.0} Hz (harmonic)", peak.freq_hz),
                            None => format!("{:.0} Hz", peak.freq_hz),
                        };
                        plot_ui.vline(
                            VLine::new(label.clone(), peak.freq_hz).color(PEAK_MARKER_COLOR),
                        );
                        plot_ui.text(
                            Text::new(
                                format!("{label}_label"),
                                PlotPoint::new(peak.freq_hz, peak.amplitude_db),
                                label,
                            )
                            .color(PEAK_MARKER_COLOR)
                            .anchor(egui::Align2::CENTER_BOTTOM),
                        );
                    }
                });
        }
    }

    /// Welch-averaged linear magnitude — no dB, chunked and averaged like the
    /// PSD tab to smooth out the noise a single full-log FFT would show.
    fn show_frequency_tab(
        ui: &mut egui::Ui,
        analysis: &SpectralAnalysis,
        filtered_visible: &mut PerAxis<bool>,
        peak_min_hz: &mut f32,
    ) {
        let plot_height = (ui.available_height() / 3.0 - 24.0).max(80.0);

        ui.add(
            Slider::new(peak_min_hz, 0.0..=500.0)
                .label("max search min Hz")
                .suffix("Hz"),
        );
        ui.add_space(4.0);

        for axis in Axis::ALL {
            let Some(spec) = analysis.axis(axis) else {
                continue;
            };
            let raw_spectrum = &spec.raw_spectrum;

            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{} frequency", axis.name())).strong());
                ui.checkbox(&mut filtered_visible[axis], "show filtered");
            });

            Plot::new(format!("frequency_plot_{}", axis.name()))
                .height(plot_height)
                .x_axis_label("Hz")
                .y_axis_label("magnitude")
                .show(ui, |plot_ui| {
                    let raw_points: PlotPoints = raw_spectrum
                        .freq_hz
                        .iter()
                        .zip(&raw_spectrum.magnitude)
                        .map(|(&f, &v)| [f, v])
                        .collect();
                    plot_ui.line(Line::new("raw", raw_points).color(GYRO_RAW_COLOR));

                    if let Some((freq, mag)) = spectrum_peak(
                        &raw_spectrum.freq_hz,
                        &raw_spectrum.magnitude,
                        *peak_min_hz as f64,
                    ) {
                        plot_ui.vline(VLine::new("max", freq).color(PEAK_MARKER_COLOR));
                        plot_ui.text(
                            Text::new(
                                "max_label",
                                PlotPoint::new(freq, mag),
                                format!("{freq:.0} Hz"),
                            )
                            .color(PEAK_MARKER_COLOR)
                            .anchor(egui::Align2::CENTER_BOTTOM),
                        );
                    }

                    if filtered_visible[axis]
                        && let Some(filtered_spectrum) = &spec.filtered_spectrum
                    {
                        let filtered_points: PlotPoints = filtered_spectrum
                            .freq_hz
                            .iter()
                            .zip(&filtered_spectrum.magnitude)
                            .map(|(&f, &v)| [f, v])
                            .collect();
                        plot_ui.line(
                            Line::new("filtered", filtered_points).color(GYRO_AXIS_COLORS[axis]),
                        );
                    }
                });
        }
    }

    fn show_vs_reference_tab(ui: &mut egui::Ui, analysis: &SpectralAnalysis, floor_db: &mut f32) {
        ui.add(
            Slider::new(floor_db, -120.0..=-5.0)
                .label("sensitivity (noise floor dB)")
                .suffix("dB"),
        );
        ui.add_space(4.0);

        let plot_height = (ui.available_height() / 3.0 - 24.0).max(80.0);

        for axis in Axis::ALL {
            let Some(spec) = analysis.axis(axis) else {
                continue;
            };
            let Some(throttle_map) = &spec.throttle_map else {
                continue;
            };

            ui.label(RichText::new(format!("{} vs throttle", axis.name())).strong());
            Heatmap {
                id: format!("throttle_heatmap_{}", axis.name()),
                orientation: HeatmapOrientation::VsThrottle,
                spectrum: throttle_map,
                height: plot_height,
                floor_db: *floor_db as f64,
                overlay: None,
            }
            .show(ui);
        }
    }
}

/// Highest-magnitude bin at or above `min_hz` — mirrors Betaflight's dynamic
/// notch peak search, which ignores everything below its `min_hz` because that
/// band is flight dynamics (stick input, prop wash), not motor/prop noise.
fn spectrum_peak(freq_hz: &[f64], magnitude: &[f64], min_hz: f64) -> Option<(f64, f64)> {
    freq_hz
        .iter()
        .zip(magnitude)
        .filter(|&(&f, _)| f >= min_hz)
        .max_by(|(_, a), (_, b)| a.total_cmp(b))
        .map(|(&f, &m)| (f, m))
}
