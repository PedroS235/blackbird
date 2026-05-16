use egui::Color32;
use egui_plot::{HLine, Line, LineStyle, PlotPoints};

use crate::analysis::step_response::compute_step_response;
use crate::analysis::{sample_rate_from_timestamps, AnalysisResult, StepResponseResult};
use crate::parser::FlightData;

const ROLL: Color32 = Color32::from_rgb(220, 80, 80);
const PITCH: Color32 = Color32::from_rgb(80, 200, 80);
const YAW: Color32 = Color32::from_rgb(80, 140, 220);
const AXIS_NAMES: [&str; 3] = ["Roll", "Pitch", "Yaw"];
const AXIS_COLORS: [Color32; 3] = [ROLL, PITCH, YAW];

pub struct StepResponsePanel {
    overlay: bool,
    selected_axis: usize,
    throttle_min_pct: f32,
    throttle_max_pct: f32,
    cached: Option<[StepResponseResult; 3]>,
}

impl Default for StepResponsePanel {
    fn default() -> Self {
        Self {
            overlay: false,
            selected_axis: 0,
            throttle_min_pct: 0.0,
            throttle_max_pct: 100.0,
            cached: None,
        }
    }
}

impl StepResponsePanel {
    pub fn invalidate_cache(&mut self) {
        self.cached = None;
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        data: &FlightData,
        sample_rate_hz: f32,
        base_analysis: &AnalysisResult,
    ) {
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.overlay, "Overlay axes");
            if !self.overlay {
                ui.separator();
                for (i, name) in AXIS_NAMES.iter().enumerate() {
                    ui.selectable_value(&mut self.selected_axis, i, *name);
                }
            }
            ui.separator();
            ui.label("Throttle:");
            let c1 = ui
                .add(
                    egui::Slider::new(&mut self.throttle_min_pct, 0.0f32..=100.0f32)
                        .suffix("%")
                        .text("min")
                        .integer(),
                )
                .changed();
            let c2 = ui
                .add(
                    egui::Slider::new(&mut self.throttle_max_pct, 0.0f32..=100.0f32)
                        .suffix("%")
                        .text("max")
                        .integer(),
                )
                .changed();
            if c1 || c2 {
                self.cached = None;
            }
        });
        ui.separator();

        let overlay = self.overlay;
        let selected_axis = self.selected_axis;

        let results = self.get_results(data, sample_rate_hz, base_analysis);
        let any_steps = results.iter().any(|r| r.step_count > 0);

        if !any_steps {
            ui.centered_and_justified(|ui| {
                ui.label(
                    "No steps detected — fly with stick inputs to generate step response data",
                );
            });
            return;
        }

        if overlay {
            show_plot(ui, results, &[0, 1, 2]);
            ui.separator();
            show_stats_row(ui, results, None);
        } else {
            let idx = selected_axis;
            if results[idx].step_count == 0 {
                ui.centered_and_justified(|ui| {
                    ui.label(format!("No steps detected on {} axis", AXIS_NAMES[idx]));
                });
            } else {
                show_plot(ui, results, &[idx]);
                ui.separator();
                show_stats_row(ui, results, Some(idx));
            }
        }
    }

    fn get_results<'a>(
        &'a mut self,
        data: &FlightData,
        nominal_hz: f32,
        base_analysis: &'a AnalysisResult,
    ) -> &'a [StepResponseResult; 3] {
        let throttle_min = self.throttle_min_pct / 100.0;
        let throttle_max = self.throttle_max_pct / 100.0;
        let full_range = throttle_min == 0.0 && throttle_max == 1.0;

        if full_range && self.cached.is_none() {
            return &base_analysis.step_response;
        }

        if self.cached.is_none() {
            let hz = sample_rate_from_timestamps(&data.time_us).unwrap_or(nominal_hz);
            let throttle = data.setpoint_throttle.as_deref();
            self.cached = Some(std::array::from_fn(|i| {
                let (cmd, resp) = match i {
                    0 => (data.setpoint_roll.as_deref(), data.gyro_adc_roll.as_deref()),
                    1 => (data.setpoint_pitch.as_deref(), data.gyro_adc_pitch.as_deref()),
                    _ => (data.setpoint_yaw.as_deref(), data.gyro_adc_yaw.as_deref()),
                };
                match (cmd, resp) {
                    (Some(c), Some(r)) => {
                        compute_step_response(c, r, throttle, throttle_min, throttle_max, hz)
                    }
                    _ => StepResponseResult::default(),
                }
            }));
        }

        self.cached.as_ref().unwrap()
    }
}

fn curve_pts(r: &StepResponseResult, curve: &[f32]) -> Vec<[f64; 2]> {
    curve
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            let t_ms = (i as f32 - r.pre_samples as f32) / r.sample_rate_hz * 1000.0;
            [t_ms as f64, v as f64]
        })
        .collect()
}

fn show_plot(ui: &mut egui::Ui, results: &[StepResponseResult; 3], axes: &[usize]) {
    let plot_h = (ui.available_height() - 80.0).max(120.0);

    egui_plot::Plot::new("step_response")
        .height(plot_h)
        .x_axis_label("time (ms)")
        .y_axis_label("normalised response")
        .legend(egui_plot::Legend::default())
        .include_y(0.0)
        .include_y(1.3)
        .show(ui, |p| {
            p.hline(HLine::new("reference", 1.0).color(Color32::from_gray(160)).width(1.0));
            p.hline(HLine::new("settle +5%", 1.05).color(Color32::from_gray(80)).width(1.0));
            p.hline(HLine::new("settle -5%", 0.95).color(Color32::from_gray(80)).width(1.0));

            for &i in axes {
                let r = &results[i];
                if r.step_count == 0 || r.curve.is_empty() {
                    continue;
                }
                let color = AXIS_COLORS[i];
                let dim = Color32::from_rgba_premultiplied(color.r(), color.g(), color.b(), 130);

                p.line(
                    Line::new(AXIS_NAMES[i], PlotPoints::new(curve_pts(r, &r.curve)))
                        .color(color)
                        .width(2.0),
                );
                if !r.positive_curve.is_empty() {
                    p.line(
                        Line::new(
                            format!("{} +", AXIS_NAMES[i]),
                            PlotPoints::new(curve_pts(r, &r.positive_curve)),
                        )
                        .color(dim)
                        .width(1.0)
                        .style(LineStyle::Dashed { length: 8.0 }),
                    );
                }
                if !r.negative_curve.is_empty() {
                    p.line(
                        Line::new(
                            format!("{} -", AXIS_NAMES[i]),
                            PlotPoints::new(curve_pts(r, &r.negative_curve)),
                        )
                        .color(dim)
                        .width(1.0)
                        .style(LineStyle::Dotted { spacing: 5.0 }),
                    );
                }
            }
        });
}

fn show_stats_row(ui: &mut egui::Ui, results: &[StepResponseResult; 3], single: Option<usize>) {
    ui.horizontal(|ui| {
        let indices: &[usize] = match single {
            Some(i) => &[i],
            None => &[0, 1, 2],
        };
        for &i in indices {
            let r = &results[i];
            if r.step_count == 0 {
                continue;
            }
            ui.colored_label(AXIS_COLORS[i], AXIS_NAMES[i]);
            ui.label(format!(
                "  {}steps (+{}/-{})",
                r.step_count, r.positive_count, r.negative_count
            ));
            ui.separator();
            ui.label(format!("{:.1}% overshoot", r.overshoot_pct));
            ui.separator();
            if r.rise_time_ms.is_finite() {
                ui.label(format!("rise {:.0}ms", r.rise_time_ms));
                ui.separator();
            }
            if r.settling_time_ms.is_finite() {
                ui.label(format!("settle {:.0}ms", r.settling_time_ms));
            } else {
                ui.label("no settle");
            }
            ui.separator();
        }
    });
}
