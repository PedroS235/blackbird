use egui::{RichText, Ui};
use elegance::Slider;

use crate::app::tabs::{TabCtx, stacked_plot_height};
use crate::app::ui::heatmap::{Heatmap, HeatmapOrientation, OverlaySeries};
use crate::parser::Axis;

/// Per-axis time-vs-frequency waterfall (raw signal power binned by time
/// instead of throttle). When the log was recorded with debug mode
/// `FFT_FREQ`, overlays the dynamic notch tracker's live center frequency
/// (`debug[0..3]`) on top, so a mistracking or clamped tracker is visible
/// directly against the noise band it's supposed to be following.
pub(super) struct Spectrogram {
    floor_db: f32,
}

impl Default for Spectrogram {
    fn default() -> Self {
        Self { floor_db: -60.0 }
    }
}

impl Spectrogram {
    pub(super) fn show(&mut self, ui: &mut Ui, ctx: &TabCtx<'_>) {
        let fd = ctx.flight;
        let has_dyn_notch_trace = ctx.metadata.debug_mode == "FFT_FREQ" && fd.has_debug_axes();

        ui.add(
            Slider::new(&mut self.floor_db, -120.0..=-5.0)
                .label("sensitivity (noise floor dB)")
                .suffix("dB"),
        );
        ui.add_space(4.0);

        let t0 = fd.start_us();
        let plot_height = stacked_plot_height(ui, 3);

        for axis in Axis::ALL {
            let Some(spec) = ctx.analysis.spectral.axis(axis) else {
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
                floor_db: self.floor_db as f64,
                overlay,
            }
            .show(ui);
        }
    }
}
