use egui::Vec2;
use elegance::{Button, Segment, SegmentedControl, SegmentedSize};

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
                        if ui.style().visuals.dark_mode {
                            ui.add(
                                egui::Image::new(egui::include_image!(
                                    "../../assets/blackbird_banner.png"
                                ))
                                .fit_to_exact_size(Vec2::new(300.0, 100.0)),
                            );
                        } else {
                            ui.add(
                                egui::Image::new(egui::include_image!(
                                    "../../assets/blackbird_banner_light.png"
                                ))
                                .fit_to_exact_size(Vec2::new(300.0, 100.0)),
                            );
                        }
                    });

                    ui.add_space(8.0);
                    egui::containers::Sides::new().shrink_left().show(
                        ui,
                        |_ui| {},
                        |ui| {
                            let mut selected = match self.theme_preference {
                                egui::ThemePreference::Dark => 0,
                                egui::ThemePreference::Light => 1,
                                egui::ThemePreference::System => 2,
                            };
                            ui.add(
                                SegmentedControl::from_segments(
                                    &mut selected,
                                    [
                                        Segment::icon(egui_phosphor::regular::MOON)
                                            .hover_text("Dark theme"),
                                        Segment::icon(egui_phosphor::regular::SUN)
                                            .hover_text("Light theme"),
                                        Segment::icon(egui_phosphor::regular::DESKTOP)
                                            .hover_text("Follow system theme"),
                                    ],
                                )
                                .size(SegmentedSize::Small),
                            );
                            self.theme_preference = match selected {
                                0 => egui::ThemePreference::Dark,
                                1 => egui::ThemePreference::Light,
                                _ => egui::ThemePreference::System,
                            };
                        },
                    );

                    ui.add_space(8.0);
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
