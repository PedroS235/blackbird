//! The filter overlays as a heatmap reads them: frequencies on the map's own
//! two axes.
//!
//! A PSD can say how much a stage removed, because its y axis is power. A map's
//! two axes are frequency and either throttle or time, so what it can say is
//! *where* a stage sat — and, for a stage the firmware moved, where it sat at
//! each throttle or each moment. That second one is the thing the PSD cannot
//! draw at all: a dynamic cutoff is a curve against the stick, and the
//! spectrum can only ever show the average of it.
//!
//! One builder for both maps. Marks are built in `(bin, frequency)` and the
//! widget places them, so nothing here knows which of the two it is drawing on.

use egui_plot::LineStyle;
use elegance::Palette;

use crate::analysis::{FilterOverlay, OverlayFamily, OverlayShape};
use crate::app::colors;
use crate::app::ui::heatmap::{Mark, OverlayMark};

/// Solid for what a stage *did*, dashed for what it was only *allowed* to do —
/// the same distinction the PSD draws by putting configured bounds in the dwell
/// lane instead of over the spectrum.
const DID: LineStyle = LineStyle::Solid;
const ALLOWED: LineStyle = LineStyle::Dashed { length: 8.0 };

/// Every mark one overlay contributes to a map.
///
/// `throttle_at` turns a bin into the throttle the craft was holding there, as
/// the log records it: on the throttle map that is the bin itself, and on the
/// spectrogram it is the stick at that moment. `None` for a bin the log cannot
/// answer for, which drops that point rather than inventing a cutoff.
///
/// The two families that need the log rather than the header — the motor
/// harmonics and the notch tracker's own trace — are the panel's own business
/// and come back empty here.
pub(super) fn marks(
    overlay: &FilterOverlay,
    bins: &[f64],
    throttle_at: &dyn Fn(f64) -> Option<f64>,
    palette: &Palette,
) -> Vec<OverlayMark> {
    let Some(loop_) = overlay.family.filter_loop() else {
        return Vec::new();
    };
    let color = colors::chain_color(palette, loop_);
    let mark = |mark, style| OverlayMark {
        name: overlay.label.clone(),
        mark,
        color,
        style,
    };

    // What the firmware drove, drawn as it drove it. Exact on these axes, where
    // the spectrum could only show the dwell-weighted average of it.
    if let Some(driven) = &overlay.driven {
        let points: Vec<[f64; 2]> = bins
            .iter()
            .filter_map(|&bin| Some([bin, driven.setting_at(throttle_at(bin)?)]))
            .collect();
        if points.len() >= 2 {
            return vec![mark(Mark::Curve(points), DID)];
        }
    }

    match &overlay.shape {
        // A notch is read at its null and a lowpass at its corner: on a map
        // there is no depth to draw, so the one frequency drawn has to be the
        // one the pilot would name the stage by.
        OverlayShape::Response(response) => match overlay.family {
            OverlayFamily::Notch(_) => response.deepest(),
            _ => response.corner(),
        }
        .map(|(hz, _)| vec![mark(Mark::Level(hz), DID)])
        .unwrap_or_default(),
        OverlayShape::Line { hz } => vec![mark(Mark::Level(*hz), DID)],
        // Both ends of what it was allowed, and nothing between them: with no
        // throttle logged there is no curve, and a filled band across a map
        // would claim every frequency inside it was cut.
        OverlayShape::Allowed { low_hz, high_hz } => vec![
            mark(Mark::Level(*low_hz), ALLOWED),
            mark(Mark::Level(*high_hz), ALLOWED),
        ],
        OverlayShape::Envelope { low, high } => [low.corner(), high.corner()]
            .into_iter()
            .flatten()
            .map(|(hz, _)| mark(Mark::Level(hz), ALLOWED))
            .collect(),
        // The tracker's own centre is per axis and lives in the log, so the
        // panel that holds the flight data draws it.
        OverlayShape::Traced(_) | OverlayShape::Harmonics(_) => Vec::new(),
    }
}

/// Whether this log can fill a family's toggle on a map. The same walk the PSD
/// does: a family with no overlay has nothing to draw, whatever the panel.
pub(super) fn available(overlays: &[FilterOverlay], family: OverlayFamily) -> bool {
    overlays.iter().any(|o| o.family == family)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::analysis::{ByAxis, Driven, FilterLoop};
    use crate::parser::metadata::{FilterType, LowpassConfig};

    fn palette() -> Palette {
        Palette::charcoal()
    }

    fn overlay(family: OverlayFamily, shape: OverlayShape) -> FilterOverlay {
        FilterOverlay {
            label: "stage".to_string(),
            family,
            shape,
            gain: None,
            dwell: None,
            driven: None,
        }
    }

    fn stick_bins() -> Vec<f64> {
        (0..=10).map(|i| 1000.0 + i as f64 * 100.0).collect()
    }

    fn on_throttle(overlay: &FilterOverlay) -> Vec<OverlayMark> {
        marks(overlay, &stick_bins(), &|bin| Some(bin), &palette())
    }

    fn dynamic_lpf() -> LowpassConfig {
        LowpassConfig {
            static_hz: 0.0,
            dyn_min_hz: 250.0,
            dyn_max_hz: 500.0,
            dyn_expo: 0.0,
            filter_type: FilterType::Pt1,
        }
    }

    /// The thing the PSD cannot draw: on axes where the cutoff *is* a function
    /// of the stick, the stage is an exact rising curve rather than the smear
    /// its dwell-weighted average collapses to.
    #[test]
    fn a_dynamic_lowpass_is_a_rising_curve_against_the_stick() {
        let mut lpf = overlay(
            OverlayFamily::Lowpass(FilterLoop::Gyro),
            OverlayShape::Allowed {
                low_hz: 250.0,
                high_hz: 500.0,
            },
        );
        lpf.driven = Some(Driven::Throttle(dynamic_lpf()));

        let drawn = on_throttle(&lpf);
        assert_eq!(drawn.len(), 1);
        let Mark::Curve(points) = &drawn[0].mark else {
            panic!("a driven stage is a curve");
        };

        assert_eq!(points.len(), stick_bins().len());
        // Betaflight's own `dynLpfCutoffFreq`, not a straight line between the
        // ends: it holds at the floor over the bottom of the range, climbs
        // through the middle, and tops out a shade over the configured maximum
        // just short of full stick. The curve drawn is the firmware's, kinks
        // and all — a straight line would be a nicer picture of a filter the
        // craft never ran.
        assert!((points[0][1] - 250.0).abs() < 1.0, "{:?}", points[0]);
        assert!(
            points[5][1] > 300.0 && points[5][1] < 450.0,
            "{:?}",
            points[5]
        );
        assert!(points[10][1] > 450.0, "{:?}", points[10]);
        assert!(
            points[..9].windows(2).all(|w| w[1][1] >= w[0][1]),
            "{points:?}"
        );
    }

    /// A map with nothing to say about the throttle at a bin cannot place a
    /// driven curve, and falls back to the geometry that needs no throttle.
    #[test]
    fn without_a_throttle_a_driven_stage_falls_back_to_its_ends() {
        let mut lpf = overlay(
            OverlayFamily::Lowpass(FilterLoop::Gyro),
            OverlayShape::Allowed {
                low_hz: 250.0,
                high_hz: 500.0,
            },
        );
        lpf.driven = Some(Driven::Throttle(dynamic_lpf()));

        let drawn = marks(&lpf, &stick_bins(), &|_| None, &palette());
        assert_eq!(drawn.len(), 2);
        assert!(drawn.iter().all(|m| m.style == ALLOWED));
    }

    /// A notch is named by its null and a lowpass by its corner, so that is
    /// the frequency each one draws where a map has no depth to show.
    #[test]
    fn a_notch_is_read_at_its_null_and_a_lowpass_at_its_corner() {
        let response = crate::analysis::filter_response::of(
            crate::analysis::Stage::Notch {
                centre_hz: 300.0,
                q: 6.0,
            },
            8000.0,
        )
        .unwrap();

        let as_notch = on_throttle(&overlay(
            OverlayFamily::Notch(FilterLoop::Gyro),
            OverlayShape::Response(response.clone()),
        ));
        let as_lowpass = on_throttle(&overlay(
            OverlayFamily::Lowpass(FilterLoop::Gyro),
            OverlayShape::Response(response),
        ));

        let level = |marks: &[OverlayMark]| match marks[0].mark {
            Mark::Level(hz) => hz,
            _ => panic!("a static stage is a level"),
        };
        assert!((level(&as_notch) - 300.0).abs() < 5.0);
        assert!(level(&as_lowpass) < level(&as_notch));
    }

    /// The two chains stay two colours here as well — a D-term stage on a gyro
    /// power map is a frequency reference, and has to read as one.
    #[test]
    fn the_two_chains_keep_their_own_colours() {
        let gyro = on_throttle(&overlay(
            OverlayFamily::Notch(FilterLoop::Gyro),
            OverlayShape::Line { hz: 200.0 },
        ));
        let dterm = on_throttle(&overlay(
            OverlayFamily::Notch(FilterLoop::Dterm),
            OverlayShape::Line { hz: 200.0 },
        ));

        assert_ne!(gyro[0].color, dterm[0].color);
    }

    /// The families that live in the log, not the header, are the panel's own
    /// business — and the harmonics are not a filter stage at all.
    #[test]
    fn the_log_shaped_families_draw_nothing_here() {
        let harmonics = overlay(
            OverlayFamily::Harmonics,
            OverlayShape::Harmonics(Vec::new()),
        );
        assert!(on_throttle(&harmonics).is_empty());

        let mut traced = overlay(
            OverlayFamily::DynNotch,
            OverlayShape::Traced(crate::parser::PerAxis::default()),
        );
        traced.gain = Some(ByAxis::Shared(Vec::new()));
        assert!(on_throttle(&traced).is_empty());
    }
}
