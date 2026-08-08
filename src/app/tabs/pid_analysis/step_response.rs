use egui::{Color32, RichText, Ui};
use egui_plot::{Line, Plot, PlotPoints};

use crate::analysis::step_response::AxisStepResponse;
use crate::app::tabs::{GYRO_AXIS_COLORS, TabCtx, stacked_plot_height};
use crate::parser::Axis;

/// Individual traces are background: the eye should read the spread as a band,
/// with the mean on top of it.
const TRACE_ALPHA: u8 = 40;
const MEAN_WIDTH: f32 = 2.0;

/// A dense log stacks hundreds of traces; past a hundred they are pixel-for-
/// pixel the same band and only cost frame time. The mean always comes from
/// all of them.
const MAX_DRAWN_TRACES: usize = 100;

/// The toggle is panel state, not shared state — following `Psd`/`Frequency`,
/// whose toggles used to be shared and silently moved together.
pub(super) struct StepResponse {
    show_individual: bool,
}

impl Default for StepResponse {
    fn default() -> Self {
        // The spread across traces is the information; hiding it by default
        // would leave a mean curve with nothing to judge it against.
        Self {
            show_individual: true,
        }
    }
}

impl StepResponse {
    pub(super) fn show(&mut self, ui: &mut Ui, ctx: &TabCtx<'_>) {
        ui.checkbox(&mut self.show_individual, "show individual responses");
        ui.add_space(4.0);

        let plot_height = stacked_plot_height(ui, 3);

        for axis in Axis::ALL {
            match ctx.analysis.step.axis(axis) {
                Some(response) => self.show_axis(ui, axis, response, plot_height),
                None => {
                    ui.label(RichText::new(axis.name()).strong());
                    ui.label(self.empty_reason(ctx, axis));
                    ui.add_space(8.0);
                }
            }
        }
    }

    fn show_axis(&self, ui: &mut Ui, axis: Axis, response: &AxisStepResponse, height: f32) {
        let color = GYRO_AXIS_COLORS[axis];

        ui.horizontal(|ui| {
            ui.label(RichText::new(axis.name()).strong());
            ui.label(format!("mean of {} responses", response.traces.len()));
        });

        let points = |values: &[f64]| -> PlotPoints {
            response
                .time_ms
                .iter()
                .zip(values)
                .map(|(&t, &v)| [t, v])
                .collect()
        };

        Plot::new(format!("step_response_plot_{}", axis.name()))
            .height(height)
            .x_axis_label("ms")
            .y_axis_label("normalised")
            .show(ui, |plot_ui| {
                if self.show_individual {
                    let stride = response.traces.len().div_ceil(MAX_DRAWN_TRACES).max(1);
                    let faded = Color32::from_rgba_unmultiplied(
                        color.r(),
                        color.g(),
                        color.b(),
                        TRACE_ALPHA,
                    );

                    for (i, trace) in response.traces.iter().enumerate().step_by(stride) {
                        plot_ui
                            .line(Line::new(format!("response {i}"), points(trace)).color(faded));
                    }
                }

                // Last, so the mean sits on top of the band.
                plot_ui.line(
                    Line::new("mean", points(&response.mean))
                        .color(color)
                        .width(MEAN_WIDTH),
                );
            });
    }

    /// An empty axis is never silently blank: either the field was not logged,
    /// or the flying itself never asked the craft a question.
    fn empty_reason(&self, ctx: &TabCtx<'_>, axis: Axis) -> String {
        let mask = ctx.analysis.step.min_setpoint_dps;

        if ctx.flight.setpoint(axis).is_none() {
            format!(
                "No setpoint[{}] in this log — the step response is gyro deconvolved from \
                 setpoint, so there is nothing to compare against. Enable the Setpoint debug \
                 field in Betaflight's Blackbox tab and fly again.",
                axis.index()
            )
        } else if ctx.flight.gyro(axis).is_none() {
            format!(
                "No gyroADC[{}] in this log — nothing recorded how the craft answered the \
                 sticks.",
                axis.index()
            )
        } else {
            format!(
                "The sticks never moved more than {mask:.0} deg/s on this axis in any \
                 analysed window. Fly some rolls, flips or hard direction changes and the \
                 step response will have something to work from."
            )
        }
    }
}
