//! The startup update check, and the one-line strip that offers what it found.

use std::sync::mpsc;

use crate::version::{self, UpdateInfo};

use super::BlackbirdApp;

/// The checking thread's end of the wire, and what it found. Dismissal is not a
/// field: dismissing drops the `found`, so the offer is gone for this session
/// and back on the next launch. Nothing is persisted, so no settings store is a
/// prerequisite for this feature.
#[derive(Default)]
pub(crate) struct UpdateCheck {
    rx: Option<mpsc::Receiver<Option<UpdateInfo>>>,
    found: Option<UpdateInfo>,
}

impl UpdateCheck {
    /// Spawns the check. The `ctx` clone is what wakes the UI when it lands: an
    /// idle window repaints on nothing, so without the request the strip would
    /// appear whenever the pilot next happened to move the mouse.
    pub(super) fn spawn(ctx: &egui::Context) -> Self {
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();

        std::thread::spawn(move || {
            let _ = tx.send(version::check_for_update());
            ctx.request_repaint();
        });

        Self {
            rx: Some(rx),
            found: None,
        }
    }
}

impl BlackbirdApp {
    pub(super) fn show_update_strip(&mut self, ui: &mut egui::Ui) {
        self.poll_update();

        // Taken rather than borrowed: the strip needs `&mut self` for the
        // dismiss, and not putting it back *is* the dismiss.
        let Some(info) = self.update.found.take() else {
            return;
        };
        let mut keep = true;

        egui::Panel::top("update_strip").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(format!(
                    "{} Blackbird {} is available — you are running {}",
                    egui_phosphor::regular::ARROW_CIRCLE_UP,
                    info.latest,
                    info.current
                ));

                if ui
                    .add(
                        elegance::Button::new(format!(
                            "{} Download",
                            egui_phosphor::regular::DOWNLOAD_SIMPLE
                        ))
                        .size(elegance::ButtonSize::Small),
                    )
                    .clicked()
                {
                    ui.ctx()
                        .open_url(egui::OpenUrl::new_tab(&info.download_url));
                }

                if ui
                    .add(
                        elegance::Button::new("Release notes")
                            .size(elegance::ButtonSize::Small)
                            .outline(),
                    )
                    .clicked()
                {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(&info.release_url));
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(
                            elegance::Button::new(egui_phosphor::regular::X)
                                .size(elegance::ButtonSize::Small)
                                .outline(),
                        )
                        .on_hover_text("Dismiss until the next launch")
                        .clicked()
                    {
                        keep = false;
                    }
                });
            });
        });

        if keep {
            self.update.found = Some(info);
        }
    }

    /// One `try_recv` per frame until the thread answers, then the receiver is
    /// dropped and this costs nothing for the rest of the session.
    fn poll_update(&mut self) {
        let Some(rx) = &self.update.rx else {
            return;
        };

        match rx.try_recv() {
            Ok(found) => {
                if found.is_none() {
                    tracing::debug!("no newer release than this build");
                }
                self.update.found = found;
                self.update.rx = None;
            }
            Err(mpsc::TryRecvError::Empty) => {}
            // The thread died without answering. Nothing to offer and nothing
            // to retry — the check is once per launch by design.
            Err(mpsc::TryRecvError::Disconnected) => {
                tracing::debug!("the update check thread ended without an answer");
                self.update.rx = None;
            }
        }
    }
}
