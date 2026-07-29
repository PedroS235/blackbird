use egui::{Color32, FontId, Layout, RichText, vec2};

use super::{BlackbirdApp, ui};

impl BlackbirdApp {
    pub(super) fn show_sidepanel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("sidepanel")
            .resizable(false)
            .exact_size(300.0)
            .show_inside(ui, |ui| {
                ui.vertical(|ui| {
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::Image::new(egui::include_image!(
                                "../../assets/blackbird-icon.png"
                            ))
                            .max_size(vec2(36.0, 36.0)),
                        );
                        ui.add_space(8.0);
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new(self.app_name)
                                    .font(FontId::proportional(22.0))
                                    .color(Color32::ORANGE),
                            );
                            ui.label(
                                RichText::new("Blackbox Analyzer")
                                    .font(FontId::proportional(11.0))
                                    .color(Color32::GRAY),
                            );
                        });
                    });

                    ui.add_space(12.0);
                    ui.separator();
                    ui.add_space(8.0);

                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Logs").strong());
                        ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("+ Add Log(s)").clicked() {
                                self.open_logs();
                            }
                        });
                    });

                    ui.add_space(8.0);
                    egui::ScrollArea::vertical()
                        .id_salt("log_list")
                        .show(ui, |ui| {
                            for loaded in &mut self.logs {
                                let sublog_count = loaded.log.len();
                                let idx = loaded.active_sublog.min(sublog_count.saturating_sub(1));
                                let metadata = &loaded.log[idx].metadata;
                                ui::log_card::show(
                                    ui,
                                    metadata,
                                    sublog_count,
                                    &mut loaded.selected,
                                    &mut loaded.active_sublog,
                                );
                                ui.add_space(4.0);
                            }
                        });
                });
            });
    }
}
