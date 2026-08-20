//! What colour is this line. One module rather than a constant per panel, and
//! one level above `tabs/` because the compare chips in `ui/` are the legend
//! for curves that `tabs/` draws — the two must not answer this differently.

use egui::{Color32, ecolor::HsvaGamma};
use elegance::{Palette, Theme};

use crate::parser::Axis;

/// How many flights a comparison can hold, and so how many colours it can
/// need. Four: past that the plot is mud and the chips no longer fit a row.
pub(in crate::app) const COMPARE_SLOTS: usize = 4;

/// Fixed hues — blue, orange, teal, magenta — so "log 3 is the teal one"
/// survives a theme toggle mid-session. Only luminance comes from the palette.
const SLOT_HUES: [f32; COMPARE_SLOTS] = [0.58, 0.08, 0.45, 0.87];

/// Fixed hues for the harmonic orders — amber, violet, spring green — in the
/// same spirit as the compare slots: "the amber band is the fundamental"
/// survives a theme toggle, and none of the three is one of Betaflight's
/// roll/pitch/yaw accents.
const HARMONIC_HUES: [f32; 3] = [0.11, 0.79, 0.35];

/// Gold for a detected peak, periwinkle for a configured filter. Both were
/// fixed RGB constants in the panels until now, which meant light mode drew
/// the dark theme's marks.
const PEAK_HUE: f32 = 0.14;
const FILTER_HUE: f32 = 0.63;

/// Saturation of the base flight's colour against the flights it is compared
/// against. Saturation rather than luminance is what steps the secondaries
/// back: dropping luminance would cost contrast against the background in one
/// theme or the other, whichever way it went.
const BASE_SATURATION: f32 = 0.95;
const COMPARED_SATURATION: f32 = 0.55;

/// The installed palette. Read per frame rather than captured: the app's
/// resolved light/dark state changes at any time (system theme, or the
/// sidepanel toggle).
pub(in crate::app) fn palette(ctx: &egui::Context) -> Palette {
    Theme::current(ctx).palette
}

/// Betaflight's own roll/red, pitch/green, yaw/blue, in the shade the current
/// palette draws that accent in — the pilot reads these three everywhere else
/// in the configurator.
pub(in crate::app) fn axis_color(palette: &Palette, axis: Axis) -> Color32 {
    match axis {
        Axis::Roll => palette.red,
        Axis::Pitch => palette.green,
        Axis::Yaw => palette.blue,
    }
}

/// The colour of a compare slot. Colour is the slot, never the flight: the base
/// flight is always slot 0, so switching the sidepanel changes which curve
/// wears a colour without moving the colour itself. Identity lives on the chip.
pub(in crate::app) fn slot_color(palette: &Palette, slot: usize) -> Color32 {
    let saturation = match slot {
        0 => BASE_SATURATION,
        _ => COMPARED_SATURATION,
    };
    hue_color(palette, SLOT_HUES[slot % COMPARE_SLOTS], saturation)
}

/// A detected noise peak.
pub(in crate::app) fn peak_color(palette: &Palette) -> Color32 {
    hue_color(palette, PEAK_HUE, BASE_SATURATION)
}

/// A configured filter's line or band — a reference the pilot set, not
/// something the craft did.
pub(in crate::app) fn filter_color(palette: &Palette) -> Color32 {
    hue_color(palette, FILTER_HUE, BASE_SATURATION)
}

/// A harmonic order's colour, 1 being the fundamental. Order, not motor: four
/// motors at one order are the same noise, and what a pilot needs to tell
/// apart is a fundamental from its multiples.
pub(in crate::app) fn harmonic_color(palette: &Palette, order: u32) -> Color32 {
    let index = order.max(1) as usize - 1;
    hue_color(palette, HARMONIC_HUES[index % HARMONIC_HUES.len()], 0.85)
}

/// Hue is the identity and survives a theme switch; only luminance comes from
/// the palette, so a mark keeps its contrast in both.
fn hue_color(palette: &Palette, hue: f32, saturation: f32) -> Color32 {
    HsvaGamma {
        h: hue,
        s: saturation,
        v: if palette.is_dark { 0.95 } else { 0.55 },
        a: 1.0,
    }
    .into()
}

#[cfg(test)]
mod test {
    use super::*;

    /// WCAG relative luminance, then its contrast ratio — a minimum rather
    /// than an exact colour, so a palette tweak in `elegance` cannot fail this
    /// suite for a change nobody can see.
    fn contrast(a: Color32, b: Color32) -> f32 {
        let luminance = |c: Color32| {
            let channel = |v: u8| {
                let v = v as f32 / 255.0;
                match v <= 0.03928 {
                    true => v / 12.92,
                    false => ((v + 0.055) / 1.055).powf(2.4),
                }
            };
            0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
        };

        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    /// Enough that a 2 px line reads as its own colour against the plot, well
    /// short of the 4.5 a body-text palette is held to.
    const MIN_CONTRAST: f32 = 2.5;

    fn palettes() -> [Palette; 2] {
        [Palette::charcoal(), Palette::paper()]
    }

    #[test]
    fn every_slot_is_a_different_colour_in_both_themes() {
        for palette in palettes() {
            for slot in 0..COMPARE_SLOTS {
                for other in slot + 1..COMPARE_SLOTS {
                    assert_ne!(
                        slot_color(&palette, slot),
                        slot_color(&palette, other),
                        "slots {slot} and {other} share a colour (dark: {})",
                        palette.is_dark
                    );
                }
            }
        }
    }

    #[test]
    fn every_mark_is_legible_on_the_background_it_is_drawn_on() {
        for palette in palettes() {
            let drawn = (0..COMPARE_SLOTS)
                .map(|slot| (format!("slot {slot}"), slot_color(&palette, slot)))
                .chain(Axis::ALL.map(|axis| (axis.name().to_string(), axis_color(&palette, axis))))
                .chain(
                    (1..=HARMONIC_HUES.len() as u32).map(|order| {
                        (format!("harmonic {order}"), harmonic_color(&palette, order))
                    }),
                )
                .chain([
                    ("peak".to_string(), peak_color(&palette)),
                    ("filter".to_string(), filter_color(&palette)),
                    ("warning".to_string(), palette.warning),
                ]);

            for (what, color) in drawn {
                let ratio = contrast(color, palette.bg);
                assert!(
                    ratio >= MIN_CONTRAST,
                    "{what} has {ratio:.2} contrast on the background (dark: {})",
                    palette.is_dark
                );
            }
        }
    }

    /// The defect this replaced: the axis colours were a fixed dark palette's,
    /// so light mode drew the dark theme's accents.
    #[test]
    fn axis_colours_follow_the_theme() {
        let [dark, light] = palettes();
        for axis in Axis::ALL {
            assert_ne!(
                axis_color(&dark, axis),
                axis_color(&light, axis),
                "{} is the same colour in both themes",
                axis.name()
            );
        }
    }

    /// A fundamental and its multiples have to be tellable apart at a glance,
    /// and none of them may be mistaken for an axis trace.
    #[test]
    fn harmonic_orders_are_distinct_from_each_other_and_from_the_axes() {
        for palette in palettes() {
            let orders: Vec<Color32> = (1..=HARMONIC_HUES.len() as u32)
                .map(|order| harmonic_color(&palette, order))
                .collect();

            for (i, &color) in orders.iter().enumerate() {
                for &other in &orders[i + 1..] {
                    assert_ne!(color, other, "two harmonic orders share a colour");
                }
                for axis in Axis::ALL {
                    assert_ne!(
                        color,
                        axis_color(&palette, axis),
                        "harmonic {} is the {} trace's colour",
                        i + 1,
                        axis.name()
                    );
                }
            }
        }
    }

    /// The order sequence wraps rather than panicking on a craft configured
    /// with more harmonics than there are hues.
    #[test]
    fn an_order_past_the_hue_sequence_wraps() {
        let palette = Palette::charcoal();
        let orders = HARMONIC_HUES.len() as u32;

        assert_eq!(
            harmonic_color(&palette, orders + 1),
            harmonic_color(&palette, 1)
        );
    }

    /// The defect this replaced, for the overlay marks this time: both were
    /// fixed RGB constants that drew the same in light mode as in dark.
    #[test]
    fn overlay_marks_follow_the_theme() {
        let [dark, light] = palettes();

        assert_ne!(peak_color(&dark), peak_color(&light));
        assert_ne!(filter_color(&dark), filter_color(&light));
        assert_ne!(harmonic_color(&dark, 1), harmonic_color(&light, 1));
    }
}
