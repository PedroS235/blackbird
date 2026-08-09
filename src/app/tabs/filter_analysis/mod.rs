mod frequency;
mod heatmap_panel;
mod psd;
mod spectrogram;
mod vs_reference;

use egui::{Color32, Ui};

use super::{TabCtx, tab_bar};
use crate::analysis::SpectralAnalysis;
use crate::parser::Axis;
use frequency::Frequency;
use heatmap_panel::{HeatmapKind, HeatmapPanel};
use psd::Psd;

pub(super) const PEAK_MARKER_COLOR: Color32 = Color32::from_rgb(255, 215, 0);

/// How many axes a spectral panel is about to stack. Axes the log never
/// carried are skipped, so a single-axis log gets one full-height plot rather
/// than a third of the panel above two thirds of nothing.
fn drawn_axes(analysis: &SpectralAnalysis) -> usize {
    Axis::ALL
        .iter()
        .filter(|&&axis| analysis.axis(axis).is_some())
        .count()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum FilterAnalysisTab {
    #[default]
    Psd,
    Frequency,
    VsReference,
    Spectrogram,
}

/// Each sub-tab owns its own widget state. `Psd` and `Frequency` both carry a
/// filtered-trace toggle, and they are deliberately separate — they used to
/// share one field, which meant toggling one silently toggled the other.
#[derive(Default)]
pub(super) struct FilterAnalysis {
    selected: FilterAnalysisTab,
    psd: Psd,
    frequency: Frequency,
    vs_reference: HeatmapPanel,
    spectrogram: HeatmapPanel,
}

impl FilterAnalysis {
    pub(super) fn show(&mut self, ui: &mut Ui, ctx: &TabCtx<'_>) {
        tab_bar(
            ui,
            &mut self.selected,
            &[
                (FilterAnalysisTab::Psd, "PSD", true),
                (FilterAnalysisTab::Frequency, "Frequency", true),
                (FilterAnalysisTab::VsReference, "Vs Reference", true),
                (FilterAnalysisTab::Spectrogram, "Spectrogram", true),
            ],
        );
        ui.add_space(4.0);

        match self.selected {
            FilterAnalysisTab::Psd => self.psd.show(ui, &ctx.analysis.spectral),
            FilterAnalysisTab::Frequency => self.frequency.show(ui, &ctx.analysis.spectral),
            FilterAnalysisTab::VsReference => self.vs_reference.show(
                ui,
                HeatmapKind::VsThrottle,
                vs_reference::rows(&ctx.analysis.spectral),
            ),
            FilterAnalysisTab::Spectrogram => {
                self.spectrogram
                    .show(ui, HeatmapKind::Spectrogram, spectrogram::rows(ctx))
            }
        }
    }
}
