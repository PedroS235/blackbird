use egui::{Color32, FontId, Layout, RichText, Vec2, vec2};
use elegance::Button;

use super::{BlackbirdApp, ui};

impl BlackbirdApp {
    pub(super) fn show_sidepanel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("sidepanel")
            .resizable(false)
            .exact_size(300.0)
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::Image::new(egui::include_image!(
                                "../../assets/blackbird_banner.png"
                            ))
                            .fit_to_exact_size(Vec2::new(300.0, 100.0)),
                        );
                    });

                    ui.add_space(12.0);
                    ui.separator();
                    ui.add_space(8.0);

                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Logs").strong());
                        ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add(Button::new("+ Add Log(s)")).clicked() {
                                self.open_logs();
                            }
                        });
                    });

                    ui.add_space(8.0);
                    egui::ScrollArea::vertical()
                        .id_salt("log_list")
                        .show(ui, |ui| {
                            let mut clicked = None;
                            for (i, loaded, is_selected) in self.logs.iter_mut() {
                                let sublog_count = loaded.log.len();
                                let idx = loaded.active_sublog.min(sublog_count.saturating_sub(1));
                                let metadata = &loaded.log[idx].metadata;
                                if ui::log_card::show(
                                    ui,
                                    metadata,
                                    sublog_count,
                                    is_selected,
                                    &mut loaded.active_sublog,
                                ) {
                                    clicked = Some(i);
                                }
                                ui.add_space(4.0);
                            }
                            if let Some(i) = clicked {
                                self.logs.select(i);
                            }
                        });
                });
            });
    }
}
