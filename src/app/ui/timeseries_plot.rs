use egui::{Color32, Id, Vec2b};
use egui_plot::{Legend, Line, Plot, PlotMemory, PlotPoints};

use crate::signal::timeseries::windowed_downsample;

pub struct Series<'a> {
    pub label: String,
    pub color: Color32,
    pub time_us: &'a [u64],
    pub samples: &'a [f64],
}

pub struct TimeseriesPlot<'a> {
    pub id: String,
    pub y_label: String,
    pub t0: u64,
    pub series: Vec<Series<'a>>,
    pub default_x_range: Option<(f64, f64)>,
    pub height: Option<f32>,
}

impl TimeseriesPlot<'_> {
    /// Series visibility is the legend's job, so nothing here reads it back.
    /// `egui_plot` drops hidden items only *after* the build closure returns,
    /// though, so a hidden series would still pay for its downsample — the
    /// closure checks `hidden_items` itself to skip that work.
    pub fn show(&self, ui: &mut egui::Ui) {
        let bucket_count = (ui.available_width().max(1.0) as usize).max(1);

        // Same derivation `Plot` uses for its own memory key.
        let plot_id = ui.make_persistent_id(Id::new(&self.id));
        let hidden = PlotMemory::load(ui.ctx(), plot_id)
            .map(|memory| memory.hidden_items)
            .unwrap_or_default();

        let (y_min, y_max) = self
            .series
            .iter()
            .flat_map(|s| s.samples.iter().copied())
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
                (lo.min(v), hi.max(v))
            });
        let (y_min, y_max) = if y_min.is_finite() && y_max.is_finite() && y_min < y_max {
            (y_min, y_max)
        } else {
            (-1.0, 1.0)
        };

        let mut plot = Plot::new(&self.id)
            .legend(Legend::default())
            .link_axis("shared_time", Vec2b::new(true, false))
            .link_cursor("shared_time", Vec2b::new(true, false))
            .allow_zoom(Vec2b::new(true, true))
            .allow_scroll(Vec2b::new(true, false))
            .allow_drag(Vec2b::new(true, true))
            .center_y_axis(true)
            .auto_bounds(Vec2b::new(false, false))
            .default_y_bounds(y_min, y_max)
            .y_axis_label(&self.y_label);

        if let Some((min, max)) = self.default_x_range {
            plot = plot.default_x_bounds(min, max);
        }
        if let Some(height) = self.height {
            plot = plot.height(height);
        }

        let t0 = self.t0;
        plot.show(ui, |plot_ui| {
            let bounds = plot_ui.plot_bounds();
            let (visible_start, visible_end) = (bounds.min()[0], bounds.max()[0]);

            for s in &self.series {
                // Registered with no points rather than skipped: the legend is
                // built from the item list, so dropping it would take the entry
                // with it and leave the trace with no way back.
                if hidden.contains(&Id::new(&s.label)) {
                    plot_ui.line(Line::new(s.label.clone(), PlotPoints::default()).color(s.color));
                    continue;
                }

                let points = windowed_downsample(
                    s.time_us,
                    s.samples,
                    t0,
                    visible_start,
                    visible_end,
                    bucket_count,
                );
                plot_ui.line(Line::new(s.label.clone(), PlotPoints::from(points)).color(s.color));
            }
        });
    }
}
