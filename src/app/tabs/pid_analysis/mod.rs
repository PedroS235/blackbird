mod gyro_vs_setpoint;
mod step_response;

use egui::Ui;

use super::TabCtx;
use step_response::StepResponse;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum PidAnalysisTab {
    #[default]
    GyroVsSetpoint,
    StepResponse,
}

#[derive(Default)]
pub(super) struct PidAnalysis {
    selected: PidAnalysisTab,
    step_response: StepResponse,
}

impl PidAnalysis {
    pub(super) fn show(&mut self, ui: &mut Ui, ctx: &TabCtx<'_>) {
        ui.horizontal(|ui| {
            for (tab, label) in [
                (PidAnalysisTab::GyroVsSetpoint, "Gyro Vs Setpoint"),
                (PidAnalysisTab::StepResponse, "Step Response"),
            ] {
                if ui.selectable_label(self.selected == tab, label).clicked() {
                    self.selected = tab;
                }
            }
        });
        ui.add_space(4.0);

        match self.selected {
            PidAnalysisTab::GyroVsSetpoint => gyro_vs_setpoint::show(ui, ctx.flight),
            PidAnalysisTab::StepResponse => self.step_response.show(ui, ctx),
        }
    }
}
