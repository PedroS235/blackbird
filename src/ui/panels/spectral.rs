use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use egui_plot::{Line, PlotPoint, PlotPoints, Polygon, Text};

use crate::{
    analysis::{spectral::N_THROTTLE_BINS, AnalysisResult},
    parser::HeaderData,
    ui::panels::timeseries::moving_average,
};

const MAX_FREQ_HZ: f32 = 1600.0;
const N_HARMONICS: u32 = 3;
const MAX_FUNDAMENTALS: usize = 4;

const HARMONIC_COLORS: [Color32; MAX_FUNDAMENTALS] = [
    Color32::from_rgb(255, 160, 40),
    Color32::from_rgb(50, 200, 255),
    Color32::from_rgb(100, 220, 80),
    Color32::from_rgb(220, 80, 220),
];

#[derive(Default, PartialEq)]
enum SpectralView {
    #[default]
    Spectrum,
    Heatmap,
}

pub struct SpectralPanel {
    selected_axis: usize,
    view: SpectralView,
    show_harmonics: bool,
    show_filtered: bool,
    /// Cached heatmap texture + the key (axis, filtered) it was built for.
    heatmap_texture: Option<(egui::TextureHandle, (usize, bool))>,
}

impl Default for SpectralPanel {
    fn default() -> Self {
        Self {
            selected_axis: 0,
            view: SpectralView::default(),
            show_harmonics: true,
            show_filtered: false,
            heatmap_texture: None,
        }
    }
}

// ── helpers ──────────────────────────────────────────────────────────────────

fn heat_color(t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        let s = (t * 2.0 * 255.0) as u8;
        Color32::from_rgb(s, 0, 0)
    } else {
        let s = ((t - 0.5) * 2.0 * 255.0) as u8;
        Color32::from_rgb(255, s, s)
    }
}

fn build_heatmap_texture(
    throttle_map: &[Vec<f32>],
    max_bin: usize,
    ctx: &egui::Context,
) -> egui::TextureHandle {
    let height = N_THROTTLE_BINS;
    let width = max_bin;

    let (min_db, max_db) = throttle_map
        .iter()
        .flatten()
        .filter(|v| v.is_finite())
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(mn, mx), &v| {
            (mn.min(v), mx.max(v))
        });
    let range = (max_db - min_db).max(1e-6);

    // Row 0 = highest throttle bin (displayed at top).
    let pixels: Vec<Color32> = (0..height)
        .rev()
        .flat_map(|tbin| {
            let row = throttle_map.get(tbin);
            (0..width).map(move |fbin| {
                let db = row.and_then(|r| r.get(fbin)).copied().unwrap_or(f32::NAN);
                if !db.is_finite() {
                    Color32::from_rgb(10, 10, 10)
                } else {
                    heat_color((db - min_db) / range)
                }
            })
        })
        .collect();

    ctx.load_texture(
        "spectral_heatmap",
        egui::ColorImage {
            size: [width, height],
            pixels,
            source_size: egui::Vec2::new(width as f32, height as f32),
        },
        egui::TextureOptions::LINEAR,
    )
}

// ── panel ────────────────────────────────────────────────────────────────────

impl SpectralPanel {
    pub fn show(&mut self, ui: &mut egui::Ui, analysis: &AnalysisResult, header: &HeaderData) {
        ui.horizontal(|ui| {
            for (i, name) in ["Roll", "Pitch", "Yaw"].iter().enumerate() {
                ui.selectable_value(&mut self.selected_axis, i, *name);
            }
            ui.separator();
            ui.selectable_value(&mut self.view, SpectralView::Spectrum, "Spectrum");
            ui.selectable_value(&mut self.view, SpectralView::Heatmap, "Heatmap");
            ui.separator();
            ui.checkbox(&mut self.show_filtered, "Filtered gyro");
            if self.view == SpectralView::Spectrum && header.rpm_filter.is_some() {
                ui.checkbox(&mut self.show_harmonics, "RPM bands");
            }
        });
        ui.separator();

        let result = if self.show_filtered {
            &analysis.spectral_filtered[self.selected_axis]
        } else {
            &analysis.spectral[self.selected_axis]
        };

        if result.average_spectrum.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label("No gyro data for this axis");
            });
            return;
        }

        match self.view {
            SpectralView::Spectrum => self.show_spectrum(ui, analysis, header, result),
            SpectralView::Heatmap => {
                self.show_heatmap(ui, &result.throttle_map, result.freq_resolution_hz);
            }
        }
    }

    fn show_spectrum(
        &mut self,
        ui: &mut egui::Ui,
        analysis: &AnalysisResult,
        header: &HeaderData,
        result: &crate::analysis::SpectralResult,
    ) {
        let freq_res = result.freq_resolution_hz;
        let max_bin =
            ((MAX_FREQ_HZ / freq_res) as usize + 1).min(result.average_spectrum.len());

        let mut pts: Vec<[f64; 2]> = (0..max_bin)
            .filter(|&k| result.average_spectrum[k].is_finite())
            .map(|k| [k as f64 * freq_res as f64, result.average_spectrum[k] as f64])
            .collect();
        moving_average(&mut pts, 7);
        let points: PlotPoints = pts.into_iter().collect();

        // Detect motor fundamentals from raw peaks (cleaner signal for detection).
        let raw = &analysis.spectral[self.selected_axis];
        let tolerance = raw.freq_resolution_hz * 4.0;
        let mut fundamentals: Vec<f32> = Vec::new();
        'outer: for peak in &raw.peaks {
            if peak.freq_hz > MAX_FREQ_HZ {
                continue;
            }
            for &f0 in &fundamentals {
                for h in 2..=N_HARMONICS {
                    if (peak.freq_hz - f0 * h as f32).abs() < tolerance {
                        continue 'outer;
                    }
                }
            }
            fundamentals.push(peak.freq_hz);
            if fundamentals.len() >= MAX_FUNDAMENTALS {
                break;
            }
        }

        const ORDINALS: [&str; 3] = ["1st", "2nd", "3rd"];

        struct Band {
            lo: f64,
            center: f64,
            hi: f64,
            color: Color32,
            label: &'static str,
        }
        let mut bands: Vec<Band> = Vec::new();
        if self.show_harmonics {
            if let Some(cfg) = &header.rpm_filter {
                for (i, &f0) in fundamentals.iter().enumerate() {
                    let base = HARMONIC_COLORS[i];
                    for h in 1..=cfg.harmonics {
                        let center = f0 * h as f32;
                        if center > MAX_FREQ_HZ {
                            break;
                        }
                        let fade = if center < cfg.min_hz - cfg.fade_range_hz {
                            continue;
                        } else if center < cfg.min_hz {
                            (center - (cfg.min_hz - cfg.fade_range_hz)) / cfg.fade_range_hz
                        } else {
                            1.0f32
                        };
                        let weight =
                            cfg.weights.get(h as usize - 1).copied().unwrap_or(1.0);
                        let half_bw = center / (2.0 * cfg.q);
                        let alpha = (60.0 * weight * fade).clamp(0.0, 255.0) as u8;
                        bands.push(Band {
                            lo: (center - half_bw) as f64,
                            center: center as f64,
                            hi: (center + half_bw) as f64,
                            color: Color32::from_rgba_unmultiplied(
                                base.r(), base.g(), base.b(), alpha,
                            ),
                            label: ORDINALS.get(h as usize - 1).copied().unwrap_or(""),
                        });
                    }
                }
            }
        }

        let line_label = if self.show_filtered { "Filtered gyro" } else { "Raw gyro" };

        egui_plot::Plot::new("spectral_line")
            .x_axis_label("Frequency (Hz)")
            .y_axis_label("Magnitude (dB)")
            .legend(egui_plot::Legend::default())
            .show(ui, |p| {
                for (idx, band) in bands.iter().enumerate() {
                    let rect: Vec<[f64; 2]> = vec![
                        [band.lo, -120.0],
                        [band.lo,    5.0],
                        [band.hi,    5.0],
                        [band.hi, -120.0],
                    ];
                    p.polygon(
                        Polygon::new("", rect)
                            .fill_color(band.color)
                            .stroke(egui::Stroke::NONE)
                            .id(egui::Id::new(("rpm_notch", idx))),
                    );
                    if !band.label.is_empty() {
                        let text_color = Color32::from_rgba_unmultiplied(
                            band.color.r(), band.color.g(), band.color.b(), 220,
                        );
                        p.text(
                            Text::new("", PlotPoint::new(band.center, -1.0), band.label)
                                .color(text_color)
                                .anchor(Align2::CENTER_TOP)
                                .id(egui::Id::new(("rpm_label", idx))),
                        );
                    }
                }
                p.line(Line::new(line_label, points).width(1.5));
            });
    }

    fn show_heatmap(
        &mut self,
        ui: &mut egui::Ui,
        throttle_map: &[Vec<f32>],
        freq_res: f32,
    ) {
        let max_bin = ((MAX_FREQ_HZ / freq_res) as usize + 1)
            .min(throttle_map.first().map_or(0, |r| r.len()));

        if max_bin == 0 {
            ui.centered_and_justified(|ui| {
                ui.label("No throttle data available");
            });
            return;
        }

        // Rebuild texture only when the data source changes.
        let cache_key = (self.selected_axis, self.show_filtered);
        if self.heatmap_texture.as_ref().map_or(true, |(_, k)| *k != cache_key) {
            let tex = build_heatmap_texture(throttle_map, max_bin, ui.ctx());
            self.heatmap_texture = Some((tex, cache_key));
        }
        let texture_id = self.heatmap_texture.as_ref().unwrap().0.id();

        // ── layout ──────────────────────────────────────────────────────────
        let margin_left   = 46.0f32; // Y-axis tick labels
        let margin_bottom = 34.0f32; // X-axis ticks + label
        let margin_top    = 6.0f32;
        let margin_right  = 44.0f32; // color bar + labels

        let (response, painter) = ui.allocate_painter(ui.available_size(), Sense::hover());
        let full = response.rect;

        let inner = Rect::from_min_max(
            Pos2::new(full.min.x + margin_left, full.min.y + margin_top),
            Pos2::new(full.max.x - margin_right, full.max.y - margin_bottom),
        );

        // ── heatmap image ────────────────────────────────────────────────────
        painter.image(
            texture_id,
            inner,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
        painter.rect_stroke(
            inner,
            0.0,
            Stroke::new(1.0, Color32::from_gray(60)),
            egui::StrokeKind::Outside,
        );

        let tick_font  = FontId::proportional(10.0);
        let label_font = FontId::proportional(11.0);
        let tick_col   = Color32::from_gray(180);

        // ── Y-axis (throttle %) ──────────────────────────────────────────────
        painter.text(
            Pos2::new(full.min.x, inner.min.y - 2.0),
            Align2::LEFT_BOTTOM,
            "Throttle",
            tick_font.clone(),
            tick_col,
        );
        for pct in [0u32, 25, 50, 75, 100] {
            let t = 1.0 - pct as f32 / 100.0; // 100 % at top
            let y = inner.min.y + t * inner.height();
            painter.line_segment(
                [Pos2::new(inner.min.x - 3.0, y), Pos2::new(inner.min.x, y)],
                Stroke::new(1.0, tick_col),
            );
            painter.text(
                Pos2::new(inner.min.x - 5.0, y),
                Align2::RIGHT_CENTER,
                format!("{}%", pct),
                tick_font.clone(),
                tick_col,
            );
        }

        // ── X-axis (frequency) ───────────────────────────────────────────────
        for freq in [0u32, 200, 400, 600, 800, 1000, 1200, 1400, 1600] {
            if freq as f32 > MAX_FREQ_HZ {
                break;
            }
            let t = freq as f32 / MAX_FREQ_HZ;
            let x = inner.min.x + t * inner.width();
            painter.line_segment(
                [Pos2::new(x, inner.max.y), Pos2::new(x, inner.max.y + 3.0)],
                Stroke::new(1.0, tick_col),
            );
            painter.text(
                Pos2::new(x, inner.max.y + 5.0),
                Align2::CENTER_TOP,
                format!("{}", freq),
                tick_font.clone(),
                tick_col,
            );
        }
        painter.text(
            Pos2::new(inner.center().x, full.max.y - 2.0),
            Align2::CENTER_BOTTOM,
            "Frequency (Hz)",
            label_font,
            tick_col,
        );

        // ── color bar ────────────────────────────────────────────────────────
        let bar_x = inner.max.x + 10.0;
        let bar_w = 12.0;
        let n_steps = inner.height() as usize;
        let step_h = inner.height() / n_steps as f32;
        for i in 0..n_steps {
            let t = 1.0 - i as f32 / n_steps as f32;
            painter.rect_filled(
                Rect::from_min_size(
                    Pos2::new(bar_x, inner.min.y + i as f32 * step_h),
                    Vec2::new(bar_w, step_h + 0.5),
                ),
                0.0,
                heat_color(t),
            );
        }
        painter.rect_stroke(
            Rect::from_min_size(Pos2::new(bar_x, inner.min.y), Vec2::new(bar_w, inner.height())),
            0.0,
            Stroke::new(1.0, Color32::from_gray(60)),
            egui::StrokeKind::Outside,
        );
        painter.text(
            Pos2::new(bar_x + bar_w / 2.0, inner.min.y - 2.0),
            Align2::CENTER_BOTTOM,
            "High",
            tick_font.clone(),
            tick_col,
        );
        painter.text(
            Pos2::new(bar_x + bar_w / 2.0, inner.max.y + 2.0),
            Align2::CENTER_TOP,
            "Low",
            tick_font,
            tick_col,
        );
    }
}
