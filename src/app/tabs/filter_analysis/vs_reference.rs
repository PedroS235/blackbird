use elegance::Palette;

use super::filter_marks;
use super::heatmap_panel::HeatmapRow;
use crate::analysis::{FilterLoop, HarmonicBand, OverlayFamily, OverlayShape, SpectralAnalysis};
use crate::app::colors;
use crate::app::tabs::TabCtx;
use crate::app::ui::harmonic_key;
use crate::app::ui::heatmap::{Mark, OverlayMark};
use crate::app::ui::overlay_menu::OverlayVisibility;
use crate::parser::{Axis, Channel};

/// Every family, and for the reason the spectrogram lists only two: this map's
/// y axis is the stick, so every filter has something true to say on it — a
/// static stage the frequency it sat at, and a dynamic one the whole curve it
/// followed, which is the one thing no spectrum can draw.
pub(super) const FAMILIES: [OverlayFamily; OverlayFamily::ALL.len()] = OverlayFamily::ALL;

pub(super) fn available(ctx: &TabCtx<'_>, family: OverlayFamily) -> bool {
    filter_marks::available(&ctx.analysis.spectral.overlays, family)
}

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
                marks: Vec::new(),
            })
        })
        .collect()
}

/// The marks the pilot ticked, added once the menu above the plots has been
/// drawn — so a toggle takes effect on the frame it is clicked.
///
/// Everything here is a frequency against the stick. The filter geometry comes
/// from the overlays, and the two things that live in the log rather than the
/// header — each motor's harmonics and the notch tracker's own centre — are
/// binned by throttle here, the same bins the map itself is binned into.
pub(super) fn attach_marks(
    rows: &mut [HeatmapRow<'_>],
    ctx: &TabCtx<'_>,
    visibility: OverlayVisibility,
    palette: &Palette,
) {
    let overlays = &ctx.analysis.spectral.overlays;
    let visible: Vec<_> = overlays
        .iter()
        .filter(|o| visibility.shows(o.family))
        .collect();
    let throttle = ctx.flight.throttle();

    for row in rows {
        let bins = &row.spectrum.bin_centers;
        // The bins *are* throttle, as the log records it, so a stage the stick
        // drove is drawn against the very axis that drove it.
        let mut marks: Vec<OverlayMark> = visible
            .iter()
            .flat_map(|overlay| filter_marks::marks(overlay, bins, &|bin| Some(bin), palette))
            .collect();

        if visibility.shows(OverlayFamily::Harmonics) {
            marks.extend(harmonic_marks(ctx, bins, palette));
        }

        // The tracker chased noise, not the stick, so where it went against
        // throttle is a measurement — one curve per axis, from the centre the
        // firmware logged.
        if visibility.shows(OverlayFamily::DynNotch)
            && let Some(traced) = visible
                .iter()
                .find(|o| matches!(o.shape, OverlayShape::Traced(_)))
            && let (Some(throttle), Some(centres)) = (throttle, ctx.flight.debug_axis(row.axis))
            && let Some(points) = binned(throttle, centres, bins)
        {
            marks.push(OverlayMark {
                name: traced.label.clone(),
                mark: Mark::Curve(points),
                color: colors::chain_color(palette, FilterLoop::Gyro),
                style: egui_plot::LineStyle::Solid,
            });
        }

        row.marks = marks;
    }
}

/// One curve per motor per order: the frequency that motor ran at, at each
/// throttle. A healthy quad's four curves lie on top of each other and split
/// exactly where one motor is working harder than its siblings — the same
/// diagnosis the spectrogram's curves make against time.
fn harmonic_marks(ctx: &TabCtx<'_>, bins: &[f64], palette: &Palette) -> Vec<OverlayMark> {
    let Some(throttle) = ctx.flight.throttle() else {
        return Vec::new();
    };

    ctx.analysis
        .spectral
        .harmonic_bands()
        .iter()
        .filter_map(|band: &HarmonicBand| {
            let erpm = ctx.flight.channel(Channel::Rpm(band.motor))?;
            let points = binned(throttle, erpm, bins)?
                .into_iter()
                // The one eRPM-to-hertz conversion, so a band on the PSD and a
                // curve here can never disagree about where a motor was.
                .map(|[bin, erpm]| [bin, ctx.metadata.erpm_to_hz(erpm) * band.order as f64])
                .collect();

            Some(OverlayMark {
                name: harmonic_key::band_name(band),
                mark: Mark::Curve(points),
                color: harmonic_key::band_color(palette, band),
                style: harmonic_key::order_style(band.order),
            })
        })
        .collect()
}

/// The mean of `samples` in each bin of `reference`, as `(bin, mean)` pairs.
///
/// A bin nothing fell into is dropped rather than interpolated across: the
/// heatmap leaves that row blank for the same reason, and a curve drawn through
/// a throttle the pilot never held is a frequency the craft never ran at.
/// Zeroes are dropped with it — a stopped motor and a tracker with nothing to
/// report are both logged as zero, and neither is a frequency.
fn binned(reference: &[f64], samples: &[f64], bins: &[f64]) -> Option<Vec<[f64; 2]>> {
    let (first, last) = (*bins.first()?, *bins.last()?);
    let width = match bins.len() {
        0 | 1 => return None,
        n => (last - first) / (n - 1) as f64,
    };

    let mut sum = vec![0.0; bins.len()];
    let mut count = vec![0u32; bins.len()];
    for (&at, &value) in reference.iter().zip(samples) {
        if !(at.is_finite() && value > 0.0) {
            continue;
        }
        let bin = (((at - first) / width).round() as isize).clamp(0, bins.len() as isize - 1);
        sum[bin as usize] += value;
        count[bin as usize] += 1;
    }

    let points: Vec<[f64; 2]> = bins
        .iter()
        .zip(sum.iter().zip(&count))
        .filter(|&(_, (_, &count))| count > 0)
        .map(|(&bin, (&sum, &count))| [bin, sum / count as f64])
        .collect();

    (points.len() >= 2).then_some(points)
}

#[cfg(test)]
mod test {
    use super::*;

    /// Ten stick bins, a hundred apart.
    fn bins() -> Vec<f64> {
        (0..10).map(|i| 1000.0 + i as f64 * 100.0).collect()
    }

    /// The curve is a mean per bin, so a motor held at one eRPM under load
    /// reads as that eRPM at the throttle it was held at, and nowhere else.
    #[test]
    fn a_channel_is_averaged_into_the_bin_the_stick_was_in() {
        let throttle = [1000.0, 1000.0, 1900.0, 1900.0];
        let erpm = [1000.0, 2000.0, 5000.0, 5000.0];

        let points = binned(&throttle, &erpm, &bins()).expect("two bins were visited");
        assert_eq!(points, vec![[1000.0, 1500.0], [1900.0, 5000.0]]);
    }

    /// A throttle the pilot never held has no frequency to draw, and a curve
    /// joined across it would run through frequencies the craft never saw.
    #[test]
    fn a_bin_nothing_fell_into_is_left_out() {
        let throttle = [1000.0, 1500.0, 1900.0];
        let erpm = [1000.0, 3000.0, 5000.0];

        let points = binned(&throttle, &erpm, &bins()).expect("three bins were visited");
        assert_eq!(points.len(), 3);
        assert!(points.iter().all(|&[bin, _]| bin != 1100.0));
    }

    /// A stopped motor is logged as zero eRPM, which is not a frequency — and
    /// a bin with nothing but zeroes in it is a bin with nothing in it.
    #[test]
    fn zeroes_are_not_a_frequency() {
        let throttle = [1000.0, 1000.0, 1900.0, 1900.0];
        let erpm = [0.0, 0.0, 5000.0, 5000.0];

        assert_eq!(binned(&throttle, &erpm, &bins()), None);
    }

    /// One bin cannot make a curve, and neither can no bins.
    #[test]
    fn a_map_with_nothing_to_bin_against_draws_no_curve() {
        assert_eq!(binned(&[1500.0], &[3000.0], &bins()), None);
        assert_eq!(binned(&[1500.0], &[3000.0], &[]), None);
    }
}
