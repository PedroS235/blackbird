mod gyro_vs_setpoint;

use egui::Ui;

use super::TabCtx;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum PidAnalysisTab {
    #[default]
    GyroVsSetpoint,
    StepResponse,
}

#[derive(Default)]
pub(super) struct PidAnalysis {
    selected: PidAnalysisTab,
}

impl PidAnalysis {
    pub(super) fn show(&mut self, ui: &mut Ui, ctx: &TabCtx<'_>) {
        ui.horizontal(|ui| {
            for (tab, label, enabled) in [
                (PidAnalysisTab::GyroVsSetpoint, "Gyro Vs Setpoint", true),
                (PidAnalysisTab::StepResponse, "Step Response", false),
            ] {
                let selectable = egui::Button::selectable(self.selected == tab, label);
                if ui.add_enabled(enabled, selectable).clicked() {
                    self.selected = tab;
                }
            }
        });
        ui.add_space(4.0);

        match self.selected {
            PidAnalysisTab::GyroVsSetpoint => gyro_vs_setpoint::show(ui, ctx.flight),
            PidAnalysisTab::StepResponse => {
                ui.label("Step Response - coming soon");
            }
        }
    }
}
