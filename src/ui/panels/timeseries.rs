use egui::Color32;
use egui_plot::{Line, LineStyle, PlotBounds, PlotPoints};

use crate::app::PlotState;
use crate::parser::FlightData;

const MAX_DISPLAY_PTS: usize = 10_000;
const SMOOTH_WINDOW: usize = 5;

const ROLL: Color32 = Color32::from_rgb(220, 80, 80);
const PITCH: Color32 = Color32::from_rgb(80, 200, 80);
const YAW: Color32 = Color32::from_rgb(80, 140, 220);
const MOTOR_COLORS: [Color32; 4] = [
    Color32::from_rgb(255, 180, 50),
    Color32::from_rgb(50, 220, 220),
    Color32::from_rgb(200, 100, 220),
    Color32::from_rgb(150, 220, 100),
];

pub struct TimeseriesPanel {
    pub show_gyro_unfilt: bool,
    pub show_gyro_adc: bool,
    pub show_setpoint: bool,
    pub show_rc_command: bool,
    pub show_motors: bool,
    pub show_roll: bool,
    pub show_pitch: bool,
    pub show_yaw: bool,
    pub smooth: bool,
}

impl Default for TimeseriesPanel {
    fn default() -> Self {
        Self {
            show_gyro_unfilt: true,
            show_gyro_adc: true,
            show_setpoint: true,
            show_rc_command: true,
            show_motors: true,
            show_roll: true,
            show_pitch: true,
            show_yaw: true,
            smooth: false,
        }
    }
}

impl TimeseriesPanel {
    pub fn show(&mut self, ui: &mut egui::Ui, data: &FlightData, plot_state: &mut PlotState) {
        self.show_controls(ui);
        ui.separator();
        self.show_plots(ui, data, plot_state);
    }

    fn show_controls(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Gyro:");
            ui.checkbox(&mut self.show_gyro_unfilt, "pre-filter");
            ui.checkbox(&mut self.show_gyro_adc, "post-filter");
            ui.separator();
            ui.label("RC:");
            ui.checkbox(&mut self.show_setpoint, "setpoint");
            ui.checkbox(&mut self.show_rc_command, "command");
            ui.separator();
            ui.checkbox(&mut self.show_motors, "motors");
            ui.separator();
            ui.label("Axes:");
            ui.checkbox(&mut self.show_roll, "roll");
            ui.checkbox(&mut self.show_pitch, "pitch");
            ui.checkbox(&mut self.show_yaw, "yaw");
            ui.separator();
            ui.checkbox(&mut self.smooth, "smooth");
        });
    }

    fn show_plots(&self, ui: &mut egui::Ui, data: &FlightData, plot_state: &mut PlotState) {
        let t0 = data.time_us.first().copied().unwrap_or(0);
        let link_id = ui.id().with(("ts_link", t0));

        let has_gyro = self.show_gyro_unfilt || self.show_gyro_adc;
        let has_rc = self.show_setpoint || self.show_rc_command;
        let has_motors = self.show_motors && !data.motor.is_empty();
        let plot_count = [has_gyro, has_rc, has_motors]
            .iter()
            .filter(|&&v| v)
            .count()
            .max(1);
        let plot_h = (ui.available_height() / plot_count as f32 - 8.0).max(80.0);

        if has_gyro {
            let r = self.make_plot(("gyro", t0), plot_h, link_id).show(ui, |p| {
                let (vmin, vmax) = view_range(p.plot_bounds(), &data.time_us, t0);
                // Re-fit Y to visible data every frame; user controls X only
                p.set_auto_bounds(egui::Vec2b::new(false, true));
                let cursor_x = p.pointer_coordinate().map(|c| c.x);
                self.add_gyro_lines(p, data, t0, vmin, vmax);
                cursor_x
            });
            if let Some(x) = r.inner {
                plot_state.cursor_time = Some(x);
            }
        }

        if has_rc {
            let r = self.make_plot(("rc", t0), plot_h, link_id).show(ui, |p| {
                let (vmin, vmax) = view_range(p.plot_bounds(), &data.time_us, t0);
                p.set_auto_bounds(egui::Vec2b::new(false, true));
                let cursor_x = p.pointer_coordinate().map(|c| c.x);
                self.add_rc_lines(p, data, t0, vmin, vmax);
                cursor_x
            });
            if let Some(x) = r.inner {
                plot_state.cursor_time = Some(x);
            }
        }

        if has_motors {
            let r = self.make_plot(("motors", t0), plot_h, link_id).show(ui, |p| {
                let (vmin, vmax) = view_range(p.plot_bounds(), &data.time_us, t0);
                p.set_auto_bounds(egui::Vec2b::new(false, true));
                let cursor_x = p.pointer_coordinate().map(|c| c.x);
                self.add_motor_lines(p, data, t0, vmin, vmax);
                cursor_x
            });
            if let Some(x) = r.inner {
                plot_state.cursor_time = Some(x);
            }
        }
    }

    fn make_plot(&self, id: impl std::hash::Hash, height: f32, link_id: egui::Id) -> egui_plot::Plot<'_> {
        egui_plot::Plot::new(id)
            .height(height)
            .link_axis(link_id, egui::Vec2b::new(true, false))
            .link_cursor(link_id, egui::Vec2b::new(true, false))
            // Only allow dragging on X; Y is auto-fitted to visible data
            .allow_drag(egui::Vec2b::new(true, false))
            .x_axis_label("time (s)")
            .legend(egui_plot::Legend::default())
    }

    fn add_gyro_lines(
        &self,
        p: &mut egui_plot::PlotUi<'_>,
        data: &FlightData,
        t0: u64,
        vmin: f64,
        vmax: f64,
    ) {
        let axes = [
            (self.show_roll,  "roll",  ROLL,  &data.gyro_unfilt_roll,  &data.gyro_adc_roll),
            (self.show_pitch, "pitch", PITCH, &data.gyro_unfilt_pitch, &data.gyro_adc_pitch),
            (self.show_yaw,   "yaw",   YAW,   &data.gyro_unfilt_yaw,   &data.gyro_adc_yaw),
        ];
        for (show, name, color, unfilt, adc) in axes {
            if !show { continue; }
            if self.show_gyro_unfilt {
                if let Some(v) = unfilt {
                    p.line(
                        self.make_line(format!("{name} pre"), &data.time_us, v, t0, vmin, vmax, color)
                            .style(LineStyle::dashed_dense()),
                    );
                }
            }
            if self.show_gyro_adc {
                if let Some(v) = adc {
                    p.line(self.make_line(format!("{name} post"), &data.time_us, v, t0, vmin, vmax, color));
                }
            }
        }
    }

    fn add_rc_lines(
        &self,
        p: &mut egui_plot::PlotUi<'_>,
        data: &FlightData,
        t0: u64,
        vmin: f64,
        vmax: f64,
    ) {
        let axes = [
            (self.show_roll,  "roll",  ROLL,  &data.setpoint_roll,  &data.rc_command_roll),
            (self.show_pitch, "pitch", PITCH, &data.setpoint_pitch, &data.rc_command_pitch),
            (self.show_yaw,   "yaw",   YAW,   &data.setpoint_yaw,   &data.rc_command_yaw),
        ];
        for (show, name, color, setpt, cmd) in axes {
            if !show { continue; }
            if self.show_setpoint {
                if let Some(v) = setpt {
                    p.line(self.make_line(format!("{name} setpoint"), &data.time_us, v, t0, vmin, vmax, color));
                }
            }
            if self.show_rc_command {
                if let Some(v) = cmd {
                    p.line(
                        self.make_line(format!("{name} cmd"), &data.time_us, v, t0, vmin, vmax, color)
                            .style(LineStyle::dashed_dense()),
                    );
                }
            }
        }
    }

    fn add_motor_lines(
        &self,
        p: &mut egui_plot::PlotUi<'_>,
        data: &FlightData,
        t0: u64,
        vmin: f64,
        vmax: f64,
    ) {
        for (i, vals) in data.motor.iter().enumerate() {
            let color = MOTOR_COLORS.get(i).copied().unwrap_or(Color32::WHITE);
            p.line(self.make_line(format!("M{}", i + 1), &data.time_us, vals, t0, vmin, vmax, color));
        }
    }

    fn make_line(
        &self,
        name: impl Into<String>,
        time_us: &[u64],
        vals: &[f32],
        t0: u64,
        vmin: f64,
        vmax: f64,
        color: Color32,
    ) -> Line<'static> {
        let pts = view_points(time_us, vals, t0, vmin, vmax, self.smooth);
        Line::new(name, pts).color(color)
    }
}

/// Returns the visible time range in normalized seconds (t - t0).
/// Falls back to full data extent on first frame when bounds are degenerate.
fn view_range(bounds: PlotBounds, time_us: &[u64], t0: u64) -> (f64, f64) {
    let min = bounds.min()[0];
    let max = bounds.max()[0];
    if max > min {
        return (min, max);
    }
    let full_max = time_us
        .last()
        .map_or(1.0, |&t| (t.saturating_sub(t0)) as f64 / 1_000_000.0);
    (0.0, full_max)
}

/// Cull to viewport (+ 50% margin), downsample, normalize timestamps, optionally smooth.
fn view_points(
    time_us: &[u64],
    vals: &[f32],
    t0: u64,
    vmin: f64,
    vmax: f64,
    smooth: bool,
) -> PlotPoints<'static> {
    let span = (vmax - vmin).max(0.001);
    let margin_us = (span * 0.5 * 1_000_000.0) as u64;

    // Convert normalized view bounds back to raw µs for binary search
    let lo = t0.saturating_add((vmin * 1_000_000.0) as u64).saturating_sub(margin_us);
    let hi = t0.saturating_add((vmax * 1_000_000.0) as u64).saturating_add(margin_us);

    let start = time_us.partition_point(|&t| t < lo);
    let end = time_us.partition_point(|&t| t <= hi);

    let mut pts = minmax_downsample(&time_us[start..end], &vals[start..end], t0, MAX_DISPLAY_PTS);

    if smooth {
        moving_average(&mut pts, SMOOTH_WINDOW);
    }

    PlotPoints::new(pts)
}

/// Min-max downsample. Bins into `max_pts/2` buckets, emits (min, max) per bucket
/// in time order. Preserves peaks and troughs. Outputs normalized x in seconds.
fn minmax_downsample(time_us: &[u64], vals: &[f32], t0: u64, max_pts: usize) -> Vec<[f64; 2]> {
    let n = time_us.len();
    if n == 0 {
        return Vec::new();
    }

    let norm = |t: u64| (t.saturating_sub(t0)) as f64 / 1_000_000.0;

    if n <= max_pts {
        return time_us
            .iter()
            .zip(vals.iter())
            .map(|(&t, &v)| [norm(t), v as f64])
            .collect();
    }

    let bucket_count = (max_pts / 2).max(1);
    let bucket_size = n.div_ceil(bucket_count);
    let mut out = Vec::with_capacity(bucket_count * 2);
    let mut i = 0;

    while i < n {
        let end = (i + bucket_size).min(n);

        let mut min_v = vals[i];
        let mut min_t = time_us[i];
        let mut max_v = vals[i];
        let mut max_t = time_us[i];

        for j in (i + 1)..end {
            let v = vals[j];
            let t = time_us[j];
            if v < min_v { min_v = v; min_t = t; }
            if v > max_v { max_v = v; max_t = t; }
        }

        if min_t <= max_t {
            out.push([norm(min_t), min_v as f64]);
            out.push([norm(max_t), max_v as f64]);
        } else {
            out.push([norm(max_t), max_v as f64]);
            out.push([norm(min_t), min_v as f64]);
        }

        i = end;
    }

    out
}

/// Centered moving average over the y-values. Smooths the display line
/// without affecting x positions, using a symmetric window clamped at edges.
fn moving_average(pts: &mut Vec<[f64; 2]>, window: usize) {
    if window <= 1 || pts.len() < window {
        return;
    }
    let n = pts.len();
    let half = window / 2;
    let orig: Vec<f64> = pts.iter().map(|p| p[1]).collect();

    for i in 0..n {
        let start = i.saturating_sub(half);
        let end = (i + half + 1).min(n);
        let sum: f64 = orig[start..end].iter().sum();
        pts[i][1] = sum / (end - start) as f64;
    }
}
