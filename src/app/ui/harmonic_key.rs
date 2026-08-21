//! One identity scheme for motor harmonics, and the legend that keys it.
//!
//! **Hue is the motor, line style is the harmonic order.** The PSD draws the
//! bands and the Spectrogram draws the curves, and a pilot who learns the key
//! on one reads the other — so the two panels ask this module rather than each
//! choosing for themselves.

use egui::{Color32, RichText, Sense, Shape, Stroke, Ui, Vec2, pos2};
use egui_plot::LineStyle;
use elegance::Palette;

use crate::analysis::HarmonicBand;
use crate::app::colors;

/// Solid, dashed, dotted for the fundamental and its multiples. Three styles
/// and no more: past dotted the marks stop being tellable apart, so any higher
/// order draws as the third rather than inventing a fourth. How many orders
/// there *are* is the analysis layer's answer, not this module's.
pub(in crate::app) fn order_style(order: u32) -> LineStyle {
    match order {
        1 => LineStyle::Solid,
        2 => LineStyle::dashed_dense(),
        _ => LineStyle::dotted_dense(),
    }
}

/// A band's colour: its motor's hue, dimmed where the RPM filter tracks the
/// order and takes nothing off it.
pub(in crate::app) fn band_color(palette: &Palette, band: &HarmonicBand) -> Color32 {
    let color = colors::motor_color(palette, band.motor);
    match band.filtered {
        true => color,
        false => colors::dimmed(color),
    }
}

/// What the hover says a mark is. Motors are 1-based here — the pilot's motor 1
/// is Betaflight's, not the index the log stores it at.
pub(in crate::app) fn band_name(band: &HarmonicBand) -> String {
    let tracked = match band.filtered {
        true => "",
        false => " (unfiltered)",
    };
    format!("M{} H{}{tracked}", band.motor + 1, band.order)
}

/// Width of a legend sample. Enough that a dashed one reads as dashed.
const SAMPLE_WIDTH: f32 = 22.0;
const SAMPLE_STROKE: f32 = 1.6;

/// The key, one wrapped row: a swatch per motor the log carries, then a sample
/// per order drawn. Twelve outlines over a spectrum cannot carry twelve labels,
/// and without a key the hues say nothing.
pub(in crate::app) fn show(ui: &mut Ui, palette: &Palette, bands: &[HarmonicBand]) {
    // The motors that actually drew something, not `0..=max`: a motor that
    // never spun has no band, and a swatch keying a hue nothing draws sends a
    // pilot looking for a curve that is not there.
    let mut motors: Vec<usize> = bands.iter().map(|b| b.motor).collect();
    motors.sort_unstable();
    motors.dedup();
    let orders = bands.iter().map(|b| b.order).max().unwrap_or(0);

    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Motor").weak());
        for motor in motors {
            sample(ui, colors::motor_color(palette, motor), LineStyle::Solid);
            ui.label(format!("{}", motor + 1));
        }

        ui.add_space(8.0);
        ui.label(RichText::new("Harmonic").weak());
        for order in 1..=orders {
            sample(ui, palette.text_muted, order_style(order));
            ui.label(format!("{order}"));
        }
    });
}

/// A short line in the colour and style a plot would draw it in. Painted rather
/// than described in text: the mark on the plot is what the pilot has to match,
/// and a written "dashed" is one indirection away from it.
fn sample(ui: &mut Ui, color: Color32, style: LineStyle) {
    let height = ui.text_style_height(&egui::TextStyle::Body);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(SAMPLE_WIDTH, height), Sense::hover());
    let y = rect.center().y;
    let stroke = Stroke::new(SAMPLE_STROKE, color);

    let path = [pos2(rect.left(), y), pos2(rect.right(), y)];

    let shapes = match style {
        LineStyle::Solid => vec![Shape::line_segment(path, stroke)],
        LineStyle::Dashed { length } => Shape::dashed_line(&path, stroke, length, length),
        LineStyle::Dotted { spacing } => Shape::dotted_line(&path, color, spacing, SAMPLE_STROKE),
    };
    ui.painter().extend(shapes);
}

#[cfg(test)]
mod test {
    use super::*;

    fn band(motor: usize, order: u32, filtered: bool) -> HarmonicBand {
        HarmonicBand {
            motor,
            order,
            low_hz: 100.0,
            high_hz: 200.0,
            filtered,
        }
    }

    /// The fundamental is the solid one, and no two of the three orders share
    /// a style — the whole point of spending the line style on the order.
    #[test]
    fn each_order_has_its_own_style_and_the_fundamental_is_solid() {
        assert_eq!(order_style(1), LineStyle::Solid);

        let styles = [order_style(1), order_style(2), order_style(3)];
        for (i, style) in styles.iter().enumerate() {
            for other in &styles[i + 1..] {
                assert_ne!(style, other, "two orders share a line style");
            }
        }
    }

    /// Two bands of the same order on different motors must be tellable apart,
    /// and two orders on the same motor must not be told apart by colour —
    /// that is the style's job.
    #[test]
    fn colour_says_the_motor_and_nothing_else() {
        let palette = Palette::charcoal();

        assert_ne!(
            band_color(&palette, &band(0, 1, true)),
            band_color(&palette, &band(1, 1, true))
        );
        assert_eq!(
            band_color(&palette, &band(0, 1, true)),
            band_color(&palette, &band(0, 2, true))
        );
    }

    /// A tracked-but-unattenuated order keeps its identity and reads as a step
    /// back from the orders that are actually being filtered.
    #[test]
    fn an_unfiltered_order_is_the_same_identity_dimmed() {
        let palette = Palette::charcoal();

        assert_ne!(
            band_color(&palette, &band(0, 2, false)),
            band_color(&palette, &band(0, 2, true))
        );
        assert_eq!(order_style(2), order_style(2));
    }

    /// The pilot's motor 1 is Betaflight's, not the index the log stores it at.
    #[test]
    fn the_hover_names_the_motor_the_pilot_numbers_it() {
        assert_eq!(band_name(&band(0, 1, true)), "M1 H1");
        assert_eq!(band_name(&band(3, 2, false)), "M4 H2 (unfiltered)");
    }
}
