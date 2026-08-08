use egui::{Color32, RichText, Ui};
use egui_plot::{Line, Plot, PlotPoints};

use crate::analysis::{AxisStepResponse, NoStepResponse};
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

        // Only the axes that draw share the height: a craft logging one axis
        // gets a full-size plot, not a third of the panel and two dead gaps.
        let drawn = Axis::ALL
            .iter()
            .filter(|&&a| ctx.analysis.step.axis(a).is_ok())
            .count();
        let plot_height = stacked_plot_height(ui, drawn);

        for axis in Axis::ALL {
            match ctx.analysis.step.axis(axis) {
                Ok(response) => self.show_axis(ui, axis, response, plot_height),
                Err(reason) => {
                    ui.label(RichText::new(axis.name()).strong());
                    ui.label(explain(axis, reason));
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
}

/// An empty axis is never silently blank — the analyser says which of its
/// exits was taken and this turns that into something a pilot can act on.
fn explain(axis: Axis, reason: NoStepResponse) -> String {
    let i = axis.index();

    match reason {
        NoStepResponse::SetpointNotLogged => format!(
            "No setpoint[{i}] in this log — the step response is gyro deconvolved from \
             setpoint, so there is nothing to compare against. Enable the Setpoint field \
             in Betaflight's Blackbox tab and fly again."
        ),
        NoStepResponse::GyroNotLogged => format!(
            "No gyroADC[{i}] in this log — nothing recorded how the craft answered the sticks."
        ),
        NoStepResponse::LogTooShort => {
            "This log is shorter than one analysis window. Fly for a few seconds longer."
                .to_string()
        }
        NoStepResponse::SticksTooStill { min_setpoint_dps } => format!(
            "The sticks never moved more than {min_setpoint_dps:.0} deg/s on this axis. Fly \
             some rolls, flips or hard direction changes and the step response will have \
             something to work from."
        ),
        NoStepResponse::NoSteadyState => format!(
            "The sticks moved but gyro axis {i} never settled anywhere — check that the \
             craft was armed and flying for this log."
        ),
    }
}
