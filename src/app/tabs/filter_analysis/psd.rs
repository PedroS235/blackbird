use egui::{Color32, RichText, Ui, Vec2b};
use egui_plot::{Line, Plot, PlotPoint, PlotPoints, Span, Text, VLine};

use super::{PEAK_MARKER_COLOR, drawn_axes};
use crate::analysis::{FilterLoop, OverlayFamily, OverlayShape, SpectralAnalysis};
use crate::app::colors;
use crate::app::tabs::stacked_plot_height;
use crate::parser::{Axis, PerAxis};

const FILTER_MARKER_COLOR: Color32 = Color32::from_rgb(140, 160, 255);

/// Keeps an explicit checkbox rather than a legend: the filtered trace is a
/// conditional build, not a hide, and the panel emits a named marker per
/// detected peak — a legend here would list a dozen frequency labels.
#[derive(Default)]
pub(super) struct Psd {
    filtered_visible: PerAxis<bool>,
}

impl Psd {
    pub(super) fn show(&mut self, ui: &mut Ui, analysis: &SpectralAnalysis) {
        let plot_height = stacked_plot_height(ui, drawn_axes(analysis));
        let palette = colors::palette(ui.ctx());

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
                .allow_zoom(Vec2b::new(true, true))
                .allow_scroll(Vec2b::new(true, false))
                .allow_drag(Vec2b::new(true, true))
                .show(ui, |plot_ui| {
                    let raw_points: PlotPoints = spec
                        .raw_psd
                        .freq_hz
                        .iter()
                        .zip(&spec.raw_psd.power_db)
                        .map(|(&f, &v)| [f, v])
                        .collect();
                    plot_ui.line(Line::new("raw", raw_points).color(palette.text_faint));

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
                            Line::new("filtered", filtered_points)
                                .color(colors::axis_color(&palette, axis)),
                        );
                    }

                    for overlay in analysis.overlays.iter().filter(|o| {
                        matches!(
                            o.family,
                            OverlayFamily::Notch(FilterLoop::Gyro)
                                | OverlayFamily::Lowpass(FilterLoop::Gyro)
                                | OverlayFamily::DynNotch
                        )
                    }) {
                        match &overlay.shape {
                            OverlayShape::Line { hz } => plot_ui.vline(
                                VLine::new(overlay.label.clone(), *hz).color(FILTER_MARKER_COLOR),
                            ),
                            OverlayShape::Band { low_hz, high_hz } => plot_ui.span(
                                Span::new(overlay.label.clone(), *low_hz..=*high_hz)
                                    .border_color(FILTER_MARKER_COLOR),
                            ),
                            _ => {}
                        }
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
}
