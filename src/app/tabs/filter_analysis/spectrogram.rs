use egui_plot::LineStyle;
use elegance::Palette;

use super::heatmap_panel::HeatmapRow;
use crate::analysis::{FilterLoop, HarmonicBand, OverlayFamily};
use crate::app::colors;
use crate::app::tabs::TabCtx;
use crate::app::ui::harmonic_key;
use crate::app::ui::heatmap::OverlaySeries;
use crate::app::ui::overlay_menu::OverlayVisibility;
use crate::parser::{Axis, Channel, FlightData, Metadata};

/// What this panel can draw. Not every family: everything else in the menu is a
/// shape in frequency alone, and this is the one panel with a time axis.
pub(super) const FAMILIES: [OverlayFamily; 2] = [OverlayFamily::Harmonics, OverlayFamily::DynNotch];

/// Whether this log can fill one of them — the panel's own answer, because the
/// dynamic notch's *traced* centre is the only part of that family with a time
/// axis, and it needs a debug mode that a configured notch says nothing about.
/// A switch that ticks on and draws nothing is what the shared menu exists to
/// prevent.
pub(super) fn available(ctx: &TabCtx<'_>, family: OverlayFamily) -> bool {
    match family {
        OverlayFamily::Harmonics => !ctx.analysis.spectral.harmonic_bands().is_empty(),
        OverlayFamily::DynNotch => logs_trace(ctx),
        _ => false,
    }
}

fn logs_trace(ctx: &TabCtx<'_>) -> bool {
    ctx.metadata.logs_dyn_notch_trace() && ctx.flight.has_debug_axes()
}

/// Per-axis time-vs-frequency waterfall (raw signal power binned by time
/// instead of throttle). The overlays are what a frequency *did* over the
/// flight: each motor's harmonics as a curve, and — where the log was recorded
/// with debug mode `FFT_FREQ` — the dynamic notch tracker's live centre
/// frequency (`debug[0..3]`), so a mistracking or clamped tracker is visible
/// directly against the noise band it is supposed to be following.
pub(super) fn rows<'a>(ctx: &TabCtx<'a>) -> Vec<HeatmapRow<'a>> {
    Axis::ALL
        .iter()
        .filter_map(|&axis| {
            let spectrum = ctx.analysis.spectral.axis(axis)?.time_map.as_ref()?;
            Some(HeatmapRow {
                axis,
                spectrum,
                overlays: Vec::new(),
            })
        })
        .collect()
}

/// The curves the pilot ticked, added once the menu above the plots has been
/// drawn — so a toggle takes effect on the frame it is clicked rather than the
/// next one.
pub(super) fn attach_overlays<'a>(
    rows: &mut [HeatmapRow<'a>],
    ctx: &TabCtx<'a>,
    visibility: OverlayVisibility,
    palette: &Palette,
) {
    // The geometry the PSD's spans were built from, reused rather than
    // recomputed: it already carries which motors were spinning, how many
    // orders Betaflight can filter, and which of them carry a weight.
    let harmonics = match visibility.shows(OverlayFamily::Harmonics) {
        true => harmonic_series(
            ctx.flight,
            ctx.metadata,
            ctx.analysis.spectral.harmonic_bands(),
            palette,
        ),
        false => Vec::new(),
    };

    let trace = visibility.shows(OverlayFamily::DynNotch) && logs_trace(ctx);

    for row in rows {
        row.overlays = harmonics.clone();
        if let Some(samples) = trace.then(|| ctx.flight.debug_axis(row.axis)).flatten() {
            row.overlays.push(series(
                "tracked center freq".to_string(),
                ctx.flight,
                samples,
                1.0,
                colors::chain_color(palette, FilterLoop::Gyro),
                LineStyle::Solid,
            ));
        }
    }
}

/// One curve per band: that motor's eRPM, converted to hertz and multiplied by
/// the order. The four motors of a healthy quad overlap into one thick trace
/// and split exactly where one diverges from its siblings, which is the
/// diagnosis the overlay exists for.
fn harmonic_series<'a>(
    fd: &'a FlightData,
    metadata: &Metadata,
    bands: &[HarmonicBand],
    palette: &Palette,
) -> Vec<OverlaySeries<'a>> {
    bands
        .iter()
        .filter_map(|band| {
            let samples = fd.channel(Channel::Rpm(band.motor))?;
            Some(series(
                harmonic_key::band_name(band),
                fd,
                samples,
                // The one eRPM-to-hertz conversion, so a band on the PSD and a
                // curve here can never disagree about where a motor was.
                metadata.erpm_to_hz(1.0) * band.order as f64,
                harmonic_key::band_color(palette, band),
                harmonic_key::order_style(band.order),
            ))
        })
        .collect()
}

/// A channel the parser filled short of the time axis would be indexed past
/// its end by the plot's own window search, so both are cut to the shorter.
fn series<'a>(
    name: String,
    fd: &'a FlightData,
    samples: &'a [f64],
    scale: f64,
    color: egui::Color32,
    style: LineStyle,
) -> OverlaySeries<'a> {
    let n = fd.time_us().len().min(samples.len());

    OverlaySeries {
        name,
        t0: fd.start_us(),
        time_us: &fd.time_us()[..n],
        samples: &samples[..n],
        scale,
        color,
        style,
    }
}
