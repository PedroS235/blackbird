use egui::{RichText, Ui};
use elegance::Slider;

use crate::analysis::SpectralAnalysis;
use crate::app::tabs::stacked_plot_height;
use crate::app::ui::heatmap::{Heatmap, HeatmapOrientation};
use crate::parser::Axis;

pub(super) struct VsReference {
    floor_db: f32,
}

impl Default for VsReference {
    fn default() -> Self {
        Self { floor_db: -60.0 }
    }
}

impl VsReference {
    pub(super) fn show(&mut self, ui: &mut Ui, analysis: &SpectralAnalysis) {
        ui.add(
            Slider::new(&mut self.floor_db, -120.0..=-5.0)
                .label("sensitivity (noise floor dB)")
                .suffix("dB"),
        );
        ui.add_space(4.0);

        let plot_height = stacked_plot_height(ui, 3);

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
                floor_db: self.floor_db as f64,
                overlay: None,
            }
            .show(ui);
        }
    }
}
