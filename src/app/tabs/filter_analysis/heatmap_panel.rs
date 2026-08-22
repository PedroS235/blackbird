use egui::{RichText, Ui};
use elegance::Slider;

use crate::app::tabs::stacked_plot_height;
use crate::app::ui::heatmap::{Heatmap, HeatmapOrientation, OverlayMark, OverlaySeries};
use crate::parser::Axis;
use crate::signal::fft::BinnedSpectrum;

/// One axis' worth of heatmap. Gathered before the panel draws, so the row
/// count is what actually renders and not a hopeful three.
pub(super) struct HeatmapRow<'a> {
    pub(super) axis: Axis,
    pub(super) spectrum: &'a BinnedSpectrum,
    /// Logged channels, decimated per frame against the visible window.
    pub(super) overlays: Vec<OverlaySeries<'a>>,
    /// Filter geometry, in this map's own axes.
    pub(super) marks: Vec<OverlayMark>,
}

/// Which heatmap this is. The two differ only in orientation, wording and plot
/// id — everything else, the sensitivity floor above all, is shared, so that
/// changing the slider's range is one edit rather than two files that never
/// reference each other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum HeatmapKind {
    VsThrottle,
    Spectrogram,
}

impl HeatmapKind {
    fn orientation(self) -> HeatmapOrientation {
        match self {
            Self::VsThrottle => HeatmapOrientation::VsThrottle,
            Self::Spectrogram => HeatmapOrientation::VsTime,
        }
    }

    /// Plot ids are egui's persistence keys — a pilot's zoom lives on them, so
    /// they stay exactly what each panel used before they were merged.
    fn plot_id(self, axis: Axis) -> String {
        match self {
            Self::VsThrottle => format!("throttle_heatmap_{}", axis.name()),
            Self::Spectrogram => format!("spectrogram_{}", axis.name()),
        }
    }

    fn heading(self, axis: Axis) -> String {
        match self {
            Self::VsThrottle => format!("{} vs throttle", axis.name()),
            Self::Spectrogram => format!("{} spectrogram", axis.name()),
        }
    }

    /// A log can carry the raw gyro and still have nothing to bin it against.
    /// Neither case is a broken log, and neither may go blank.
    fn nothing_to_bin(self) -> &'static str {
        match self {
            Self::VsThrottle => {
                "No throttle in this log — this map bins the noise by stick position, so without \
                 rcCommand[3] there is nothing to bin against. Enable the RC Commands field in \
                 Betaflight's Blackbox tab and fly again."
            }
            Self::Spectrogram => {
                "No time reference in this log — the spectrogram bins the noise over the flight, \
                 and this log carries no timestamps to bin against."
            }
        }
    }
}

pub(super) struct HeatmapPanel {
    floor_db: f32,
}

impl Default for HeatmapPanel {
    fn default() -> Self {
        Self { floor_db: -60.0 }
    }
}

impl HeatmapPanel {
    pub(super) fn show(&mut self, ui: &mut Ui, kind: HeatmapKind, rows: Vec<HeatmapRow<'_>>) {
        // Before the slider: a sensitivity control over dead space is the same
        // blank panel, only with something to fiddle with.
        if rows.is_empty() {
            ui.label(kind.nothing_to_bin());
            return;
        }

        ui.add(
            Slider::new(&mut self.floor_db, -120.0..=-5.0)
                .label("sensitivity (noise floor dB)")
                .suffix("dB"),
        );
        ui.add_space(4.0);

        // Measured after the slider, which has already taken its height.
        let height = stacked_plot_height(ui, rows.len());

        for row in rows {
            ui.label(RichText::new(kind.heading(row.axis)).strong());
            Heatmap {
                id: kind.plot_id(row.axis),
                orientation: kind.orientation(),
                spectrum: row.spectrum,
                height,
                floor_db: self.floor_db as f64,
                overlays: row.overlays,
                marks: row.marks,
            }
            .show(ui);
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    /// Renaming a plot id silently throws away the persisted zoom of every
    /// pilot who had one.
    #[test]
    fn plot_ids_are_stable() {
        assert_eq!(
            HeatmapKind::VsThrottle.plot_id(Axis::Roll),
            "throttle_heatmap_roll"
        );
        assert_eq!(
            HeatmapKind::Spectrogram.plot_id(Axis::Yaw),
            "spectrogram_yaw"
        );
    }
}
