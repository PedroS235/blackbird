use egui::{Color32, RichText, Ui};
use egui_plot::{Line, Plot, PlotPoint, PlotPoints, Text, VLine};

use super::{PEAK_MARKER_COLOR, drawn_axes};
use crate::ai;
use crate::analysis::SpectralAnalysis;
use crate::app::tabs::{get_axis_color, stacked_plot_height_reserving};
use crate::app::ui::ai_feedback;
use crate::parser::{Axis, PerAxis};

const FILTER_MARKER_COLOR: Color32 = Color32::from_rgb(140, 160, 255);

/// Keeps an explicit checkbox rather than a legend: the filtered trace is a
/// conditional build, not a hide, and the panel emits a named marker per
/// detected peak — a legend here would list a dozen frequency labels.
#[derive(Default)]
pub(super) struct Psd {
    filtered_visible: PerAxis<bool>,
    feedback: ai::Feedback,
}

impl Psd {
    pub(super) fn show(&mut self, ui: &mut Ui, analysis: &SpectralAnalysis) {
        let plot_height =
            stacked_plot_height_reserving(ui, drawn_axes(analysis), ai_feedback::RESERVE_HEIGHT);

        for axis in Axis::ALL {
            let Some(spec) = analysis.axis(axis) else {
                continue;
            };

            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{} psd", axis.name())).strong());
                ui.checkbox(&mut self.filtered_visible[axis], "show filtered");
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
                    plot_ui.line(
                        Line::new("raw", raw_points)
                            .color(elegance::Palette::charcoal().text_faint),
                    );

                    if self.filtered_visible[axis]
                        && let Some(filtered_psd) = &spec.filtered_psd
                    {
                        let filtered_points: PlotPoints = filtered_psd
                            .freq_hz
                            .iter()
                            .zip(&filtered_psd.power_db)
                            .map(|(&f, &v)| [f, v])
                            .collect();
                        plot_ui.line(
                            Line::new("filtered", filtered_points).color(get_axis_color(axis)),
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

        ui.add_space(8.0);
        ai_feedback::show(ui, &mut self.feedback, || ai::psd_message(analysis));
    }
}
