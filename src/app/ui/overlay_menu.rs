//! Which reference overlays the pilot has asked to see, and the dropdown that
//! asks.
//!
//! A dropdown rather than an inline row: the vertical space above the plots is
//! divided between three stacked axes, and a closed menu costs none of it.

use egui::{RichText, Ui};
use elegance::Switch;

use crate::analysis::{FilterOverlay, OverlayFamily};

/// Menu order is `OverlayFamily::ALL`'s order, and a family's position in it
/// is that family's visibility flag — so a family the analysis can produce
/// always has a switch, and no two families can share one.
const MENU_ORDER: [OverlayFamily; OverlayFamily::ALL.len()] = OverlayFamily::ALL;

/// The heading the switch sits under. The gyro and D-term switches of one
/// filter share theirs.
fn section_of(family: OverlayFamily) -> &'static str {
    match family {
        OverlayFamily::Harmonics => "Harmonics",
        OverlayFamily::DynNotch => "Dyn notch",
        OverlayFamily::Notch(_) => "Static notches",
        OverlayFamily::Lowpass(_) => "LPFs",
    }
}

/// The switch's label. Under a section heading that already names the filter,
/// what is left to say is which loop it feeds.
fn title(family: OverlayFamily) -> String {
    match family {
        OverlayFamily::Harmonics => "Show motor harmonics".to_string(),
        OverlayFamily::DynNotch => "Show configured range and traced centre".to_string(),
        OverlayFamily::Notch(l) | OverlayFamily::Lowpass(l) => l.name().to_string(),
    }
}

/// Detected noise peaks are not a filter — nothing configured them, the
/// analysis found them — so they are not an `OverlayFamily`. They get a switch
/// beside the families because to a pilot they are one more thing drawn over
/// the curve, and user story 1 asks for a panel that opens with none of it.
const PEAKS_TITLE: &str = "Show detected peaks";
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

    fn enabled_count(&self) -> usize {
        self.families.iter().filter(|&&on| on).count() + usize::from(self.peaks)
    }
}

/// The dropdown. A family the log cannot fill greys out with the reason,
/// rather than vanishing — the same law the tab bar is held to.
pub(in crate::app) fn show(
    ui: &mut Ui,
    visibility: &mut OverlayVisibility,
    overlays: &[FilterOverlay],
    has_peaks: bool,
) {
    let label = match visibility.enabled_count() {
        0 => "Overlays".to_string(),
        n => format!("Overlays ({n})"),
    };

    ui.menu_button(label, |ui| {
        ui.label(RichText::new("Peaks").strong());
        let peaks = ui.add_enabled(has_peaks, Switch::new(&mut visibility.peaks, PEAKS_TITLE));
        if !has_peaks {
            peaks.on_disabled_hover_text(NO_PEAKS);
        }

        let mut section = None;
        for family in MENU_ORDER {
            // One heading per family, so the gyro and D-term switches read as
            // two views of one filter rather than four unrelated toggles.
            if section != Some(section_of(family)) {
                section = Some(section_of(family));
                ui.label(RichText::new(section_of(family)).strong());
            }

            let available = overlays.iter().any(|o| o.family == family);
            let entry = ui.add_enabled(
                available,
                Switch::new(&mut visibility.families[flag(family)], title(family)),
            );

            if !available {
                // The disabled variant, not `on_hover_text`: egui gates that
                // one on the widget being enabled, so a greyed switch would
                // state no reason at all.
                entry.on_disabled_hover_text(unavailable_reason(family));
            }
        }
    });
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
        assert_eq!(visibility.enabled_count(), 0);
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
        assert_eq!(visibility.enabled_count(), 1);
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
        assert_eq!(visibility.enabled_count(), 1);
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
