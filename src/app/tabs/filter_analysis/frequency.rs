use egui::{RichText, Ui};
use egui_plot::{Line, Plot, PlotPoint, PlotPoints, Text, VLine};
use elegance::Slider;

use super::{PEAK_MARKER_COLOR, drawn_axes};
use crate::analysis::SpectralAnalysis;
use crate::app::tabs::{get_axis_color, stacked_plot_height};
use crate::parser::{Axis, PerAxis};

/// Welch-averaged linear magnitude — no dB, chunked and averaged like the
/// PSD tab to smooth out the noise a single full-log FFT would show.
pub(super) struct Frequency {
    filtered_visible: PerAxis<bool>,
    peak_min_hz: f32,
}

impl Default for Frequency {
    fn default() -> Self {
        Self {
            filtered_visible: PerAxis::splat(false),
            peak_min_hz: 100.0,
        }
    }
}

impl Frequency {
    pub(super) fn show(&mut self, ui: &mut Ui, analysis: &SpectralAnalysis) {
        ui.add(
            Slider::new(&mut self.peak_min_hz, 0.0..=500.0)
                .label("max search min Hz")
                .suffix("Hz"),
        );
        ui.add_space(4.0);

        // After the slider, and over the axes that draw: measuring before the
        // slider sized the plots against height the slider then took.
        let plot_height = stacked_plot_height(ui, drawn_axes(analysis));

        for axis in Axis::ALL {
            let Some(spec) = analysis.axis(axis) else {
                continue;
            };
            let raw_spectrum = &spec.raw_spectrum;

            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{} frequency", axis.name())).strong());
                ui.checkbox(&mut self.filtered_visible[axis], "show filtered");
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
                    plot_ui.line(
                        Line::new("raw", raw_points)
                            .color(elegance::Palette::charcoal().text_faint),
                    );

                    if let Some((freq, mag)) = spectrum_peak(
                        &raw_spectrum.freq_hz,
                        &raw_spectrum.magnitude,
                        self.peak_min_hz as f64,
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

                    if self.filtered_visible[axis]
                        && let Some(filtered_spectrum) = &spec.filtered_spectrum
                    {
                        let filtered_points: PlotPoints = filtered_spectrum
                            .freq_hz
                            .iter()
                            .zip(&filtered_spectrum.magnitude)
                            .map(|(&f, &v)| [f, v])
                            .collect();
                        plot_ui.line(
                            Line::new("filtered", filtered_points).color(get_axis_color(axis)),
                        );
                    }
                });
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
