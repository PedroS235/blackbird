use std::collections::HashMap;
use std::ops::RangeInclusive;

use egui::{Color32, DragValue, RichText, Ui};
use egui_plot::{Line, MarkerShape, Plot, PlotPoints, Points};
use elegance::Palette;

use crate::analysis::{NoStepResponse, StepMetrics, StepResponseAnalysis, StepResponseAnalyzer};
use crate::app::colors;
use crate::app::log_store::FlightKey;
use crate::app::tabs::{TabCtx, stacked_plot_height};
use crate::app::ui::compare::{self, CompareSet};
use crate::parser::{Axis, FlightData};

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

/// The knobs' last results, one per compared flight, kept so that dragging a
/// slider does not re-run the stack on every frame of the drag.
///
/// Keyed by [`FlightKey`]: with up to four flights on screen one slot would
/// thrash, each flight evicting the last every frame. The analyzer is stored
/// once rather than per entry — the knobs are shared by design, and a per-entry
/// copy could disagree, which is exactly the mistake sharing them prevents.
#[derive(Default)]
struct Cache {
    analyzer: Option<StepResponseAnalyzer>,
    entries: HashMap<FlightKey, StepResponseAnalysis>,
}

impl Cache {
    /// At the defaults the load-time analysis is exactly what these knobs would
    /// produce — `LogLoader.step_response`, which the app never sets, is the
    /// same value — so the map stays empty and nothing is computed.
    ///
    /// Past that, recompute is synchronous, which a knob being *dragged* could
    /// not afford: a `DragValue` changes on every mouse-move frame, and a
    /// five-minute log is a few hundred milliseconds per run at 1 kHz and
    /// seconds at 8 kHz — times the number of flights compared. So a drag in
    /// progress keeps what it has and the stack re-runs once, on release, over
    /// the whole set. One call over the set is also where a worker thread goes.
    /// This flight's analysis under the current knobs, or its load-time one —
    /// which at the defaults is the same analysis, and is the only thing there
    /// is to draw for a flight added while a knob is held.
    fn analysis<'a>(
        &'a self,
        key: FlightKey,
        at_load: &'a StepResponseAnalysis,
    ) -> &'a StepResponseAnalysis {
        self.entries.get(&key).unwrap_or(at_load)
    }

    fn refresh(
        &mut self,
        analyzer: &StepResponseAnalyzer,
        dragging: bool,
        flights: &[(FlightKey, &FlightData)],
    ) {
        if *analyzer == StepResponseAnalyzer::default() {
            self.analyzer = None;
            self.entries.clear();
            return;
        }

        // A first drag has nothing cached to show, so it computes once and then
        // holds that until the knob is released.
        let stale = self.analyzer.as_ref() != Some(analyzer);
        if stale && !(dragging && !self.entries.is_empty()) {
            self.entries.clear();
            self.analyzer = Some(analyzer.clone());
        }

        // Flights no longer compared lose their entry, so removing a chip frees
        // its analysis.
        self.entries
            .retain(|key, _| flights.iter().any(|(compared, _)| compared == key));

        // Whatever is missing is analysed under the parameters the rest of the
        // entries were made with — a flight added mid-drag drawn under the
        // knobs' new values would be a curve nothing else on screen shares.
        let Some(cached) = self.analyzer.clone() else {
            return;
        };
        for (key, flight) in flights {
            self.entries
                .entry(*key)
                .or_insert_with(|| cached.analyze(flight));
        }
    }
}

/// One compared flight, resolved for this frame: which slot's colour it wears,
/// what the chip calls it, and the analysis to draw.
struct Compared<'a> {
    slot: usize,
    label: String,
    step: &'a StepResponseAnalysis,
}

/// One flight's row of the metrics grid.
struct MetricsRow {
    slot: usize,
    label: String,
    count: usize,
    metrics: StepMetrics,
}

/// How the numbers are said. Prose reads well as a sentence; three sentences
/// are a table pretending not to be, and aligning the numbers vertically is the
/// entire reason to compare them.
enum MetricsView {
    Prose(MetricsRow),
    Grid(Vec<MetricsRow>),
}

/// Panel state, not shared state — following `Psd`/`Frequency`, whose toggles
/// used to be shared and silently moved together. The knobs live here too, so
/// they survive a log or sublog switch and two flights can be compared under
/// identical analysis — and so does which flights are being compared, for the
/// same reason.
#[derive(Default)]
pub(super) struct StepResponse {
    /// Off: `StepMetrics.spread_pct` reports the inter-quartile range of the
    /// per-trace peaks and the line above the plot prints it, so the band is not
    /// the only witness to the spread. For most pilots the mean is the only line
    /// that matters, and the band is what they see first.
    show_individual: bool,
    analyzer: StepResponseAnalyzer,
    compare: CompareSet,
    cache: Cache,
}

impl StepResponse {
    pub(super) fn show(&mut self, ui: &mut Ui, ctx: &TabCtx<'_>) {
        let slots = compare::show(ui, &mut self.compare, ctx.base, ctx.catalog);
        let comparing = slots.len() > 1;
        ui.add_space(4.0);

        ui.horizontal(|ui| {
            ui.add_enabled(
                !comparing,
                egui::Checkbox::new(&mut self.show_individual, "show individual responses"),
            )
            .on_hover_text(match comparing {
                // Honest about why rather than silently ignoring the click.
                true => "One flight at a time: several overlaid bands are mud.".to_string(),
                false => "Draws the retained responses behind the mean, so a clean mean of \
                          agreement is tellable from a mean of two different flight regimes. The \
                          range printed beside the numbers is the same claim, counted rather than \
                          drawn."
                    .to_string(),
            });
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
            // on 1200 deg/s rates and a full flip on 400. The base flight's,
            // since the chips carry the others' on hover and say when they
            // differ.
            if let Some(rates) = &ctx.metadata.rates {
                ui.separator();
                ui.label(RichText::new(rates.to_string()).weak())
                    .on_hover_text(
                        "The rate curve the selected flight was flown on, as the log records it: \
                         type, then the roll/pitch/yaw rate values. Not deg/s — turning these \
                         into a maximum rate needs a different formula per rate type.",
                    );
            }
        });

        let dragging = self.show_knobs(ui, ctx.flight.duration_s() / 4.0);
        ui.add_space(4.0);

        // Resolved once, so the cache and the plots below agree on which
        // flights are on screen even if the store changed under a stale key.
        let resolved: Vec<(FlightKey, &FlightData)> = slots
            .iter()
            .filter_map(|&key| Some((key, ctx.catalog.resolve(key)?.flight)))
            .collect();
        self.cache.refresh(&self.analyzer, dragging, &resolved);

        // Copied out before the cache is borrowed for the rest of the frame.
        let show_individual = self.show_individual && !comparing;
        let compared: Vec<Compared<'_>> = slots
            .iter()
            .enumerate()
            .filter_map(|(slot, &key)| {
                let flight = ctx.catalog.resolve(key)?;
                Some(Compared {
                    slot,
                    label: ctx.catalog.label(key).unwrap_or_default(),
                    step: self.cache.analysis(key, &flight.analysis.step),
                })
            })
            .collect();

        // Only the axes that draw share the height: a craft logging one axis
        // gets a full-size plot, not a third of the panel and two dead gaps.
        let drawn = drawn_axes(&compared);
        let plot_height = stacked_plot_height(ui, drawn.len());
        let palette = colors::palette(ui.ctx());

        for axis in Axis::ALL {
            ui.label(RichText::new(axis.name()).strong());

            // Union, not intersection: one setpoint-less sublog must not blank
            // an axis for the whole comparison — but every flight that cannot
            // fill it still says so, by name now that several are on screen.
            for note in axis_notes(axis, &compared) {
                ui.label(note);
            }

            match drawn.contains(&axis) {
                true => show_axis(ui, &palette, axis, &compared, plot_height, show_individual),
                false => ui.add_space(8.0),
            }
        }
    }

    /// Collapsed by default, so the pilot who only wants the curve sees the
    /// same panel as before. Every knob is in the units it means — seconds and
    /// deg/s, never FFT lengths — and names its default.
    ///
    /// Returns whether a knob is under the pointer right now, which is what
    /// keeps a drag from re-running the stack on every frame of it.
    fn show_knobs(&mut self, ui: &mut Ui, max_trim_s: f64) -> bool {
        egui::CollapsingHeader::new("analysis parameters")
            .show(ui, |ui| self.knob_grid(ui, max_trim_s))
            .body_returned
            .unwrap_or(false)
    }

    /// `max_trim_s` is this log's share to spare — past it the analyser stops
    /// trimming altogether, so a knob that went further would jump the curve
    /// back to the whole flight with nothing on screen saying why.
    fn knob_grid(&mut self, ui: &mut Ui, max_trim_s: f64) -> bool {
        let d = StepResponseAnalyzer::default();
        let a = &mut self.analyzer;
        // The band is one field but two knobs, so it is taken apart here
        // and put back together below.
        let (mut low, mut high) = (*a.steady_state_band.start(), *a.steady_state_band.end());
        let trim_cap = max_trim_s.max(a.trim_s);
        let over_cap = a.trim_s > max_trim_s;

        // No response-length knob: `response_ms` sets where the steady-state
        // tail sits, so a control labelled "response length" re-normalised
        // every trace while implying it only changed the view. Looking closer
        // at the rise is plot zoom, which `egui_plot` already provides.
        let knobs: [(&str, &mut f64, RangeInclusive<f64>, f64, &str, String); 7] = [
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
                "trim log ends",
                &mut a.trim_s,
                // Never below where it already sits: a `DragValue` writes its
                // range back, so a short log would silently pull the knob off
                // the default and make the panel recompute what it was handed.
                0.0..=trim_cap,
                0.1,
                " s",
                format!(
                    "How much of each end of the log to leave out. The craft is arming, in a \
                          hand or landing there, and it answers the sticks like something else. \
                          Trimming never takes more than half a flight, so this log stops at \
                          {max_trim_s:.1} s. Default {} s.",
                    d.trim_s
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

        // Said on screen rather than only in the tooltip: past the cap the
        // analyser trims nothing, and a knob that reads 2 s while the whole
        // log is being analysed is a lie.
        if over_cap {
            ui.label(
                RichText::new(format!(
                    "log too short to spare {:.1} s each end — analysed whole",
                    self.analyzer.trim_s
                ))
                .weak(),
            );
        }

        if ui.button("reset to defaults").clicked() {
            self.analyzer = d;
        }

        dragging
    }
}

/// Axes any compared flight can fill. The union: a flight whose axis errored
/// contributes no curve but must not take the axis away from the flights that
/// have one.
fn drawn_axes(compared: &[Compared<'_>]) -> Vec<Axis> {
    Axis::ALL
        .into_iter()
        .filter(|&axis| compared.iter().any(|c| c.step.axis(axis).is_ok()))
        .collect()
}

/// What each flight that cannot fill this axis has to say about it, named so a
/// pilot knows which of the flights on screen the sentence is about.
fn axis_notes(axis: Axis, compared: &[Compared<'_>]) -> Vec<String> {
    compared
        .iter()
        .filter_map(|c| {
            let reason = c.step.axis(axis).err()?;
            Some(match compared.len() {
                1 => explain(axis, reason),
                _ => format!("{}: {}", c.label, explain(axis, reason)),
            })
        })
        .collect()
}

/// The rows of the metrics table, in slot order — a flight that cannot fill
/// this axis has said so above it instead.
fn metrics_rows(axis: Axis, compared: &[Compared<'_>]) -> Vec<MetricsRow> {
    compared
        .iter()
        .filter_map(|c| {
            let response = c.step.axis(axis).ok()?;
            Some(MetricsRow {
                slot: c.slot,
                label: c.label.clone(),
                count: response.count,
                metrics: response.metrics.clone(),
            })
        })
        .collect()
}

/// Keyed on how many flights are being compared rather than on how many filled
/// this axis: with a second flight on screen the surviving row still needs its
/// colour swatch and its name, or the numbers no longer say which curve they
/// belong to.
fn metrics_view(mut rows: Vec<MetricsRow>, compared: usize) -> MetricsView {
    match (compared, rows.len()) {
        (1, 1) => MetricsView::Prose(rows.remove(0)),
        _ => MetricsView::Grid(rows),
    }
}

/// One plot per axis, one mean line per compared flight in its slot colour.
/// Colour is the slot here at every count, including one: a curve must not
/// change colour the moment a second flight is added, and axis identity is
/// already carried by the label above the plot.
fn show_axis(
    ui: &mut Ui,
    palette: &Palette,
    axis: Axis,
    compared: &[Compared<'_>],
    height: f32,
    show_individual: bool,
) {
    show_metrics(
        ui,
        palette,
        axis,
        metrics_view(metrics_rows(axis, compared), compared.len()),
    );

    Plot::new(format!("step_response_plot_{}", axis.name()))
        .height(height)
        .x_axis_label("ms")
        .y_axis_label("normalised")
        .show(ui, |plot_ui| {
            for c in compared {
                let Ok(response) = c.step.axis(axis) else {
                    continue;
                };
                let color = colors::slot_color(palette, c.slot);
                let points = |values: &[f64]| -> PlotPoints {
                    response
                        .time_ms
                        .iter()
                        .zip(values)
                        .map(|(&t, &v)| [t, v])
                        .collect()
                };

                // Every retained trace: the analyser's `max_traces` is the one
                // owner of how dense this band is, so a second cap here cannot
                // thin it out as the stack behind it grows.
                if show_individual {
                    let faded = Color32::from_rgba_unmultiplied(
                        color.r(),
                        color.g(),
                        color.b(),
                        TRACE_ALPHA,
                    );

                    for (i, trace) in response.sample.iter().enumerate() {
                        plot_ui
                            .line(Line::new(format!("response {i}"), points(trace)).color(faded));
                    }
                }

                // Last, so the mean sits on top of the band.
                plot_ui.line(
                    Line::new(c.label.clone(), points(&response.mean))
                        .color(color)
                        .width(MEAN_WIDTH),
                );

                // So the overshoot number and the picture are visibly the same
                // claim. Rise and delay stay numeric — three annotations across
                // three stacked plots is noise.
                let m = &response.metrics;
                plot_ui.points(
                    Points::new(
                        format!("{} peak", c.label),
                        vec![[m.peak_ms, m.peak_normalised()]],
                    )
                    .color(color)
                    .shape(MarkerShape::Diamond)
                    .radius(PEAK_MARKER_RADIUS),
                );
            }
        });
}

/// The line a pilot would say out loud, kept for the single-flight form. Whole
/// percentages and whole milliseconds throughout — the measurement supports no
/// more precision than that.
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

/// The hover under both forms — the numbers mean the same thing either way.
const METRICS_HINT: &str = "Overshoot is how far past the commanded rate the averaged curve went, \
     peak is when it got there and delay how long it took to reach half the commanded rate. The \
     range is the middle half of the individual responses: their peaks land at slightly different \
     times, so averaging flattens them and the curve's own overshoot normally sits below that \
     range. A wide range means the responses behind the curve disagree.";

fn show_metrics(ui: &mut Ui, palette: &Palette, axis: Axis, view: MetricsView) {
    match view {
        MetricsView::Prose(row) => {
            ui.horizontal(|ui| {
                ui.label(format!("mean of {} responses", row.count));
                ui.separator();
                ui.label(metrics_line(&row.metrics, row.count))
                    .on_hover_text(METRICS_HINT);
            });
        }
        MetricsView::Grid(rows) => {
            // Keyed by axis: three stacked grids sharing an id would fight
            // over their column widths.
            egui::Grid::new(format!("step_response_metrics_{}", axis.name()))
                .num_columns(6)
                .striped(true)
                .show(ui, |ui| {
                    for header in [
                        "flight",
                        "overshoot",
                        "peak",
                        "delay",
                        "spread",
                        "responses",
                    ] {
                        ui.label(RichText::new(header).weak());
                    }
                    ui.end_row();

                    for row in &rows {
                        // The swatch is what ties a row to its curve; the chips
                        // above are the legend for both.
                        ui.horizontal(|ui| {
                            ui.colored_label(colors::slot_color(palette, row.slot), "\u{25cf}");
                            ui.label(&row.label);
                        });
                        ui.label(format!("{:.0}%", row.metrics.overshoot_pct));
                        ui.label(format!("{:.0} ms", row.metrics.peak_ms));
                        ui.label(format!("{:.0} ms", row.metrics.delay_ms));
                        ui.label(format!(
                            "{:.0}\u{2013}{:.0}%",
                            row.metrics.spread_pct.start(),
                            row.metrics.spread_pct.end()
                        ));
                        // The thin-stack caveat as its own cell rather than a
                        // trailing clause, so the column still aligns.
                        ui.label(match row.count < THIN_STACK {
                            true => RichText::new(row.count.to_string()).color(palette.warning),
                            false => RichText::new(row.count.to_string()),
                        });
                        ui.end_row();
                    }
                })
                .response
                .on_hover_text(METRICS_HINT);
        }
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

#[cfg(test)]
mod test {
    use std::sync::Arc;

    use super::*;
    use crate::analysis::AxisStepResponse;
    use crate::app::log_store::LogId;
    use crate::parser::{Channel, PerAxis};

    /// A response recognisable by its overshoot, so a grid row can be checked
    /// against the flight it came from.
    fn response(overshoot_pct: f64, count: usize) -> AxisStepResponse {
        AxisStepResponse {
            time_ms: Arc::from(vec![0.0, 1.0]),
            sample: Vec::new(),
            count,
            mean: vec![0.0, 1.0 + overshoot_pct / 100.0],
            metrics: StepMetrics {
                overshoot_pct,
                peak_ms: 1.0,
                delay_ms: 0.5,
                spread_pct: 0.0..=overshoot_pct,
            },
        }
    }

    /// An analysis that filled the named axes and failed on the rest.
    fn analysis(filled: &[Axis], overshoot_pct: f64, count: usize) -> StepResponseAnalysis {
        StepResponseAnalysis::from_axes(PerAxis(Axis::ALL.map(
            |axis| match filled.contains(&axis) {
                true => Ok(response(overshoot_pct, count)),
                false => Err(NoStepResponse::SetpointNotLogged),
            },
        )))
    }

    fn compared<'a>(flights: &'a [(&str, StepResponseAnalysis)]) -> Vec<Compared<'a>> {
        flights
            .iter()
            .enumerate()
            .map(|(slot, (label, step))| Compared {
                slot,
                label: label.to_string(),
                step,
            })
            .collect()
    }

    /// Union, not intersection: one setpoint-less sublog must not blank an axis
    /// for the whole comparison.
    #[test]
    fn an_axis_draws_when_any_compared_flight_can_fill_it() {
        let flights = [
            ("before", analysis(&[Axis::Roll], 12.0, 40)),
            ("after", analysis(&[Axis::Pitch], 8.0, 40)),
        ];

        assert_eq!(
            drawn_axes(&compared(&flights)),
            vec![Axis::Roll, Axis::Pitch]
        );
    }

    /// The flight that could not fill a drawn axis still explains itself, and
    /// says which flight it is talking about now that several are on screen.
    #[test]
    fn a_flight_that_cannot_fill_an_axis_explains_itself_by_name() {
        let flights = [
            ("before", analysis(&[Axis::Roll], 12.0, 40)),
            ("after", analysis(&[Axis::Pitch], 8.0, 40)),
        ];
        let compared = compared(&flights);

        let notes = axis_notes(Axis::Roll, &compared);

        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(notes[0].starts_with("after:"), "{}", notes[0]);
        assert_eq!(axis_notes(Axis::Yaw, &compared).len(), 2);
    }

    /// At one flight the sentence carries no flight name — nothing to
    /// disambiguate, and the prose form is the one that reads well.
    #[test]
    fn one_flight_explains_itself_without_naming_itself() {
        let flights = [("only", analysis(&[], 0.0, 0))];

        let notes = axis_notes(Axis::Roll, &compared(&flights));

        assert_eq!(notes.len(), 1);
        assert!(!notes[0].starts_with("only:"), "{}", notes[0]);
    }

    /// Aligning the numbers vertically is the entire reason to compare them, so
    /// the rows must be in slot order and carry the right flight's numbers.
    #[test]
    fn the_metrics_table_is_in_slot_order_with_each_flights_own_numbers() {
        let flights = [
            ("before", analysis(&[Axis::Roll], 20.0, 40)),
            ("after", analysis(&[Axis::Roll], 6.0, 12)),
        ];

        let rows = metrics_rows(Axis::Roll, &compared(&flights));

        let read: Vec<(usize, &str, f64, usize)> = rows
            .iter()
            .map(|row| {
                (
                    row.slot,
                    row.label.as_str(),
                    row.metrics.overshoot_pct,
                    row.count,
                )
            })
            .collect();
        assert_eq!(read, vec![(0, "before", 20.0, 40), (1, "after", 6.0, 12)]);
    }

    /// A flight with nothing on this axis is not a blank row in the grid — it
    /// has already said why above the plot.
    #[test]
    fn a_flight_without_this_axis_gets_no_row() {
        let flights = [
            ("before", analysis(&[Axis::Roll], 20.0, 40)),
            ("after", analysis(&[Axis::Pitch], 6.0, 40)),
        ];

        assert_eq!(metrics_rows(Axis::Roll, &compared(&flights)).len(), 1);
    }

    #[test]
    fn one_flight_reads_as_prose_and_two_as_a_grid() {
        let one = [("only", analysis(&[Axis::Roll], 20.0, 40))];
        let two = [
            ("before", analysis(&[Axis::Roll], 20.0, 40)),
            ("after", analysis(&[Axis::Roll], 6.0, 40)),
        ];

        let prose = metrics_view(metrics_rows(Axis::Roll, &compared(&one)), 1);
        let grid = metrics_view(metrics_rows(Axis::Roll, &compared(&two)), 2);

        match prose {
            MetricsView::Prose(row) => assert_eq!(
                metrics_line(&row.metrics, row.count),
                "20% overshoot (individual responses 0–20%), peak 1 ms, delay 0 ms"
            ),
            MetricsView::Grid(rows) => panic!("one flight drew a grid of {}", rows.len()),
        }
        match grid {
            MetricsView::Grid(rows) => assert_eq!(rows.len(), 2),
            MetricsView::Prose(_) => panic!("two flights read as one sentence"),
        }
    }

    /// The layout is not unit-testable, but it can be *run*: a headless `Ui`
    /// catches a panicking widget, and two plots or grids fighting over an id.
    #[test]
    fn both_metrics_forms_draw_without_panicking() {
        let flights = [
            ("before", analysis(&[Axis::Roll, Axis::Pitch], 20.0, 40)),
            ("after", analysis(&[Axis::Roll], 6.0, 4)),
        ];
        let compared = compared(&flights);
        let palette = Palette::charcoal();

        for set in [&compared[..1], &compared[..]] {
            egui::__run_test_ui(|ui| {
                for axis in drawn_axes(set) {
                    for note in axis_notes(axis, set) {
                        ui.label(note);
                    }
                    show_axis(ui, &palette, axis, set, 80.0, true);
                }
            });
        }
    }

    /// The row that survived still needs its swatch and its name: its curve is
    /// drawn in a slot colour, and prose carries neither.
    #[test]
    fn a_comparison_stays_a_grid_when_only_one_flight_filled_the_axis() {
        let flights = [
            ("before", analysis(&[Axis::Roll], 20.0, 40)),
            ("after", analysis(&[Axis::Pitch], 6.0, 40)),
        ];
        let compared = compared(&flights);

        let view = metrics_view(metrics_rows(Axis::Roll, &compared), compared.len());

        match view {
            MetricsView::Grid(rows) => assert_eq!(rows.len(), 1),
            MetricsView::Prose(_) => panic!("a comparison read as one flight's sentence"),
        }
    }

    const FS: u64 = 1_000;

    /// Four seconds of parked sticks. Every window is rejected and the analyser
    /// names the threshold it rejected them against — which is what makes the
    /// parameters behind a cache entry observable from outside.
    fn still_log() -> FlightData {
        let n = 4 * FS as usize;
        FlightData::default()
            .with_time((0..n as u64).map(|i| i * (1_000_000 / FS)).collect())
            .with_channel(Channel::Setpoint(Axis::Roll), vec![0.0; n])
            .with_channel(Channel::Gyro(Axis::Roll), vec![0.0; n])
    }

    /// The threshold an entry was analysed under, read back out of it.
    fn threshold(analysis: &StepResponseAnalysis) -> f64 {
        match analysis.axis(Axis::Roll).unwrap_err() {
            NoStepResponse::SticksTooStill { min_setpoint_dps } => min_setpoint_dps,
            other => panic!("expected the stick mask to reject everything, got {other:?}"),
        }
    }

    fn analyzer(min_setpoint_dps: f64) -> StepResponseAnalyzer {
        StepResponseAnalyzer {
            min_setpoint_dps,
            ..Default::default()
        }
    }

    fn keys(count: usize) -> Vec<FlightKey> {
        (0..count as u64).map(|i| (LogId::new(i), 0)).collect()
    }

    /// The defaults path is what a pilot who never opens the knobs is on: the
    /// panel draws each flight's load-time analysis and computes nothing.
    #[test]
    fn at_the_defaults_nothing_is_cached() {
        let (log, keys) = (still_log(), keys(2));
        let flights: Vec<_> = keys.iter().map(|&key| (key, &log)).collect();
        let mut cache = Cache::default();

        cache.refresh(&StepResponseAnalyzer::default(), false, &flights);

        assert!(cache.entries.is_empty());
        assert!(cache.analyzer.is_none());
    }

    /// One slot would thrash with four flights on screen, each evicting the
    /// last every frame.
    #[test]
    fn every_compared_flight_gets_its_own_entry() {
        let (log, keys) = (still_log(), keys(3));
        let flights: Vec<_> = keys.iter().map(|&key| (key, &log)).collect();
        let mut cache = Cache::default();

        cache.refresh(&analyzer(80.0), false, &flights);

        assert_eq!(cache.entries.len(), 3);
        for key in keys {
            assert_eq!(threshold(&cache.entries[&key]), 80.0);
        }
    }

    /// The load-bearing one: a stale entry would draw two flights analysed
    /// under different parameters, which is the mistake shared knobs exist to
    /// prevent.
    #[test]
    fn a_moved_knob_invalidates_every_entry() {
        let (log, keys) = (still_log(), keys(2));
        let flights: Vec<_> = keys.iter().map(|&key| (key, &log)).collect();
        let mut cache = Cache::default();

        cache.refresh(&analyzer(80.0), false, &flights);
        cache.refresh(&analyzer(140.0), false, &flights);

        for key in keys {
            assert_eq!(threshold(&cache.entries[&key]), 140.0);
        }
    }

    #[test]
    fn a_flight_dropped_from_the_set_loses_its_entry() {
        let (log, keys) = (still_log(), keys(2));
        let both: Vec<_> = keys.iter().map(|&key| (key, &log)).collect();
        let mut cache = Cache::default();

        cache.refresh(&analyzer(80.0), false, &both);
        cache.refresh(&analyzer(80.0), false, &both[..1]);

        assert_eq!(cache.entries.len(), 1);
        assert!(cache.entries.contains_key(&keys[0]));
    }

    /// A `DragValue` changes on every mouse-move frame, so a drag keeps drawing
    /// what it has and the stack re-runs once, on release.
    #[test]
    fn a_drag_holds_what_it_has_and_recomputes_on_release() {
        let (log, keys) = (still_log(), keys(2));
        let flights: Vec<_> = keys.iter().map(|&key| (key, &log)).collect();
        let mut cache = Cache::default();

        cache.refresh(&analyzer(80.0), false, &flights);
        cache.refresh(&analyzer(140.0), true, &flights);
        assert_eq!(threshold(&cache.entries[&keys[0]]), 80.0);

        cache.refresh(&analyzer(140.0), false, &flights);
        assert_eq!(threshold(&cache.entries[&keys[0]]), 140.0);
    }

    /// A flight added while a knob is held must not be the one curve on screen
    /// produced by different parameters.
    #[test]
    fn a_flight_added_mid_drag_matches_the_flights_beside_it() {
        let (log, keys) = (still_log(), keys(2));
        let mut cache = Cache::default();

        cache.refresh(&analyzer(80.0), false, &[(keys[0], &log)]);
        cache.refresh(&analyzer(140.0), true, &[(keys[0], &log), (keys[1], &log)]);

        assert_eq!(threshold(&cache.entries[&keys[0]]), 80.0);
        assert_eq!(threshold(&cache.entries[&keys[1]]), 80.0);
    }
}
