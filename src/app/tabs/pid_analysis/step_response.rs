use std::ops::RangeInclusive;
use std::sync::Arc;

use egui::{Color32, DragValue, RichText, Ui};
use egui_plot::{Line, MarkerShape, Plot, PlotPoints, Points};

use crate::analysis::{
    AxisStepResponse, NoStepResponse, StepMetrics, StepResponseAnalysis, StepResponseAnalyzer,
};
use crate::app::tabs::{GYRO_AXIS_COLORS, TabCtx, stacked_plot_height};
use crate::parser::Axis;

/// Individual traces are background: the eye should read the spread as a band,
/// with the mean on top of it.
const TRACE_ALPHA: u8 = 40;
const MEAN_WIDTH: f32 = 2.0;
const PEAK_MARKER_RADIUS: f32 = 4.0;

/// Below this many responses the numbers are still shown — "not much data" has
/// to be tellable from "nothing was computed" — but they carry the count. A
/// statement about statistics rather than about flying, so not a knob. On the
/// fixture's freestyle logs even the hardest preset averages 40.
const THIN_STACK: usize = 10;

/// The stick mask is not a quality gate — the median second of even a hard
/// freestyle log peaks around 20 deg/s — it is a choice of which inputs the
/// curve describes, and a discipline is how a pilot says that. It changes the
/// answer: on the fixture's freestyle logs overshoot falls from 1.20 at
/// 25 deg/s to 1.12 at 120, because big inputs meet rate limits and prop
/// saturation that small ones never reach.
///
/// Racing sits at Betaflight's default centre stick sensitivity; the other two
/// bracket it. All three keep enough traces on a real flight to average (40 at
/// the hardest), and the gentlest still analyses a cinematic log that 52
/// rejects outright.
const STICK_PRESETS: [(&str, f64, &str); 3] = [
    (
        "Cinematic",
        25.0,
        "Gentle pans and drifts. About the floor where a soft flight still answers — \
         below this the deconvolution is reading hover jitter.",
    ),
    (
        "Racing",
        70.0,
        "Committed inputs: hard direction changes rather than trim corrections.",
    ),
    (
        "Freestyle",
        120.0,
        "Flips and rolls only — the inputs that take the craft to its rate limits.",
    ),
];

/// The knobs' last result, kept so that dragging one slider does not re-run
/// the stack on every frame. Identified by the log's time axis rather than an
/// index, so a store that reallocates cannot alias two flights.
struct Cached {
    time: Arc<Vec<u64>>,
    analyzer: StepResponseAnalyzer,
    analysis: StepResponseAnalysis,
}

/// Panel state, not shared state — following `Psd`/`Frequency`, whose toggles
/// used to be shared and silently moved together. The knobs live here too, so
/// they survive a log or sublog switch and two flights can be compared under
/// identical analysis.
pub(super) struct StepResponse {
    show_individual: bool,
    analyzer: StepResponseAnalyzer,
    cached: Option<Cached>,
}

impl Default for StepResponse {
    fn default() -> Self {
        // The spread across traces is the information; hiding it by default
        // would leave a mean curve with nothing to judge it against.
        Self {
            show_individual: true,
            analyzer: StepResponseAnalyzer::default(),
            cached: None,
        }
    }
}

impl StepResponse {
    pub(super) fn show(&mut self, ui: &mut Ui, ctx: &TabCtx<'_>) {
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.show_individual, "show individual responses");
            ui.separator();

            // Top level rather than inside the knobs: picking a discipline is
            // a pilot-level question, where the deg/s behind it is not.
            ui.label("stick input:");
            for (name, dps, hint) in STICK_PRESETS {
                let chosen = self.analyzer.min_setpoint_dps == dps;
                if ui
                    .selectable_label(chosen, name)
                    .on_hover_text(format!("{hint}\n\nMinimum stick input {dps:.0} deg/s."))
                    .clicked()
                {
                    self.analyzer.min_setpoint_dps = dps;
                }
            }

            // What the presets mean depends on the craft: 120 deg/s is a flick
            // on 1200 deg/s rates and a full flip on 400.
            if let Some(rates) = &ctx.metadata.rates {
                ui.separator();
                ui.label(RichText::new(rates.to_string()).weak())
                    .on_hover_text(
                        "The rate curve this flight was flown on, as the log records it: type, \
                         then the roll/pitch/yaw rate values. Not deg/s — turning these into a \
                         maximum rate needs a different formula per rate type.",
                    );
            }
        });

        let dragging = self.show_knobs(ui);
        ui.add_space(4.0);

        // Copied out before the analysis borrows `self` for the rest of the
        // frame — the plots read it, the recompute owns everything else.
        let show_individual = self.show_individual;
        let step = self.analysis(ctx, dragging);

        // Only the axes that draw share the height: a craft logging one axis
        // gets a full-size plot, not a third of the panel and two dead gaps.
        let drawn = Axis::ALL.iter().filter(|&&a| step.axis(a).is_ok()).count();
        let plot_height = stacked_plot_height(ui, drawn);

        for axis in Axis::ALL {
            match step.axis(axis) {
                Ok(response) => show_axis(ui, axis, response, plot_height, show_individual),
                Err(reason) => {
                    ui.label(RichText::new(axis.name()).strong());
                    ui.label(explain(axis, reason));
                    ui.add_space(8.0);
                }
            }
        }
    }

    /// At the defaults the load-time analysis is exactly what these knobs
    /// would produce — `LogLoader.step_response`, which the app never sets, is
    /// the same value — so the panel draws it and computes nothing.
    ///
    /// Past that, recompute is synchronous, which a knob being *dragged* could
    /// not afford: a `DragValue` changes on every mouse-move frame, and a
    /// five-minute log is a few hundred milliseconds per run at 1 kHz and
    /// seconds at 8 kHz. So a drag in progress keeps drawing the last result
    /// and the stack re-runs once, when the knob is let go.
    fn analysis<'a>(&'a mut self, ctx: &'a TabCtx<'_>, dragging: bool) -> &'a StepResponseAnalysis {
        if self.analyzer == StepResponseAnalyzer::default() {
            self.cached = None;
            return &ctx.analysis.step;
        }

        let time = ctx.flight.time_handle();
        let fresh = self
            .cached
            .as_ref()
            .is_some_and(|c| Arc::ptr_eq(&c.time, &time) && c.analyzer == self.analyzer);

        // A first drag has nothing cached to show, so it computes once and
        // then holds that until the knob is released.
        if !fresh && !(dragging && self.cached.is_some()) {
            self.cached = Some(Cached {
                analysis: self.analyzer.analyze(ctx.flight),
                analyzer: self.analyzer.clone(),
                time,
            });
        }

        &self.cached.as_ref().expect("just computed").analysis
    }

    /// Collapsed by default, so the pilot who only wants the curve sees the
    /// same panel as before. Every knob is in the units it means — seconds and
    /// deg/s, never FFT lengths — and names its default.
    ///
    /// Returns whether a knob is under the pointer right now, which is what
    /// keeps a drag from re-running the stack on every frame of it.
    fn show_knobs(&mut self, ui: &mut Ui) -> bool {
        egui::CollapsingHeader::new("analysis parameters")
            .show(ui, |ui| self.knob_grid(ui))
            .body_returned
            .unwrap_or(false)
    }

    fn knob_grid(&mut self, ui: &mut Ui) -> bool {
        let d = StepResponseAnalyzer::default();
        let a = &mut self.analyzer;
        // The band is one field but two knobs, so it is taken apart here
        // and put back together below.
        let (mut low, mut high) = (*a.steady_state_band.start(), *a.steady_state_band.end());

        // No response-length knob: `response_ms` sets where the steady-state
        // tail sits, so a control labelled "response length" re-normalised
        // every trace while implying it only changed the view. Looking closer
        // at the rise is plot zoom, which `egui_plot` already provides.
        let knobs: [(&str, &mut f64, RangeInclusive<f64>, f64, &str, String); 6] = [
            (
                "window",
                &mut a.window_s,
                0.1..=5.0,
                0.01,
                " s",
                format!(
                    "How much flight each response is measured over. Longer holds lower \
                          frequencies, shorter keeps the tune constant across it. Below {} s it \
                          also truncates the response, which moves the steady state every trace \
                          is normalised by. Default {} s.",
                    d.response_ms / 1e3,
                    d.window_s
                ),
            ),
            (
                "hop",
                &mut a.hop_s,
                0.01..=2.0,
                0.005,
                " s",
                format!(
                    "How far the window moves between responses. Smaller stacks more \
                          traces and costs more time. Default {:.4} s.",
                    d.hop_s
                ),
            ),
            (
                "minimum stick input",
                &mut a.min_setpoint_dps,
                0.0..=500.0,
                1.0,
                " deg/s",
                format!(
                    "How hard the sticks must move for a second of flight to count. The \
                          presets above set it per discipline; this is the same knob. \
                          Default {} deg/s.",
                    d.min_setpoint_dps
                ),
            ),
            (
                "λ (regularisation)",
                &mut a.lambda_k,
                1e-5..=1.0,
                0.0005,
                "",
                format!(
                    "How much the deconvolution is smoothed where the sticks carried no \
                          energy. Raise it to see whether an overshoot is real. Default {}.",
                    d.lambda_k
                ),
            ),
            (
                "steady state ≥",
                &mut low,
                0.0..=10.0,
                0.01,
                "×",
                format!(
                    "A response settling below this share of the commanded rate is \
                          discarded. Default {}×.",
                    d.steady_state_band.start()
                ),
            ),
            (
                "steady state ≤",
                &mut high,
                0.0..=20.0,
                0.01,
                "×",
                format!(
                    "A response settling above this multiple of the commanded rate is \
                          discarded. Default {}×.",
                    d.steady_state_band.end()
                ),
            ),
        ];

        let dragging = egui::Grid::new("step_response_knobs")
            .num_columns(2)
            .show(ui, |ui| {
                let mut dragging = false;
                for (label, value, range, speed, suffix, hint) in knobs {
                    ui.label(label).on_hover_text(&hint);
                    let knob = ui
                        .add(
                            DragValue::new(value)
                                .speed(speed)
                                .range(range)
                                .suffix(suffix),
                        )
                        .on_hover_text(hint);
                    dragging |= knob.dragged();
                    ui.end_row();
                }
                dragging
            })
            .inner;

        // A low dragged past high would be an empty band that silently
        // rejects everything.
        a.steady_state_band = low..=high.max(low);

        if ui.button("reset to defaults").clicked() {
            self.analyzer = d;
        }

        dragging
    }
}

fn show_axis(
    ui: &mut Ui,
    axis: Axis,
    response: &AxisStepResponse,
    height: f32,
    show_individual: bool,
) {
    let color = GYRO_AXIS_COLORS[axis];

    ui.horizontal(|ui| {
        ui.label(RichText::new(axis.name()).strong());
        ui.label(format!("mean of {} responses", response.count));
        ui.separator();
        ui.label(metrics_line(&response.metrics, response.count))
            .on_hover_text(
                "Overshoot is how far past the commanded rate the averaged curve went, peak is \
                 when it got there and delay how long it took to reach half the commanded rate. \
                 The range is the middle half of the individual responses: their peaks land at \
                 slightly different times, so averaging flattens them and the curve's own \
                 overshoot normally sits below that range. A wide range means the responses \
                 behind the curve disagree.",
            );
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
            // Every retained trace: the analyser's `max_traces` is the one
            // owner of how dense this band is, so a second cap here cannot
            // thin it out as the stack behind it grows.
            if show_individual {
                let faded =
                    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), TRACE_ALPHA);

                for (i, trace) in response.sample.iter().enumerate() {
                    plot_ui.line(Line::new(format!("response {i}"), points(trace)).color(faded));
                }
            }

            // Last, so the mean sits on top of the band.
            plot_ui.line(
                Line::new("mean", points(&response.mean))
                    .color(color)
                    .width(MEAN_WIDTH),
            );

            // So the overshoot number and the picture are visibly the same
            // claim. Rise and delay stay numeric — three annotations across
            // three stacked plots is noise.
            let m = &response.metrics;
            plot_ui.points(
                Points::new("peak", vec![[m.peak_ms, m.peak_normalised()]])
                    .color(color)
                    .shape(MarkerShape::Diamond)
                    .radius(PEAK_MARKER_RADIUS),
            );
        });
}

/// The line a pilot would say out loud. Whole percentages and whole
/// milliseconds throughout — the measurement supports no more precision than
/// that.
fn metrics_line(m: &StepMetrics, count: usize) -> String {
    let line = format!(
        "{:.0}% overshoot (individual responses {:.0}–{:.0}%), peak {:.0} ms, delay {:.0} ms",
        m.overshoot_pct,
        m.spread_pct.start(),
        m.spread_pct.end(),
        m.peak_ms,
        m.delay_ms
    );

    match count < THIN_STACK {
        true => format!("{line} — from only {count} responses"),
        false => line,
    }
}

/// An empty axis is never silently blank — the analyser says which of its
/// exits was taken and this turns that into something a pilot can act on,
/// naming the knob to walk back where there is one.
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
            "This log is shorter than one analysis window. Fly for a few seconds longer, or \
             shorten the window."
                .to_string()
        }
        NoStepResponse::SticksTooStill { min_setpoint_dps } => format!(
            "The sticks never moved more than {min_setpoint_dps:.0} deg/s on this axis. Fly \
             some rolls, flips or hard direction changes and the step response will have \
             something to work from, or lower the minimum stick input."
        ),
        NoStepResponse::NoSteadyState { band } => format!(
            "The sticks moved but no response on gyro axis {i} settled between {:.2}× and \
             {:.2}× the commanded rate — check that the craft was armed and flying, or widen \
             the steady-state band.",
            band.start(),
            band.end()
        ),
    }
}
