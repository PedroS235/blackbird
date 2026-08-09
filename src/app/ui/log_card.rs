use crate::parser::Metadata;
use egui::RichText;
use elegance::{Button, Card, Theme};

pub fn show(
    ui: &mut egui::Ui,
    metadata: &Metadata,
    sublog_count: usize,
    is_selected: bool,
    active_sublog: &mut usize,
) -> (bool, bool) {
    let looptime = metadata.looptime_us.unwrap_or(0) as f32;
    let hz = if looptime > 0.0 { 1e6 / looptime } else { 0.0 };
    let duration_s = metadata.duration.as_secs_f32();
    let mut clicked = false;
    let mut close = false;

    Card::new().show(ui, |ui| {
        ui.set_max_width(ui.available_width());

        // Title row: chart icon, file name left, radio right. Styled to
        // match Card's own heading() by hand, since that helper renders
        // straight to a Label with no Response to hang a hover off.
        //
        // `Sides` reserves the radio's width first (`shrink_left`), then
        // hands the label whatever's left — a plain `ui.horizontal` would
        // measure the label against the row's full width before the radio
        // claims its slot, so a long name would overlap it instead of
        // truncating short.
        egui::containers::Sides::new()
            .shrink_left()
            .truncate()
            .show(
                ui,
                |ui| {
                    let theme = Theme::current(ui.ctx());
                    let heading = |text: String| {
                        RichText::new(text)
                            .color(theme.palette.text_muted)
                            .size(theme.typography.heading)
                            .strong()
                    };
                    let icon = format!("{} ", egui_phosphor::regular::DATABASE);
                    ui.label(heading(format!("{icon}{}", metadata.file_name)));
                },
                |ui| {
                    if ui.radio(is_selected, "").clicked() {
                        clicked = true;
                    }
                },
            );

        ui.add_space(8.0);
        ui.label(RichText::new(&metadata.craft_name));
        ui.label(RichText::new(format!("{} | {}", metadata.firmware, metadata.board)).small());
        ui.label(RichText::new(format!("{:.0} Hz  •  {:.1} s", hz, duration_s)).small());
        if let Some(rates) = &metadata.rates {
            ui.label(RichText::new(rates.to_string()).small())
                .on_hover_text("The craft's rate curve: type, then roll/pitch/yaw rates.");
        }

        // Sub-log selector — only shown when file has multiple logs
        if sublog_count > 1 {
            ui.add_space(6.0);
            ui.separator();
            ui.add_space(4.0);
            ui.label(RichText::new("Sub-log:").small());
            // Wrap onto as many rows as the panel's actual width needs,
            // rather than a fixed chunk size — with the panel width now
            // pinned (see sidepanel.rs), a rigid row would just get clipped
            // instead of pushing the panel wider.
            ui.horizontal_wrapped(|ui| {
                for i in 0..sublog_count {
                    let button = if *active_sublog == i {
                        Button::new((i + 1).to_string())
                    } else {
                        Button::new((i + 1).to_string()).outline()
                    };

                    if ui.add(button).clicked() {
                        *active_sublog = i;
                    }
                }
            });
        }

        if ui
            .add(
                elegance::Button::new(format!("{} Remove", egui_phosphor::regular::X_CIRCLE))
                    .accent(elegance::Accent::Red)
                    .full_width(),
            )
            .clicked()
        {
            close = true;
        };
    });

    (clicked, close)
}
