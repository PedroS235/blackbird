//! The "ask AI" button and whatever it produced below it. One function so
//! Step Response and PSD render an identical widget instead of two
//! near-copies drifting apart. `ai::Feedback` stays UI-free; this is where it
//! meets egui.

use egui::{Color32, Ui};
use egui_phosphor::regular::OPEN_AI_LOGO;

use crate::ai::Feedback;

/// `build_message` runs only on click, so a panel that has never been asked
/// pays nothing for formatting its metrics.
pub fn show(ui: &mut Ui, feedback: &mut Feedback, build_message: impl FnOnce() -> String) {
    feedback.poll();

    let clicked = ui
        .add(
            elegance::Button::new(OPEN_AI_LOGO)
                .enabled(!feedback.is_loading())
                .loading(feedback.is_loading()),
        )
        .on_hover_text("Ask AI for tuning feedback")
        .clicked();

    if clicked {
        feedback.ask(build_message());
    }

    match feedback {
        Feedback::Done(Ok(text)) => {
            ui.add_space(4.0);
            elegance::Card::new()
                .heading("AI feedback")
                .show(ui, |ui| ui.label(text.as_str()));
        }
        Feedback::Done(Err(e)) => {
            ui.add_space(4.0);
            ui.colored_label(Color32::from_rgb(220, 80, 90), e);
        }
        Feedback::Idle | Feedback::Loading(_) => {}
    }
}
