mod frequency;
mod psd;
mod spectrogram;
mod vs_reference;

use egui::{Color32, Ui};

use super::TabCtx;
use frequency::Frequency;
use psd::Psd;
use spectrogram::Spectrogram;
use vs_reference::VsReference;

pub(super) const PEAK_MARKER_COLOR: Color32 = Color32::from_rgb(255, 215, 0);

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
    vs_reference: VsReference,
    spectrogram: Spectrogram,
}

impl FilterAnalysis {
    pub(super) fn show(&mut self, ui: &mut Ui, ctx: &TabCtx<'_>) {
        ui.horizontal(|ui| {
            for (tab, label) in [
                (FilterAnalysisTab::Psd, "PSD"),
                (FilterAnalysisTab::Frequency, "Frequency"),
                (FilterAnalysisTab::VsReference, "Vs Reference"),
                (FilterAnalysisTab::Spectrogram, "Spectrogram"),
            ] {
                if ui.selectable_label(self.selected == tab, label).clicked() {
                    self.selected = tab;
                }
            }
        });
        ui.add_space(4.0);

        match self.selected {
            FilterAnalysisTab::Psd => self.psd.show(ui, ctx.analysis),
            FilterAnalysisTab::Frequency => self.frequency.show(ui, ctx.analysis),
            FilterAnalysisTab::VsReference => self.vs_reference.show(ui, ctx.analysis),
            FilterAnalysisTab::Spectrogram => self.spectrogram.show(ui, ctx),
        }
    }
}
