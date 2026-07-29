use crate::parser::Metadata;
use egui::{Frame, Layout, RichText, Stroke, vec2};

const MAX_FILENAME_LEN: usize = 30;

pub fn show(
    ui: &mut egui::Ui,
    metadata: &Metadata,
    sublog_count: usize,
    selected: &mut bool,
    active_sublog: &mut usize,
) {
    let looptime = metadata.looptime_us.unwrap_or(0) as f32;
    let hz = if looptime > 0.0 { 1e6 / looptime } else { 0.0 };
    let duration_s = metadata.duration.as_secs_f32();

    let style = ui.style();
    let bg = style.visuals.widgets.noninteractive.bg_fill;
    let stroke = Stroke::new(1.0, style.visuals.widgets.noninteractive.bg_stroke.color);

    Frame::new()
        .fill(bg)
        .stroke(stroke)
        .corner_radius(6.0)
        .inner_margin(vec2(8.0, 6.0))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());

            // Title row: file name left, checkbox right
            ui.horizontal(|ui| {
                // Cut the file name if it's too big
                if metadata.file_name.len() > MAX_FILENAME_LEN {
                    ui.label(RichText::new(format!("{}...", &metadata.file_name[..30])).strong())
                        .on_hover_text(&metadata.file_name);
                } else {
                    ui.label(RichText::new(&metadata.file_name).strong());
                }
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.checkbox(selected, "");
                });
            });

            ui.add_space(4.0);
            ui.label(RichText::new(&metadata.craft_name));
            ui.label(RichText::new(format!("{} | {}", metadata.firmware, metadata.board)).small());
            ui.label(RichText::new(format!("{:.0} Hz  •  {:.1} s", hz, duration_s)).small());

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
}
