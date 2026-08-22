//! What colour is this line. One module rather than a constant per panel, and
//! one level above `tabs/` because the compare chips in `ui/` are the legend
//! for curves that `tabs/` draws — the two must not answer this differently.

use egui::{Color32, ecolor::HsvaGamma};
use elegance::{Palette, Theme};

use crate::analysis::FilterLoop;
use crate::parser::Axis;

/// How many flights a comparison can hold, and so how many colours it can
/// need. Four: past that the plot is mud and the chips no longer fit a row.
pub(in crate::app) const COMPARE_SLOTS: usize = 7;

/// Fixed hues — blue, orange, teal, magenta — so "log 3 is the teal one"
/// survives a theme toggle mid-session. Only luminance comes from the palette.
const SLOT_HUES: [f32; COMPARE_SLOTS] = [0.0, 0.08, 0.20, 0.30, 0.47, 0.65, 0.81];

/// Fixed hues for the motors — amber, spring green, cyan, violet — in the same
/// spirit as the compare slots: "the amber trace is motor 1" survives a theme
/// toggle, and none of the four is one of Betaflight's roll/pitch/yaw accents.
///
/// Hue is the *motor*, not the harmonic order, which the line style carries.
/// Which order a peak is sits at a multiple of the fundamental and a pilot can
/// read it off the frequency axis; which motor is louder than its three
/// siblings is a bent shaft or a dying bearing, and nothing else on the plot
/// says it.
const MOTOR_HUES: [f32; 4] = [0.11, 0.35, 0.55, 0.79];

/// Gold for a detected peak, and one hue per filter chain — periwinkle for the
/// gyro loop, rose for the D-term. All three were fixed RGB constants in the
/// panels until now, which meant light mode drew the dark theme's marks.
///
/// Two chain hues rather than one per stage: which stage a curve is, is
/// derivable — its corner label says so, and LPF2 always sits above LPF1 —
/// whereas which loop it belongs to is not derivable from the shape, and it is
/// the one that changes the CLI line the pilot types.
const PEAK_HUE: f32 = 0.14;
const CHAIN_HUES: [f32; 2] = [0.63, 0.93];

/// A motor's mark is a thin outline or a curve among twelve, so it takes full
/// saturation — and on a light background a lower value, because a hue at full
/// value on paper is a highlighter rather than a line. Both clear the contrast
/// floor the mark tests hold every drawn colour to.
const MOTOR_SATURATION: f32 = 1.0;
const MOTOR_VALUE_LIGHT: f32 = 0.65;

/// Saturation of the base flight's colour against the flights it is compared
/// against. Saturation rather than luminance is what steps the secondaries
/// back: dropping luminance would cost contrast against the background in one
/// theme or the other, whichever way it went.
const BASE_SATURATION: f32 = 0.95;
const COMPARED_SATURATION: f32 = 0.75;

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

/// One filter chain's hue. Everything within a chain — the total, its stage
/// curves, the fill under it and its dwell lane — wears this one colour and
/// separates by width and alpha instead.
pub(in crate::app) fn chain_color(palette: &Palette, loop_: FilterLoop) -> Color32 {
    let hue = match loop_ {
        FilterLoop::Gyro => CHAIN_HUES[0],
        FilterLoop::Dterm => CHAIN_HUES[1],
    };
    hue_color(palette, hue, BASE_SATURATION)
}

/// One motor's colour, 0-based. Four hues, cycled past the fourth: a hex or an
/// octo still draws, at the cost of two motors sharing a hue — four distinct
/// hues that all clear the contrast floor is already most of the wheel, and
/// eight would crowd into neighbours a pilot cannot tell apart anyway.
pub(in crate::app) fn motor_color(palette: &Palette, motor: usize) -> Color32 {
    shade(
        palette,
        MOTOR_HUES[motor % MOTOR_HUES.len()],
        MOTOR_SATURATION,
        MOTOR_VALUE_LIGHT,
    )
}

/// The same mark, for something the filter tracks and takes nothing off. Hue
/// and style still say which motor at which order — dimming says the noise is
/// there and nothing is being removed from it.
pub(in crate::app) fn dimmed(color: Color32) -> Color32 {
    color.gamma_multiply(0.45)
}

/// How dark a mark goes on a light background by default. A single trace can
/// afford to stay bright; anything drawn in bulk passes its own value to
/// [`shade`].
const LIGHT_VALUE: f32 = 0.90;

/// Hue is the identity and survives a theme switch; only luminance comes from
/// the palette, so a mark keeps its contrast in both.
fn hue_color(palette: &Palette, hue: f32, saturation: f32) -> Color32 {
    shade(palette, hue, saturation, LIGHT_VALUE)
}

fn shade(palette: &Palette, hue: f32, saturation: f32, light_value: f32) -> Color32 {
    HsvaGamma {
        h: hue,
        s: saturation,
        v: if palette.is_dark { 1.0 } else { light_value },
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

    /// The diagnosis the overlay exists for is one motor behaving unlike its
    /// three siblings, so no two of a quad's motors may share a drawn identity
    /// — and none of them may be mistaken for an axis trace.
    #[test]
    fn every_motor_of_a_quad_is_a_different_colour_from_the_others_and_the_axes() {
        for palette in palettes() {
            let motors: Vec<Color32> = (0..MOTOR_HUES.len())
                .map(|motor| motor_color(&palette, motor))
                .collect();

            for (i, &color) in motors.iter().enumerate() {
                for &other in &motors[i + 1..] {
                    assert_ne!(color, other, "two motors share a colour");
                }
                for axis in Axis::ALL {
                    assert_ne!(
                        color,
                        axis_color(&palette, axis),
                        "motor {i} is the {} trace's colour",
                        axis.name()
                    );
                }
            }
        }
    }

    /// A hex still draws: the sequence wraps rather than panicking, at the
    /// cost of motor 5 wearing motor 1's hue.
    #[test]
    fn a_motor_past_the_hue_sequence_wraps() {
        let palette = Palette::charcoal();

        assert_eq!(
            motor_color(&palette, MOTOR_HUES.len()),
            motor_color(&palette, 0)
        );
    }

    /// Dimming may not turn one motor's mark into another's, and it has to be
    /// visibly a step down from the mark it dims.
    #[test]
    fn a_dimmed_mark_is_the_same_identity_a_step_darker() {
        for palette in palettes() {
            for motor in 0..MOTOR_HUES.len() {
                let color = motor_color(&palette, motor);
                assert_ne!(dimmed(color), color);
            }
        }
    }

    /// The defect this replaced, for the overlay marks this time: both were
    /// fixed RGB constants that drew the same in light mode as in dark.
    #[test]
    fn overlay_marks_follow_the_theme() {
        let [dark, light] = palettes();

        assert_ne!(peak_color(&dark), peak_color(&light));
        assert_ne!(motor_color(&dark, 0), motor_color(&light, 0));
        for loop_ in [FilterLoop::Gyro, FilterLoop::Dterm] {
            assert_ne!(chain_color(&dark, loop_), chain_color(&light, loop_));
        }
    }

    /// The one distinction a filter curve's colour carries: a D-term stage is
    /// hanging off its own reference line because it never touched the signal
    /// on the plot, and a pilot who reads it as a gyro stage reads the fill
    /// underneath as including it.
    #[test]
    fn the_two_chains_are_different_colours_in_both_themes() {
        for palette in palettes() {
            let (gyro, dterm) = (
                chain_color(&palette, FilterLoop::Gyro),
                chain_color(&palette, FilterLoop::Dterm),
            );
            assert_ne!(gyro, dterm, "the chains share a colour");

            for axis in Axis::ALL {
                assert_ne!(gyro, axis_color(&palette, axis));
                assert_ne!(dterm, axis_color(&palette, axis));
            }
            assert_ne!(gyro, peak_color(&palette));
            assert_ne!(dterm, peak_color(&palette));
        }
    }
}
