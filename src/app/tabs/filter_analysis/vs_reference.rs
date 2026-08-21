use super::heatmap_panel::HeatmapRow;
use crate::analysis::SpectralAnalysis;
use crate::parser::Axis;

/// Per-axis throttle-vs-frequency map: raw signal power binned by throttle, so
/// noise that only appears under load is visible where it appears.
pub(super) fn rows(analysis: &SpectralAnalysis) -> Vec<HeatmapRow<'_>> {
    Axis::ALL
        .iter()
        .filter_map(|&axis| {
            let spectrum = analysis.axis(axis)?.throttle_map.as_ref()?;
            Some(HeatmapRow {
                axis,
                spectrum,
                overlays: Vec::new(),
            })
        })
        .collect()
}
