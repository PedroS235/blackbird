use egui::{Color32, ColorImage, TextureOptions, Vec2, Vec2b};
use egui_plot::{Line, LineStyle, Plot, PlotImage, PlotPoint, PlotPoints};

use crate::signal::fft::BinnedSpectrum;
use crate::signal::timeseries::windowed_downsample;

/// A frequency-over-time line to overlay on the heatmap, in plot space (time on
/// x, Hz on y). Only meaningful for [`HeatmapOrientation::VsTime`].
///
/// A list rather than one: the dynamic notch tracker's centre used to be the
/// only thing drawn here and was a special case, and per-motor harmonic curves
/// are a dozen more of the same kind of thing. Each carries its own colour and
/// style, so the panel that built it owns the identity scheme.
#[derive(Clone)]
pub struct OverlaySeries<'a> {
    pub name: String,
    pub t0: u64,
    pub time_us: &'a [u64],
    /// Borrowed raw, in whatever unit the log holds — `scale` converts.
    pub samples: &'a [f64],
    /// Applied after decimation. Min-max decimation commutes with a positive
    /// scale, so a motor's eRPM is borrowed as logged and converted once per
    /// *drawn* point, rather than a converted copy of every sample being built
    /// per frame for every motor at every order.
    pub scale: f64,
    pub color: Color32,
    pub style: LineStyle,
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
    pub overlays: Vec<OverlaySeries<'a>>,
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
        let overlays = &self.overlays;

        Plot::new(self.id.as_str())
            .height(self.height)
            .x_axis_label(x_label)
            .allow_zoom(Vec2b::new(true, true))
            .allow_scroll(Vec2b::new(false, false))
            .allow_drag(Vec2b::new(true, true))
            .y_axis_label(y_label)
            .show(ui, |plot_ui| {
                plot_ui.image(PlotImage::new(self.id.as_str(), &texture, center, size));

                for overlay in overlays {
                    let bounds = plot_ui.plot_bounds();
                    let points: Vec<[f64; 2]> = windowed_downsample(
                        overlay.time_us,
                        overlay.samples,
                        overlay.t0,
                        bounds.min()[0],
                        bounds.max()[0],
                        bucket_count,
                    )
                    .into_iter()
                    .map(|[t, v]| [t, v * overlay.scale])
                    .collect();

                    // One line per run that stays inside the map, so a curve
                    // that leaves it breaks instead of being joined across the
                    // gap by a chord at frequencies the motor never occupied.
                    // Only the first run is named: twelve curves already, and
                    // a hover listing the same motor five times says less.
                    for (i, run) in drawable_runs(&points, freq_min, freq_max)
                        .into_iter()
                        .enumerate()
                    {
                        let name = match i {
                            0 => overlay.name.clone(),
                            _ => String::new(),
                        };
                        plot_ui.line(
                            Line::new(name, PlotPoints::from(run.to_vec()))
                                .color(overlay.color)
                                .style(overlay.style),
                        );
                    }
                }
            });
    }
}

/// The runs of consecutive points the map can actually show, split where the
/// curve leaves it.
///
/// The image sets the plot's bounds, so a third harmonic above Nyquist would
/// stretch them until the heatmap is a stripe. Zero is dropped with the rest —
/// a stopped motor and a tracker with nothing to report are both logged as 0,
/// and neither is a frequency the craft flew.
fn drawable_runs(points: &[[f64; 2]], freq_min: f64, freq_max: f64) -> Vec<&[[f64; 2]]> {
    points
        .split(|&[_, v]| !(v > 0.0 && (freq_min..=freq_max).contains(&v)))
        .filter(|run| !run.is_empty())
        .collect()
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

#[cfg(test)]
mod test {
    use super::*;

    /// A curve that leaves the map breaks in two rather than being joined
    /// across the gap: a chord drawn over the clipped span sits at frequencies
    /// the motor never occupied, which is a lie the pilot cannot see is one.
    #[test]
    fn a_curve_that_leaves_the_map_breaks_instead_of_being_joined() {
        let points = [
            [0.0, 100.0],
            [1.0, 200.0],
            [2.0, 900.0],
            [3.0, 250.0],
            [4.0, 300.0],
        ];

        let runs = drawable_runs(&points, 1.0, 500.0);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0], &points[0..2]);
        assert_eq!(runs[1], &points[3..5]);
    }

    /// A stopped motor is logged as zero eRPM, which is not a frequency at
    /// all — and nothing is drawn for it, at either end of the flight.
    #[test]
    fn a_stopped_motor_draws_nothing_rather_than_a_line_at_zero() {
        let points = [[0.0, 0.0], [1.0, 300.0], [2.0, 0.0]];

        let runs = drawable_runs(&points, 1.0, 500.0);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0], &points[1..2]);
    }

    /// A curve wholly inside the map is one unbroken line.
    #[test]
    fn a_curve_inside_the_map_stays_one_line() {
        let points = [[0.0, 100.0], [1.0, 200.0], [2.0, 300.0]];

        assert_eq!(drawable_runs(&points, 1.0, 500.0), vec![&points[..]]);
    }
}
