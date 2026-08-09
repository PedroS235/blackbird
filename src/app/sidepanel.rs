use egui::{Color32, FontId, Layout, RichText, Vec2, vec2};
use elegance::Button;

use super::{BlackbirdApp, ui};

impl BlackbirdApp {
    pub(super) fn show_sidepanel(&mut self, ui: &mut egui::Ui) {
        // `exact_size` pins min == max, so the panel is forced back to this
        // value every frame — unlike `min_size` alone, whose width is read
        // from a persisted `PanelState` that only ever grows to fit whatever
        // overflowed last frame and never shrinks back once the wide card
        // (long name, many sub-logs) is gone.
        let width = (ui.ctx().content_rect().width() * 0.3).clamp(250.0, 420.0);

        egui::Panel::left("sidepanel")
            .resizable(false)
            .exact_size(width)
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

                    if ui.add(Button::new("+ Add Log(s)").full_width()).clicked() {
                        self.open_logs();
                    }

                    ui.add_space(8.0);
                    egui::ScrollArea::vertical()
                        .id_salt("log_list")
                        .show(ui, |ui| {
                            let mut clicked = None;
                            let mut closed = None;
                            for (i, loaded, is_selected) in self.logs.iter_mut() {
                                let sublog_count = loaded.log.len();
                                let idx = loaded.active_sublog.min(sublog_count.saturating_sub(1));
                                let metadata = &loaded.log[idx].metadata;
                                let (log_clicked, log_closed) = ui::log_card::show(
                                    ui,
                                    metadata,
                                    sublog_count,
                                    is_selected,
                                    &mut loaded.active_sublog,
                                );

                                if log_clicked {
                                    clicked = Some(i);
                                }

                                if log_closed {
                                    closed = Some(i);
                                }
                                ui.add_space(4.0);
                            }
                            if let Some(i) = closed {
                                self.logs.remove(i);
                            } else if let Some(i) = clicked {
                                self.logs.select(i);
                            }
                        });
                });
            });
    }
}
