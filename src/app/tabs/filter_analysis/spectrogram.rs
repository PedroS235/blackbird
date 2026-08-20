use super::heatmap_panel::HeatmapRow;
use crate::app::tabs::TabCtx;
use crate::app::ui::heatmap::OverlaySeries;
use crate::parser::Axis;

/// Per-axis time-vs-frequency waterfall (raw signal power binned by time
/// instead of throttle). When the log was recorded with debug mode
/// `FFT_FREQ`, overlays the dynamic notch tracker's live center frequency
/// (`debug[0..3]`) on top, so a mistracking or clamped tracker is visible
/// directly against the noise band it's supposed to be following.
pub(super) fn rows<'a>(ctx: &TabCtx<'a>) -> Vec<HeatmapRow<'a>> {
    let fd = ctx.flight;
    let has_dyn_notch_trace = ctx.metadata.logs_dyn_notch_trace() && fd.has_debug_axes();
    let t0 = fd.start_us();

    Axis::ALL
        .iter()
        .filter_map(|&axis| {
            let spectrum = ctx.analysis.spectral.axis(axis)?.time_map.as_ref()?;
            let overlay = has_dyn_notch_trace
                .then(|| fd.debug_axis(axis))
                .flatten()
                .map(|samples| OverlaySeries {
                    t0,
                    time_us: fd.time_us(),
                    samples,
                });
            Some(HeatmapRow {
                axis,
                spectrum,
                overlay,
            })
        })
        .collect()
}
