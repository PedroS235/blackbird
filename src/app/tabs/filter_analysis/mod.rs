mod filter_marks;
mod heatmap_panel;
mod psd;
mod spectrogram;
mod vs_reference;

use egui::Ui;

use super::{TabCtx, tab_bar};
use crate::analysis::SpectralAnalysis;
use crate::app::colors;
use crate::app::ui::overlay_menu::{self, OverlayVisibility};
use crate::parser::Axis;
use heatmap_panel::{HeatmapKind, HeatmapPanel};
use psd::Psd;

/// Here the raw gyro is the whole point — there is no falling back to the
/// filtered trace, since the comparison between the two is what this tab is.
const NO_RAW_GYRO: &str = "No gyroUnfilt in this log. Filter analysis works from the gyro \
                           before filtering — with only the filtered trace there is nothing to \
                           compare, and no way to see what the filters removed. Betaflight \
                           records it in debug mode GYRO_SCALED (`set debug_mode = \
                           GYRO_SCALED`, or the Debug mode dropdown in the configurator's \
                           Blackbox tab); fly again with it on.";

/// How many axes a spectral panel is about to stack. Axes the log never
/// carried are skipped, so a single-axis log gets one full-height plot rather
/// than a third of the panel above two thirds of nothing.
fn drawn_axes(analysis: &SpectralAnalysis) -> usize {
    Axis::ALL
        .iter()
        .filter(|&&axis| analysis.axis(axis).is_some())
        .count()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum FilterAnalysisTab {
    #[default]
    Psd,
    VsReference,
    Spectrogram,
}

#[derive(Default)]
pub(super) struct FilterAnalysis {
    selected: FilterAnalysisTab,
    psd: Psd,
    vs_reference: HeatmapPanel,
    spectrogram: HeatmapPanel,
    /// One instance per sub-tab, as every sub-tab with a menu gets: the PSD's
    /// switches must not move when a map's are toggled, and the two maps draw
    /// the same families against different axes.
    vs_reference_overlays: OverlayVisibility,
    spectrogram_overlays: OverlayVisibility,
}

impl FilterAnalysis {
    pub(super) fn show(&mut self, ui: &mut Ui, ctx: &TabCtx<'_>) {
        // Built before the bar, because whether the two heatmaps have anything
        // to draw is what greys their tabs out. Rows are borrowed slices — the
        // cost is three `Option` checks each.
        let mut throttle_rows = vs_reference::rows(&ctx.analysis.spectral);
        let mut time_rows = spectrogram::rows(ctx);

        tab_bar(
            ui,
            &mut self.selected,
            &[
                (FilterAnalysisTab::Psd, "Power Spectral Density", true),
                (
                    FilterAnalysisTab::VsReference,
                    "Vs Reference",
                    !throttle_rows.is_empty(),
                ),
                (
                    FilterAnalysisTab::Spectrogram,
                    "Spectrogram",
                    !time_rows.is_empty(),
                ),
            ],
        );
        ui.add_space(4.0);

        // Every sub-tab here reads the pre-filter gyro, so they all go blank
        // together and for one reason. Said once, above the lot of them.
        if drawn_axes(&ctx.analysis.spectral) == 0 {
            ui.label(NO_RAW_GYRO);
            return;
        }

        match self.selected {
            FilterAnalysisTab::Psd => self.psd.show(ui, &ctx.analysis.spectral),
            FilterAnalysisTab::VsReference => {
                // The menu first, then the marks it asked for — the same order
                // the spectrogram uses, and for the same reason: a toggle has
                // to take effect on the frame it is clicked.
                overlay_menu::show(
                    ui,
                    &mut self.vs_reference_overlays,
                    &vs_reference::FAMILIES,
                    overlay_menu::Drawn::OnMap,
                    |family| vs_reference::available(ctx, family),
                    None,
                );
                ui.add_space(4.0);
                vs_reference::attach_marks(
                    &mut throttle_rows,
                    ctx,
                    self.vs_reference_overlays,
                    &colors::palette(ui.ctx()),
                );
                self.vs_reference
                    .show(ui, HeatmapKind::VsThrottle, throttle_rows)
            }
            FilterAnalysisTab::Spectrogram => {
                // The menu first, then the curves it asked for: the panel this
                // one shares with Vs Reference knows nothing about overlays,
                // and drawing the row here rather than inside it keeps a
                // toggle effective on the frame it is clicked.
                overlay_menu::show(
                    ui,
                    &mut self.spectrogram_overlays,
                    &spectrogram::FAMILIES,
                    overlay_menu::Drawn::OnMap,
                    |family| spectrogram::available(ctx, family),
                    None,
                );
                ui.add_space(4.0);
                spectrogram::attach_overlays(
                    &mut time_rows,
                    ctx,
                    self.spectrogram_overlays,
                    &colors::palette(ui.ctx()),
                );
                self.spectrogram
                    .show(ui, HeatmapKind::Spectrogram, time_rows)
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::analysis::{Analysis, GyroNoiseAnalyzer, OverlayFamily};
    use crate::app::log_store::{FlightCatalog, FlightKey, FlightRef, LogId};
    use crate::app::ui::overlay_menu::OverlayVisibility;

    /// A tab drawn on its own flight and nothing else. The maps here draw one
    /// flight only; comparison is the step response panel's concept.
    struct NoOtherFlights;

    impl FlightCatalog for NoOtherFlights {
        fn flights(&self) -> Vec<FlightKey> {
            Vec::new()
        }
        fn selected(&self) -> Option<FlightKey> {
            None
        }
        fn resolve(&self, _key: FlightKey) -> Option<FlightRef<'_>> {
            None
        }
        fn label(&self, _key: FlightKey) -> Option<String> {
            None
        }
    }

    /// Both maps, drawn headlessly with every family on: the levels, the
    /// throttle-driven curves, the harmonic curves and the tracker's own
    /// centre. A mark placed on the wrong axis or a curve built from mismatched
    /// slices is a panic here rather than a shape a pilot has to disbelieve.
    #[test]
    fn every_family_draws_on_both_maps() {
        let (fd, metadata) = test_flight::synthetic();
        let analysis = Analysis {
            spectral: GyroNoiseAnalyzer::default().analyze(&fd, &metadata),
            ..Default::default()
        };
        let ctx = TabCtx {
            flight: &fd,
            analysis: &analysis,
            metadata: &metadata,
            base: (LogId::new(0), 0),
            catalog: &NoOtherFlights,
        };

        for tab in [
            FilterAnalysisTab::VsReference,
            FilterAnalysisTab::Spectrogram,
        ] {
            let mut panel = FilterAnalysis {
                selected: tab,
                vs_reference_overlays: OverlayVisibility::all_on(),
                spectrogram_overlays: OverlayVisibility::all_on(),
                ..Default::default()
            };
            let egui_ctx = egui::Context::default();

            // Twice, as the PSD's own render test does: a heatmap's marks are
            // clipped against bounds the first frame has not established yet.
            for _ in 0..2 {
                let _ = egui_ctx.run_ui(egui::RawInput::default(), |egui_ctx| {
                    egui::CentralPanel::default().show(egui_ctx, |ui| panel.show(ui, &ctx));
                });
            }
        }
    }

    /// What the two maps actually put on the plot, rather than only that they
    /// drew without panicking: a stage the throttle drove is a curve on both,
    /// a static stage is one frequency across the map, and the harmonics and
    /// the tracker's own centre come off the log.
    #[test]
    fn both_maps_draw_curves_for_what_moved_and_levels_for_what_did_not() {
        use crate::app::ui::heatmap::Mark;

        let (fd, metadata) = test_flight::synthetic();
        let analysis = Analysis {
            spectral: GyroNoiseAnalyzer::default().analyze(&fd, &metadata),
            ..Default::default()
        };
        let ctx = TabCtx {
            flight: &fd,
            analysis: &analysis,
            metadata: &metadata,
            base: (LogId::new(0), 0),
            catalog: &NoOtherFlights,
        };
        let palette = elegance::Palette::charcoal();
        let all_on = OverlayVisibility::all_on();

        let mut throttle_rows = vs_reference::rows(&analysis.spectral);
        vs_reference::attach_marks(&mut throttle_rows, &ctx, all_on, &palette);
        let marks = &throttle_rows.first().expect("a throttle map").marks;

        let curves = marks
            .iter()
            .filter(|m| matches!(m.mark, Mark::Curve(_)))
            .count();
        let levels = marks
            .iter()
            .filter(|m| matches!(m.mark, Mark::Level(_)))
            .count();
        // The dynamic gyro LPF, one curve per motor per order, and the notch
        // tracker's own centre against the stick.
        assert!(curves >= 3, "{curves} curves, {levels} levels");
        assert!(levels >= 2, "{curves} curves, {levels} levels");

        let mut time_rows = spectrogram::rows(&ctx);
        spectrogram::attach_overlays(&mut time_rows, &ctx, all_on, &palette);
        let row = time_rows.first().expect("a spectrogram");
        // Against time the harmonics and the trace stay logged channels, and
        // the header's geometry is marks — the dynamic cutoff among them,
        // because the stick at each moment is in the log.
        assert!(!row.overlays.is_empty());
        assert!(
            row.marks.iter().any(|m| matches!(m.mark, Mark::Curve(_))),
            "no driven curve against time"
        );
        assert!(row.marks.iter().any(|m| matches!(m.mark, Mark::Level(_))));
    }

    /// The two maps toggle independently, as every sub-tab with a menu does —
    /// the defect this rule exists for was two panels sharing one flag.
    #[test]
    fn the_two_maps_keep_their_own_switches() {
        let mut panel = FilterAnalysis {
            vs_reference_overlays: OverlayVisibility::all_on(),
            ..Default::default()
        };

        assert!(panel.vs_reference_overlays.shows(OverlayFamily::Harmonics));
        assert!(!panel.spectrogram_overlays.shows(OverlayFamily::Harmonics));
        panel.spectrogram_overlays = OverlayVisibility::all_on();
        assert!(panel.spectrogram_overlays.shows(OverlayFamily::Harmonics));
    }
}

/// A synthetic flight the panels can be drawn over. Shared by the sub-tab
/// tests: every one of them needs a log that fills every overlay family, and
/// three copies of that would drift.
#[cfg(test)]
pub(super) mod test_flight {
    use crate::parser::Axis;

    /// A synthetic flight with everything the overlays are built from: two
    /// noisy axes pre- and post-filter, throttle for the dynamic lowpass,
    /// eRPM for the harmonics, and a tracked notch centre in `debug[0]`.
    pub(super) fn synthetic() -> (crate::parser::FlightData, crate::parser::Metadata) {
        use crate::parser::Channel;
        use crate::parser::metadata::{
            DynNotchConfig, FilterConfig, FilterType, LowpassConfig, NotchConfig,
            StaticLowpassConfig,
        };

        const N: usize = 4096;
        let at = |i: usize| i as f64 / 4000.0;
        let noise = |i: usize| {
            let t = at(i);
            (t * 220.0 * std::f64::consts::TAU).sin() * 40.0
                + (t * 640.0 * std::f64::consts::TAU).sin() * 12.0
        };

        let mut fd = crate::parser::FlightData::default()
            .with_time((0..N as u64).map(|i| i * 250).collect())
            .with_channel(
                Channel::Throttle,
                (0..N).map(|i| 1000.0 + at(i) * 800.0).collect(),
            )
            .with_channel(Channel::Rpm(0), vec![4200.0; N])
            .with_channel(
                Channel::Debug(0),
                (0..N).map(|i| 200.0 + at(i) * 90.0).collect(),
            );
        for axis in Axis::ALL {
            fd = fd
                .with_channel(Channel::RawGyro(axis), (0..N).map(noise).collect())
                .with_channel(
                    Channel::Gyro(axis),
                    (0..N).map(|i| noise(i) * 0.5).collect(),
                )
                .with_channel(
                    Channel::Debug(axis.index()),
                    (0..N).map(|i| 200.0 + at(i) * 90.0).collect(),
                );
        }

        let metadata = crate::parser::Metadata {
            looptime_us: Some(125),
            debug_mode: "FFT_FREQ".to_string(),
            filters: FilterConfig {
                gyro_lpf1: Some(LowpassConfig {
                    static_hz: 0.0,
                    dyn_min_hz: 250.0,
                    dyn_max_hz: 500.0,
                    dyn_expo: 0.0,
                    filter_type: FilterType::Pt1,
                }),
                gyro_lpf2: Some(StaticLowpassConfig {
                    cutoff_hz: 500.0,
                    filter_type: FilterType::Pt1,
                }),
                gyro_notches: vec![NotchConfig {
                    center_hz: 300.0,
                    cutoff_hz: 280.0,
                }],
                dterm_lpf1: Some(LowpassConfig {
                    static_hz: 100.0,
                    dyn_min_hz: 0.0,
                    dyn_max_hz: 0.0,
                    dyn_expo: 0.0,
                    filter_type: FilterType::Pt2,
                }),
                dterm_notches: vec![NotchConfig {
                    center_hz: 0.0,
                    cutoff_hz: 0.0,
                }],
                dyn_notch: Some(DynNotchConfig {
                    min_hz: 100.0,
                    max_hz: 500.0,
                    count: 2,
                    q: 5.0,
                }),
                ..Default::default()
            },
            ..Default::default()
        };
        (fd, metadata)
    }
}
