use egui::{Color32, ColorImage, TextureOptions, Vec2};
use egui_plot::{Line, Plot, PlotImage, PlotPoint, PlotPoints};

use crate::signal::fft::BinnedSpectrum;
use crate::signal::timeseries::windowed_downsample;

/// A tracked-frequency line to overlay on the heatmap, in plot space (time on
/// x, Hz on y). Only meaningful for [`HeatmapOrientation::VsTime`].
pub struct OverlaySeries<'a> {
    pub t0: u64,
    pub time_us: &'a [u64],
    pub samples: &'a [f64],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeatmapOrientation {
    /// Frequency on x, bin (e.g. throttle) on y.
    VsThrottle,
    /// Bin (time) on x, frequency on y.
    VsTime,
}

impl HeatmapOrientation {
    fn axis_labels(self) -> (&'static str, &'static str) {
        match self {
            Self::VsThrottle => ("Hz", "throttle"),
            Self::VsTime => ("s", "Hz"),
        }
    }
}

/// `floor_db` is the noise floor, in dB relative to the map's peak (which
/// sits at 0dB) — the user-controlled "sensitivity" of the color scale.
/// Anything at or below it maps to the coldest color.
pub struct Heatmap<'a> {
    pub id: String,
    pub orientation: HeatmapOrientation,
    pub spectrum: &'a BinnedSpectrum,
    pub height: f32,
    pub floor_db: f64,
    pub overlay: Option<OverlaySeries<'a>>,
}

impl Heatmap<'_> {
    pub fn show(&self, ui: &mut egui::Ui) {
        let freq_count = self.spectrum.freq_hz.len();
        let n_bins = self.spectrum.bin_centers.len();
        if freq_count < 2 || n_bins == 0 {
            ui.label("Not enough data");
            return;
        }

        let db_range = (0.0 - self.floor_db).max(f64::MIN_POSITIVE);
        let x_is_freq = self.orientation == HeatmapOrientation::VsThrottle;
        let (img_w, img_h) = if x_is_freq {
            (freq_count, n_bins)
        } else {
            (n_bins, freq_count)
        };

        // Texture rows run top-to-bottom, but plot y grows upward, so the
        // vertical axis is flipped to line the image up with the plot's y axis.
        let mut pixels = vec![Color32::TRANSPARENT; img_w * img_h];
        for (bin, row) in self.spectrum.power_db.iter().enumerate() {
            for (freq_idx, &v) in row.iter().enumerate() {
                if !v.is_finite() {
                    continue;
                }
                let (col, pixel_row) = if x_is_freq {
                    (freq_idx, n_bins - 1 - bin)
                } else {
                    (bin, freq_count - 1 - freq_idx)
                };
                pixels[pixel_row * img_w + col] =
                    heat_color(((v - self.floor_db) / db_range) as f32);
            }
        }
        let texture = ui.ctx().load_texture(
            self.id.as_str(),
            ColorImage::new([img_w, img_h], pixels),
            TextureOptions::LINEAR,
        );

        let freq_min = self.spectrum.freq_hz[0];
        let freq_max = self.spectrum.freq_hz[freq_count - 1];
        let bin_width = if n_bins > 1 {
            self.spectrum.bin_centers[1] - self.spectrum.bin_centers[0]
        } else {
            1.0
        };
        let bin_min = self.spectrum.bin_centers[0] - bin_width / 2.0;
        let bin_max = self.spectrum.bin_centers[n_bins - 1] + bin_width / 2.0;

        let (center, size) = if x_is_freq {
            (
                PlotPoint::new((freq_min + freq_max) / 2.0, (bin_min + bin_max) / 2.0),
                Vec2::new((freq_max - freq_min) as f32, (bin_max - bin_min) as f32),
            )
        } else {
            (
                PlotPoint::new((bin_min + bin_max) / 2.0, (freq_min + freq_max) / 2.0),
                Vec2::new((bin_max - bin_min) as f32, (freq_max - freq_min) as f32),
            )
        };

        let (x_label, y_label) = self.orientation.axis_labels();
        let bucket_count = (ui.available_width().max(1.0) as usize).max(1);
        let overlay = &self.overlay;

        Plot::new(self.id.as_str())
            .height(self.height)
            .x_axis_label(x_label)
            .y_axis_label(y_label)
            .show(ui, |plot_ui| {
                plot_ui.image(PlotImage::new(self.id.as_str(), &texture, center, size));

                if let Some(overlay) = overlay {
                    let bounds = plot_ui.plot_bounds();
                    let points = windowed_downsample(
                        overlay.time_us,
                        overlay.samples,
                        overlay.t0,
                        bounds.min()[0],
                        bounds.max()[0],
                        bucket_count,
                    );
                    plot_ui.line(
                        Line::new("tracked center freq", PlotPoints::from(points))
                            .color(Color32::WHITE),
                    );
                }
            });
    }
}

fn heat_color(t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let (r, g, b) = if t < 0.5 {
        let s = t * 2.0;
        (0.0, s, 1.0 - s)
    } else {
        let s = (t - 0.5) * 2.0;
        (s, 1.0 - s, 0.0)
    };
    Color32::from_rgb((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}
