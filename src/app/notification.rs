use super::BlackbirdApp;

pub(crate) enum Level {
    Info,
    Warning,
    Error,
}

pub(crate) struct Notification {
    pub(crate) message: String,
    pub(crate) level: Level,
}

impl BlackbirdApp {
    pub(super) fn show_notifications(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("notifications_center").show(ui, |ui| {
            if let Some(n) = self.notifications.back() {
                let color = match n.level {
                    Level::Error => egui::Color32::RED,
                    Level::Warning => egui::Color32::YELLOW,
                    Level::Info => egui::Color32::WHITE,
                };
                ui.colored_label(color, &n.message);
            }
        });
    }
}
