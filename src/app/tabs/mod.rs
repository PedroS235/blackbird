mod filter_analysis;
mod pid_analysis;
mod timeseries;

use egui::{Color32, Ui};

use crate::analysis::Analysis;
use crate::parser::{FlightData, Metadata, ParsedLog, PerAxis};

use filter_analysis::FilterAnalysis;
use pid_analysis::PidAnalysis;
use timeseries::Timeseries;

pub(super) const GYRO_AXIS_COLORS: PerAxis<Color32> =
    PerAxis([Color32::RED, Color32::GREEN, Color32::BLUE]);
pub(super) const GYRO_RAW_COLOR: Color32 = Color32::GRAY;

/// What every tab is handed. Raw data only — a predicate derived from it stays
/// with whoever reads it, so this stays the one place a new kind of shared data
/// is added.
pub(super) struct TabCtx<'a> {
    pub(super) flight: &'a FlightData,
    pub(super) analysis: &'a Analysis,
    pub(super) metadata: &'a Metadata,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum MainTab {
    #[default]
    Timeseries,
    FilterAnalysis,
    PidAnalysis,
}

/// The whole main view: which tab is open at each level, and every tab's own
/// widget state.
#[derive(Default)]
pub(super) struct Tabs {
    selected: MainTab,
    timeseries: Timeseries,
    filter_analysis: FilterAnalysis,
    pid_analysis: PidAnalysis,
}

impl Tabs {
    /// Resolves the selected log once so that nothing below here sees an
    /// `Option`, and no tab re-implements the empty case.
    pub(super) fn show(&mut self, ui: &mut Ui, flight: Option<(&ParsedLog, &Analysis)>) {
        egui::CentralPanel::default().show(ui, |ui| {
            tab_bar(
                ui,
                &mut self.selected,
                &[
                    (MainTab::Timeseries, "Timeseries", true),
                    (MainTab::FilterAnalysis, "Filter Analysis", true),
                    (MainTab::PidAnalysis, "PID Analysis", true),
                ],
            );
            ui.separator();

            let Some((parsed, analysis)) = flight else {
                ui.label("No log selected");
                return;
            };
            let ctx = TabCtx {
                flight: &parsed.flight_data,
                analysis,
                metadata: &parsed.metadata,
            };

            match self.selected {
                MainTab::Timeseries => self.timeseries.show(ui, &ctx),
                MainTab::FilterAnalysis => self.filter_analysis.show(ui, &ctx),
                MainTab::PidAnalysis => self.pid_analysis.show(ui, &ctx),
            }
        });
    }
}

/// Every tab bar in the app, at every level. `add_enabled` throughout rather
/// than `selectable_label`: a tab the log cannot fill has to grey out rather
/// than vanish, and which idiom a new tab bar gets should not depend on which
/// file it was copied from.
fn tab_bar<T: Copy + PartialEq>(ui: &mut Ui, selected: &mut T, tabs: &[(T, &str, bool)]) {
    ui.horizontal(|ui| {
        for &(tab, label, enabled) in tabs {
            let button = egui::Button::selectable(*selected == tab, label);
            if ui.add_enabled(enabled, button).clicked() {
                *selected = tab;
            }
        }
    });
}

/// Stacked per-axis plots each get a third of the panel, less the axis label
/// above them, and never collapse to nothing on a short window.
fn stacked_plot_height(ui: &Ui, rows: usize) -> f32 {
    (ui.available_height() / rows.max(1) as f32 - 24.0).max(80.0)
}
