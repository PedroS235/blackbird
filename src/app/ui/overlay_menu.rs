//! Which reference overlays the pilot has asked to see, and the row of toggles
//! that asks.
//!
//! Laid out inline rather than behind a dropdown. A button that opens a menu
//! announces nothing — a pilot has to already know the overlays exist to go
//! looking for them, and every family is off by default, so nothing on the
//! plot hints that there is anything to find. The row costs one line, and
//! wraps to a second on a narrow window rather than pushing the plots down.

use egui::{RichText, Ui};

use crate::analysis::{FilterLoop, OverlayFamily};

/// What the panel does with an overlay, and so what its hover has to say. The
/// PSD draws gain against the spectrum; a heatmap draws the same filters as
/// frequencies on its own two axes, and a hover promising a fill under the raw
/// trace would be describing a panel the pilot is not looking at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app) enum Drawn {
    OnSpectrum,
    OnMap,
}

/// Row order is `OverlayFamily::ALL`'s order, and a family's position in it is
/// that family's visibility flag — so a family the analysis can produce always
/// has a toggle, and no two families can share one.
const MENU_ORDER: [OverlayFamily; OverlayFamily::ALL.len()] = OverlayFamily::ALL;

/// The toggle's label. Short, because seven of them share a row: enough to
/// name the filter, with the detail left to the hover.
fn title(family: OverlayFamily) -> String {
    match family {
        OverlayFamily::Harmonics => "Harmonics".to_string(),
        OverlayFamily::DynNotch => "Dyn notch".to_string(),
        OverlayFamily::Notch(l) => format!("{} notch", l.name()),
        OverlayFamily::Lowpass(l) => format!("{} LPF", l.name()),
    }
}

/// What the toggle draws, for the hover — the row has no space to say it and a
/// pilot should not have to turn one on to find out.
fn description(family: OverlayFamily, drawn: Drawn) -> &'static str {
    match (family, drawn) {
        (OverlayFamily::Harmonics, _) => {
            "One mark per motor per harmonic order — hue is the motor, and solid, dashed and \
             dotted are the fundamental and its two multiples. On the PSD, the frequencies each \
             motor spent the flight at; on the spectrogram, each motor's frequency over time; on \
             the throttle map, the frequency it ran at against the stick."
        }
        (OverlayFamily::DynNotch, Drawn::OnSpectrum) => {
            "Where the dynamic notch was allowed to work, along the plot floor, and — where the \
             log was flown in FFT_FREQ — what it actually took off, averaged over the centres its \
             tracker chose, with the time it spent at each drawn in the same lane."
        }
        (OverlayFamily::DynNotch, Drawn::OnMap) => {
            "The two ends of the range the notch was allowed, dashed — and where the log was \
             flown in FFT_FREQ, the centre its tracker actually chose, so a tracker sitting off \
             the noise band it is meant to be following is visible against that band."
        }
        (OverlayFamily::Notch(FilterLoop::Gyro), Drawn::OnSpectrum)
        | (OverlayFamily::Lowpass(FilterLoop::Gyro), Drawn::OnSpectrum) => GYRO_STAGE,
        (OverlayFamily::Notch(FilterLoop::Dterm), Drawn::OnSpectrum)
        | (OverlayFamily::Lowpass(FilterLoop::Dterm), Drawn::OnSpectrum) => DTERM_STAGE,
        (OverlayFamily::Notch(_), Drawn::OnMap) => {
            "Each notch at the frequency it nulls, across the whole map — a static stage moves \
             with neither the stick nor the clock."
        }
        (OverlayFamily::Lowpass(_), Drawn::OnMap) => {
            "Each lowpass at its corner. A stage the throttle drives is drawn as the curve it \
             really followed — Betaflight's own cutoff curve against the stick — which is the one \
             thing the spectrum can only ever show the average of."
        }
    }
}

/// What a chain's stages are drawn as. One text per loop rather than one per
/// family: what separates a gyro overlay from a D-term one is which signal it
/// acted on, and every stage of a chain is drawn the same way.
const GYRO_STAGE: &str = "Drawn on the data: the visible gyro stages cascaded into one total, hung off the raw \
     spectrum's own points, with the region between the two filled — that fill is the energy the \
     chain removed. Each stage keeps its own thin curve underneath.";
const DTERM_STAGE: &str = "This plot is gyro power, and the D-term stages never touched it — so they are drawn \
     against their own 0 dB line rather than against your spectrum, and take no part in the gyro \
     chain's total. A peak here is what the D-term filters had to survive.";

/// Detected noise peaks are not a filter — nothing configured them, the
/// analysis found them — so they are not an `OverlayFamily`. They get a toggle
/// beside the families because to a pilot they are one more thing drawn over
/// the curve, and user story 1 asks for a panel that opens with none of it.
const PEAKS_TITLE: &str = "Peaks";
const PEAKS_DESCRIPTION: &str = "The loudest noise peaks the analysis found, the three strongest labelled with what the \
     filters took off them.";
const NO_PEAKS: &str =
    "Nothing in this log rose far enough above the noise floor to be reported as a peak.";

/// Why the switch is greyed out. A control the log cannot fill says what the
/// log is missing rather than vanishing.
fn unavailable_reason(family: OverlayFamily) -> &'static str {
    match family {
        OverlayFamily::Harmonics => {
            "This log has no eRPM. Motor harmonics are computed from the RPM the ESCs report \
             back, which needs bidirectional DShot (`set dshot_bidir = ON`)."
        }
        // Its configured bounds are drawable from the header alone, so the
        // switch now greys out only when there is no notch at all. A log
        // without FFT_FREQ still gets the bounds, and the hover says what the
        // debug mode would add.
        OverlayFamily::DynNotch => "No dynamic notch was configured on this flight.",
        OverlayFamily::Notch(_) => "No static notch was enabled on this flight.",
        OverlayFamily::Lowpass(_) => "No lowpass stage was configured on this flight.",
    }
}

fn flag(family: OverlayFamily) -> usize {
    MENU_ORDER
        .iter()
        .position(|&f| f == family)
        .expect("every family has a menu entry")
}

/// One flag per overlay family.
///
/// A shared type with a separate instance per sub-tab. Shared so the menu and
/// the panels that read it are written once; separate instances because the
/// PSD and Frequency sub-tabs once shared a visibility field, and toggling one
/// silently toggled the other.
///
/// Every family is off by default, so the panel opens as a clean spectrum and
/// the pilot adds exactly the reference they want. There is no settings layer
/// in this application, so that default is per session by construction.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::app) struct OverlayVisibility {
    families: [bool; MENU_ORDER.len()],
    peaks: bool,
}

impl OverlayVisibility {
    pub(in crate::app) fn shows(&self, family: OverlayFamily) -> bool {
        self.families[flag(family)]
    }

    pub(in crate::app) fn shows_peaks(&self) -> bool {
        self.peaks
    }

    /// Everything on, for a panel test that has to draw every overlay a log
    /// can produce — nothing in the app turns them all on at once.
    #[cfg(test)]
    pub(in crate::app) fn all_on() -> Self {
        Self {
            families: [true; MENU_ORDER.len()],
            peaks: true,
        }
    }
}

/// The toggle row. A family the log cannot fill greys out with the reason,
/// rather than vanishing — the same law the tab bar is held to.
///
/// `families` is what the *panel* can draw, which is not every family: the
/// spectrogram draws harmonics and the notch tracker's centre and nothing
/// else, and a greyed toggle there would blame the log for a shape the panel
/// was never going to draw.
///
/// `available` is the panel's own answer too, rather than a walk over the
/// overlay list here: the spectrogram can only draw the dynamic notch's
/// *traced* centre, which needs a debug mode the mere presence of a dyn-notch
/// overlay says nothing about. A switch that ticks on and draws nothing is the
/// second rule this menu exists to remove.
///
/// Wrapped, so a narrow window costs a second line rather than clipping a
/// toggle the pilot then cannot find.
pub(in crate::app) fn show(
    ui: &mut Ui,
    visibility: &mut OverlayVisibility,
    families: &[OverlayFamily],
    drawn: Drawn,
    available: impl Fn(OverlayFamily) -> bool,
    has_peaks: Option<bool>,
) {
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Overlays").weak());

        // `None` is a panel that draws no peaks at all — not a log without
        // any. A greyed switch here would state a reason about the log that
        // is not the reason, so the switch is absent instead.
        if let Some(has_peaks) = has_peaks {
            toggle(
                ui,
                &mut visibility.peaks,
                PEAKS_TITLE,
                has_peaks,
                |on| match on {
                    true => PEAKS_DESCRIPTION.to_string(),
                    false => NO_PEAKS.to_string(),
                },
            );
        }

        for &family in families {
            let flag = flag(family);

            toggle(
                ui,
                &mut visibility.families[flag],
                &title(family),
                available(family),
                |on| match on {
                    true => description(family, drawn).to_string(),
                    false => unavailable_reason(family).to_string(),
                },
            );
        }
    });
}

/// One overlay's toggle: filled while it is drawing, outlined while it is not,
/// so the row reads as a set of states rather than a set of buttons.
fn toggle(
    ui: &mut Ui,
    on: &mut bool,
    label: &str,
    available: bool,
    hover: impl Fn(bool) -> String,
) {
    let button = match *on {
        true => elegance::Button::new(label),
        false => elegance::Button::new(label).outline(),
    };
    let response = ui.add_enabled(available, button);

    if response.clicked() {
        *on = !*on;
    }
    match available {
        true => response.on_hover_text(hover(true)),
        // The disabled variant, not `on_hover_text`: egui gates that one on the
        // widget being enabled, so a greyed toggle would state no reason at all.
        false => response.on_disabled_hover_text(hover(false)),
    };
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::analysis::FilterLoop;

    /// The panel opens as a clean spectrum. Every line on it is one the pilot
    /// asked for.
    #[test]
    fn every_family_starts_hidden() {
        let visibility = OverlayVisibility::default();

        assert!(MENU_ORDER.iter().all(|&f| !visibility.shows(f)));
        assert!(!visibility.shows_peaks());
    }

    /// Families are indexed by position in `MENU_ORDER`, so no two share a
    /// flag — the defect that a shared field caused between the PSD and
    /// Frequency sub-tabs, one level down.
    #[test]
    fn each_family_has_its_own_flag() {
        let mut visibility = OverlayVisibility::default();
        visibility.families[flag(OverlayFamily::DynNotch)] = true;

        assert!(visibility.shows(OverlayFamily::DynNotch));
        assert!(!visibility.shows(OverlayFamily::Harmonics));
        assert!(!visibility.shows_peaks());
    }

    /// Peaks are not a filter family, and toggling them must not move one.
    #[test]
    fn peaks_have_their_own_switch() {
        let visibility = OverlayVisibility {
            peaks: true,
            ..Default::default()
        };

        assert!(visibility.shows_peaks());
        assert!(MENU_ORDER.iter().all(|&f| !visibility.shows(f)));
    }

    /// Seven toggles share one row, so none of them may be a sentence. The
    /// detail lives in the hover, which every one of them has.
    #[test]
    fn every_toggle_is_short_enough_to_share_a_row_and_says_more_on_hover() {
        for family in MENU_ORDER {
            let title = title(family);
            assert!(title.len() <= 14, "{title:?} is too long for the row");
            // Both panels' hovers: a family listed on a map has to say what a
            // map draws, not what the PSD draws.
            for drawn in [Drawn::OnSpectrum, Drawn::OnMap] {
                assert!(
                    !description(family, drawn).is_empty(),
                    "{title:?} explains nothing on {drawn:?}"
                );
            }
            assert!(!unavailable_reason(family).is_empty());
        }

        assert!(PEAKS_TITLE.len() <= 14);
        assert!(!PEAKS_DESCRIPTION.is_empty());
    }

    /// Both loops of a family are separately toggleable — a pilot looking at
    /// the filters feeding one loop should not have the other's drawn too.
    #[test]
    fn the_two_loops_of_a_family_do_not_share_a_flag() {
        let mut visibility = OverlayVisibility::default();
        visibility.families[flag(OverlayFamily::Notch(FilterLoop::Gyro))] = true;

        assert!(visibility.shows(OverlayFamily::Notch(FilterLoop::Gyro)));
        assert!(!visibility.shows(OverlayFamily::Notch(FilterLoop::Dterm)));
    }
}
