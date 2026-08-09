use crate::parser::Metadata;
use egui::{Frame, Layout, RichText, Stroke, vec2};
use elegance::Card;

const MAX_FILENAME_LEN: usize = 30;

pub fn show(
    ui: &mut egui::Ui,
    metadata: &Metadata,
    sublog_count: usize,
    is_selected: bool,
    active_sublog: &mut usize,
) -> bool {
    let looptime = metadata.looptime_us.unwrap_or(0) as f32;
    let hz = if looptime > 0.0 { 1e6 / looptime } else { 0.0 };
    let duration_s = metadata.duration.as_secs_f32();
    let mut clicked = false;

    Card::new().show(ui, |ui| {
        ui.set_min_width(ui.available_width());

        // Title row: chart icon, file name left, radio right
        ui.horizontal(|ui| {
            ui.label(RichText::new(egui_phosphor::regular::DATABASE));
            // Cut the file name if it's too big — by characters, since a
            // byte index into an accented or CJK name can land mid-glyph
            // and panic on a repaint the pilot cannot get back from.
            match metadata.file_name.char_indices().nth(MAX_FILENAME_LEN) {
                Some((cut, _)) => ui
                    .label(RichText::new(format!("{}...", &metadata.file_name[..cut])).strong())
                    .on_hover_text(&metadata.file_name),
                None => ui.label(RichText::new(&metadata.file_name).strong()),
            };
            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.radio(is_selected, "").clicked() {
                    clicked = true;
                }
            });
        });

        ui.add_space(4.0);
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
                for chunk in (0..sublog_count).collect::<Vec<_>>().chunks(8) {
                    ui.horizontal(|ui| {
                        for &i in chunk {
                            if ui
                                .selectable_label(
                                    *active_sublog == i,
                                    RichText::new((i + 1).to_string()).small(),
                                )
                                .clicked()
                            {
                                *active_sublog = i;
                            }
                        }
                    });
                }
            }
        });

    clicked
}
