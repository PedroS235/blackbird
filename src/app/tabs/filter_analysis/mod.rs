mod frequency;
mod heatmap_panel;
mod psd;
mod spectrogram;
mod vs_reference;

use egui::Ui;

use super::{TabCtx, tab_bar};
use crate::analysis::SpectralAnalysis;
use crate::parser::Axis;
use frequency::Frequency;
use heatmap_panel::{HeatmapKind, HeatmapPanel};
use psd::Psd;

/// Here the raw gyro is the whole point — there is no falling back to the
/// filtered trace, since the comparison between the two is what this tab is.
const NO_RAW_GYRO: &str = "No gyroUnfilt in this log. Filter analysis works from the gyro \
                           before filtering — with only the filtered trace there is nothing to \
                           compare, and no way to see what the filters removed. Betaflight \
                           records it in debug mode GYRO_SCALED (`set debug_mode = \
                           GYRO_SCALED`, or the Debug mode dropdown in the configurator's \
                           Blackbox tab); fly again with it on.";

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
        // Built before the bar, because whether the two heatmaps have anything
        // to draw is what greys their tabs out. Rows are borrowed slices — the
        // cost is three `Option` checks each.
        let throttle_rows = vs_reference::rows(&ctx.analysis.spectral);
        let time_rows = spectrogram::rows(ctx);

        tab_bar(
            ui,
            &mut self.selected,
            &[
                (FilterAnalysisTab::Psd, "PSD", true),
                (FilterAnalysisTab::Frequency, "Frequency", true),
                (
                    FilterAnalysisTab::VsReference,
                    "Vs Reference",
                    !throttle_rows.is_empty(),
                ),
                (
                    FilterAnalysisTab::Spectrogram,
                    "Spectrogram",
                    !time_rows.is_empty(),
                ),
            ],
        );
        ui.add_space(4.0);

        // Every sub-tab here reads the pre-filter gyro, so they all go blank
        // together and for one reason. Said once, above the lot of them.
        if drawn_axes(&ctx.analysis.spectral) == 0 {
            ui.label(NO_RAW_GYRO);
            return;
        }

        match self.selected {
            FilterAnalysisTab::Psd => self.psd.show(ui, &ctx.analysis.spectral),
            FilterAnalysisTab::Frequency => self.frequency.show(ui, &ctx.analysis.spectral),
            FilterAnalysisTab::VsReference => {
                self.vs_reference
                    .show(ui, HeatmapKind::VsThrottle, throttle_rows)
            }
            FilterAnalysisTab::Spectrogram => {
                self.spectrogram
                    .show(ui, HeatmapKind::Spectrogram, time_rows)
            }
        }
    }
}
