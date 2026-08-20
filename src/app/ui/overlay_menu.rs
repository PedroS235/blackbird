//! Which reference overlays the pilot has asked to see, and the dropdown that
//! asks.
//!
//! A dropdown rather than an inline row: the vertical space above the plots is
//! divided between three stacked axes, and a closed menu costs none of it.

use egui::Ui;
use elegance::Switch;

use crate::analysis::{FilterOverlay, OverlayFamily};

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
    families: [bool; OverlayFamily::ALL.len()],
}

impl OverlayVisibility {
    pub(in crate::app) fn shows(&self, family: OverlayFamily) -> bool {
        self.families[family.index()]
    }

    fn any(&self) -> usize {
        self.families.iter().filter(|&&on| on).count()
    }
}

/// The dropdown. A family the log cannot fill greys out with the reason,
/// rather than vanishing — the same law the tab bar is held to.
pub(in crate::app) fn show(
    ui: &mut Ui,
    visibility: &mut OverlayVisibility,
    overlays: &[FilterOverlay],
) {
    let label = match visibility.any() {
        0 => "Overlays".to_string(),
        n => format!("Overlays ({n})"),
    };

    ui.menu_button(label, |ui| {
        for family in OverlayFamily::ALL {
            let available = overlays.iter().any(|o| o.family == family);
            let entry = ui.add_enabled(
                available,
                Switch::new(&mut visibility.families[family.index()], family.title()),
            );

            if !available {
                entry.on_hover_text(family.unavailable_reason());
            }
        }
    });
}

#[cfg(test)]
mod test {
    use super::*;

    /// The panel opens as a clean spectrum. Every line on it is one the pilot
    /// asked for.
    #[test]
    fn every_family_starts_hidden() {
        let visibility = OverlayVisibility::default();

        assert!(OverlayFamily::ALL.iter().all(|&f| !visibility.shows(f)));
        assert_eq!(visibility.any(), 0);
    }

    /// Families are indexed by position in `ALL`, so no two share a flag.
    #[test]
    fn each_family_has_its_own_flag() {
        let mut visibility = OverlayVisibility::default();
        visibility.families[OverlayFamily::DynNotch.index()] = true;

        assert!(visibility.shows(OverlayFamily::DynNotch));
        assert!(!visibility.shows(OverlayFamily::Harmonics));
        assert_eq!(visibility.any(), 1);
    }
}
