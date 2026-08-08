use crate::parser::PerAxis;

/// Per-tab UI state for `MainTab::FilterAnalysis`. `psd_filtered_visible` and
/// `frequency_filtered_visible` are deliberately separate fields — they used
/// to be the same field shared across the Psd and Frequency sub-tabs, which
/// meant toggling one silently toggled the other.
///
/// The timeseries family of plots has no state here: its series visibility is
/// the plot legend's, and so lives in `egui_plot`'s per-plot memory.
pub(super) struct FilterAnalysisTabState {
    pub(super) psd_filtered_visible: PerAxis<bool>,
    pub(super) frequency_filtered_visible: PerAxis<bool>,
    pub(super) frequency_peak_min_hz: f32,
    pub(super) heatmap_floor_db: f32,
    pub(super) spectrogram_floor_db: f32,
}

impl Default for FilterAnalysisTabState {
    fn default() -> Self {
        Self {
            psd_filtered_visible: PerAxis::splat(false),
            frequency_filtered_visible: PerAxis::splat(false),
            frequency_peak_min_hz: 100.0,
            heatmap_floor_db: -60.0,
            spectrogram_floor_db: -60.0,
        }
    }
}

#[derive(Default)]
pub(super) struct MainViewState {
    pub(super) filter_analysis: FilterAnalysisTabState,
}
