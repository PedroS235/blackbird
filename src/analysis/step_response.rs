use std::ops::RangeInclusive;
use std::sync::Arc;

use crate::parser::{Axis, FlightData, PerAxis};
use crate::signal::deconv::WienerDeconvolver;

/// Recovers the craft's step response by deconvolving gyro from setpoint over
/// every overlapping window of the log, the way PIDToolbox and Blackbox
/// Explorer do. Every part of the flight where the sticks moved contributes —
/// unlike step *detection*, which finds almost nothing in a freestyle log.
///
/// Thresholds are fields so a call site can loosen them (a cinematic log moves
/// the sticks gently) instead of them being constants buried here.
#[derive(Debug, Clone, PartialEq)]
pub struct StepResponseAnalyzer {
    /// Long enough to hold the low frequencies, short enough that the tune is
    /// constant across it. PID-Analyzer's `framelen`.
    pub window_s: f64,
    /// Window/16, PID-Analyzer's `superpos` — a dense stack for the
    /// all-traces view. Absolute rather than a fraction of the window, so the
    /// two knobs stay independent.
    pub hop_s: f64,
    /// Regularisation, relative to the setpoint's own mean power.
    pub lambda_k: f64,
    /// Windows where the sticks barely moved deconvolve to noise.
    pub min_setpoint_dps: f64,
    /// Cut off each end of the log. The craft is on the ground there, or in a
    /// hand, and it answers the sticks like something else entirely.
    pub trim_s: f64,
    /// Covers rise, overshoot and settle at FPV timescales.
    pub response_ms: f64,
    /// The stretch of the response averaged to find the steady state each
    /// trace is normalised against.
    pub tail_ms: f64,
    /// A craft that answered its sticks settles somewhere near its commanded
    /// rate. A trace settling outside this band did not come from one: near
    /// zero it carries no gain to normalise by, high it is a deconvolution
    /// blow-up, negative it is upside down and would subtract from the mean.
    pub steady_state_band: RangeInclusive<f64>,
    /// How many traces are kept for the band the panel draws. A field rather
    /// than a knob: past a hundred traces the band is pixel-for-pixel the same
    /// and this changes only memory, never the answer — the mean is always
    /// accumulated over every window.
    pub max_traces: usize,
}

impl Default for StepResponseAnalyzer {
    fn default() -> Self {
        Self {
            window_s: 1.0,
            hop_s: 1.0 / 16.0,
            lambda_k: 0.01,
            min_setpoint_dps: 52.0,
            trim_s: super::DEFAULT_TRIM_S,
            response_ms: 500.0,
            tail_ms: 100.0,
            steady_state_band: 0.5..=3.0,
            max_traces: 200,
        }
    }
}

/// The curve read as the numbers a pilot would say out loud. Measured on the
/// mean, so the figure and the drawn picture can never disagree.
///
/// No settling time: a 500 ms window averaged from as few as forty traces
/// cannot support one, and a figure that moves whenever the stick mask moves
/// reads as precision that is not there.
#[derive(Debug, Clone, PartialEq)]
pub struct StepMetrics {
    /// Peak above the steady state, in percent. Traces are normalised to a
    /// steady state of 1, so a peak of 1.18 reads as 18.
    pub overshoot_pct: f64,
    /// When that peak lands — a slow craft from a fast one.
    pub peak_ms: f64,
    /// Time to half the commanded rate: how much of the tune is latency
    /// rather than gain. Every trace is normalised by its own tail, so the
    /// mean always settles at 1 and always crosses half of it.
    pub delay_ms: f64,
    /// Interquartile range of the *individual* responses' overshoot, in the
    /// same percent. Not a confidence interval around `overshoot_pct`: peaks
    /// land at slightly different times, so averaging flattens them and the
    /// mean's peak normally sits below the middle of this range. What it says
    /// is how much the responses behind the curve disagree — a range rather
    /// than a ± because the distribution is not symmetric.
    pub spread_pct: RangeInclusive<f64>,
}

impl StepMetrics {
    /// The peak this reports on the normalised curve, for drawing a marker at
    /// the same place the number came from.
    pub fn peak_normalised(&self) -> f64 {
        1.0 + self.overshoot_pct / 100.0
    }

    /// `peak_overshoots` is every surviving window's peak, not just the
    /// retained sample's — the spread describes the flight, not the buffer.
    /// Both it and `mean` are searched over the rise only: past `tail` the
    /// curve is the steady state it was normalised against, where a slow drift
    /// would otherwise be reported as an overshoot half a second in.
    fn measure(time_ms: &[f64], mean: &[f64], tail: usize, peak_overshoots: &mut [f64]) -> Self {
        let rise = &mean[..rise_len(mean.len(), tail)];
        let (peak_i, peak) = rise
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.total_cmp(b))
            .expect("the rise is never empty");
        let delay_i = mean
            .iter()
            .position(|&v| v >= 0.5)
            .unwrap_or(mean.len() - 1);

        peak_overshoots.sort_by(f64::total_cmp);
        let quantile = |f: f64| {
            let last = peak_overshoots.len().saturating_sub(1) as f64;
            as_pct(peak_overshoots[(last * f).round() as usize])
        };

        Self {
            overshoot_pct: as_pct(*peak),
            peak_ms: time_ms[peak_i],
            delay_ms: time_ms[delay_i],
            spread_pct: quantile(0.25)..=quantile(0.75),
        }
    }
}

/// How much of a response is rise rather than the steady-state tail — at least
/// one sample, however the window and response length were set.
fn rise_len(len: usize, tail: usize) -> usize {
    len.saturating_sub(tail).max(1)
}

/// A normalised peak as overshoot over the steady state, in percent.
fn as_pct(peak: f64) -> f64 {
    (peak - 1.0) * 100.0
}

/// One axis' worth of step responses: the mean over every surviving window, a
/// bounded sample of the individual traces behind it, and the numbers read off
/// that mean. The spread across traces is itself diagnostic, so the sample is
/// kept as well as the average.
#[derive(Debug, Clone)]
pub struct AxisStepResponse {
    /// Shared across traces — they are all the same length on the same grid.
    pub time_ms: Arc<[f64]>,
    /// A bounded, evenly spread subset of the traces, for the drawn band.
    /// Never the thing to count or average: see `count` and `mean`.
    pub sample: Vec<Vec<f64>>,
    /// How many windows actually survived — how much flight the mean is built
    /// from, which is not `sample.len()`.
    pub count: usize,
    pub mean: Vec<f64>,
    pub metrics: StepMetrics,
}

/// A bounded, evenly spread sample of the trace stack.
///
/// Traces are admitted on a stride; when the buffer fills it is decimated in
/// place by dropping every other entry and the stride doubles. Decimating as
/// it fills is the point — collecting everything and thinning afterwards would
/// leave the peak allocation, which is what costs ~57 MB per 1 kHz sublog and
/// ~459 MB at 8 kHz, untouched. Deterministic, so the same log draws the same
/// band every run.
struct TraceSample {
    cap: usize,
    stride: usize,
    seen: usize,
    traces: Vec<Vec<f64>>,
}

impl TraceSample {
    fn new(cap: usize) -> Self {
        Self {
            cap,
            stride: 1,
            seen: 0,
            traces: Vec::new(),
        }
    }

    /// Takes the trace by value: one not admitted is dropped rather than
    /// copied, which is the allocation this exists to avoid.
    fn push(&mut self, trace: Vec<f64>) {
        if self.cap > 0 && self.seen.is_multiple_of(self.stride) {
            self.traces.push(trace);

            if self.traces.len() >= self.cap {
                let mut i = 0;
                self.traces.retain(|_| {
                    i += 1;
                    i % 2 == 1
                });
                self.stride *= 2;
            }
        }
        self.seen += 1;
    }
}

/// Why an axis has no step response. Each cause reads differently to a pilot —
/// a field they never enabled, a flight that never asked the craft a question,
/// a craft that never answered — so the analyser names the cause rather than
/// leaving the panel to guess it back out of the flight data.
#[derive(Debug, Clone, PartialEq)]
pub enum NoStepResponse {
    SetpointNotLogged,
    GyroNotLogged,
    /// Fewer samples than one window, or no usable sample rate.
    LogTooShort,
    /// No window cleared the stick mask, whose threshold this carries.
    SticksTooStill {
        min_setpoint_dps: f64,
    },
    /// The sticks moved but no response settled inside the acceptance band,
    /// which this carries so the panel can say what it was.
    NoSteadyState {
        band: RangeInclusive<f64>,
    },
}

#[derive(Debug, Clone)]
pub struct StepResponseAnalysis {
    axes: PerAxis<Result<AxisStepResponse, NoStepResponse>>,
}

impl Default for StepResponseAnalysis {
    fn default() -> Self {
        Self {
            axes: PerAxis(Axis::ALL.map(|_| Err(NoStepResponse::SetpointNotLogged))),
        }
    }
}

impl StepResponseAnalysis {
    pub fn axis(&self, axis: Axis) -> Result<&AxisStepResponse, NoStepResponse> {
        self.axes[axis].as_ref().map_err(Clone::clone)
    }
}

/// The parts of a run that depend only on the sample rate. Built once per log
/// so the FFT is planned once, not once per axis.
struct Plan {
    fs: f64,
    window: usize,
    hop: usize,
    /// Samples of impulse response kept — the region the panel draws.
    len: usize,
    /// Samples averaged at the end of that region to find the steady state.
    tail: usize,
    /// Hann taper, one per window sample. A window is a slice out of
    /// continuous flight: handed to the FFT as a rectangle, both of its edges
    /// are step discontinuities the craft never flew, and their leakage lands
    /// in the recovered impulse response as a spike at `t = 0` that the
    /// cumulative sum turns into a head start. PIDToolbox and PID-Analyzer
    /// both taper first; λ does not absorb this.
    hann: Vec<f64>,
    deconvolver: WienerDeconvolver,
}

/// Symmetric Hann taper, matching `np.hanning` — zero at both ends, so the
/// segment's edges carry no discontinuity into the transform.
fn hann(len: usize) -> Vec<f64> {
    let last = len.saturating_sub(1) as f64;
    (0..len)
        .map(|i| match last {
            0.0 => 1.0,
            last => 0.5 * (1.0 - (std::f64::consts::TAU * i as f64 / last).cos()),
        })
        .collect()
}

impl StepResponseAnalyzer {
    pub fn analyze(&self, fd: &FlightData) -> StepResponseAnalysis {
        let fd = fd.trimmed(self.trim_s);
        let plan = self.plan(fd.sample_rate_hz());

        StepResponseAnalysis {
            axes: PerAxis(Axis::ALL.map(|axis| {
                let setpoint = fd.setpoint(axis).ok_or(NoStepResponse::SetpointNotLogged)?;
                let gyro = fd.gyro(axis).ok_or(NoStepResponse::GyroNotLogged)?;
                let plan = plan.as_ref().ok_or(NoStepResponse::LogTooShort)?;

                self.analyze_axis(plan, setpoint, gyro)
            })),
        }
    }

    /// `None` when the log carries no usable sample rate, which leaves every
    /// window length undefined.
    fn plan(&self, fs: f64) -> Option<Plan> {
        if fs <= 0.0 {
            return None;
        }
        let samples = |seconds: f64| (seconds * fs).round() as usize;
        let window = samples(self.window_s).max(1);
        let len = samples(self.response_ms / 1e3).clamp(1, window);

        Some(Plan {
            fs,
            hop: samples(self.hop_s).max(1),
            tail: samples(self.tail_ms / 1e3).clamp(1, len),
            hann: hann(window),
            deconvolver: WienerDeconvolver::new(window, self.lambda_k),
            window,
            len,
        })
    }

    fn analyze_axis(
        &self,
        plan: &Plan,
        setpoint: &[f64],
        gyro: &[f64],
    ) -> Result<AxisStepResponse, NoStepResponse> {
        let last_start = setpoint
            .len()
            .min(gyro.len())
            .checked_sub(plan.window)
            .ok_or(NoStepResponse::LogTooShort)?;

        // Whether anything got past the stick mask decides which empty state
        // the panel shows: a still flight reads nothing like a dead gyro.
        let mut sticks_moved = false;

        // Scratch for the tapered signals, reused across every window — a long
        // log at 8 kHz is thousands of windows per axis and these would
        // otherwise be thousands of window-sized allocations.
        let (mut sp_w, mut gyro_w) = (vec![0.0; plan.window], vec![0.0; plan.window]);

        // The mean is accumulated over every surviving window, never over the
        // retained sample — capping what is kept must not change the answer.
        let mut sum = vec![0.0; plan.len];
        let mut peaks: Vec<f64> = Vec::new();
        let mut sample = TraceSample::new(self.max_traces);

        for start in (0..=last_start).step_by(plan.hop) {
            let sp = &setpoint[start..start + plan.window];
            if sp.iter().fold(0.0, |m: f64, v| m.max(v.abs())) < self.min_setpoint_dps {
                continue;
            }
            sticks_moved = true;

            // Tapered before the transform, both signals the same way, so
            // the recovered response is of the craft and not of the cut.
            let taper = |out: &mut [f64], signal: &[f64]| {
                for ((o, v), w) in out.iter_mut().zip(signal).zip(&plan.hann) {
                    *o = v * w;
                }
            };
            taper(&mut sp_w, sp);
            taper(&mut gyro_w, &gyro[start..start + plan.window]);

            // The step response is the cumulative sum of the impulse
            // response, truncated to the region the panel draws.
            let mut step: Vec<f64> = plan
                .deconvolver
                .impulse_response(&sp_w, &gyro_w)
                .iter()
                .scan(0.0, |acc, &h| {
                    *acc += h;
                    Some(*acc)
                })
                .take(plan.len)
                .collect();

            // Per-trace normalisation: without it a single drifting trace
            // shifts the mean and overshoot stops meaning anything.
            let steady = step[plan.len - plan.tail..].iter().sum::<f64>() / plan.tail as f64;
            if !self.steady_state_band.contains(&steady) {
                continue;
            }
            step.iter_mut().for_each(|v| *v /= steady);

            sum.iter_mut().zip(&step).for_each(|(s, v)| *s += v);
            peaks.push(
                step[..rise_len(plan.len, plan.tail)]
                    .iter()
                    .copied()
                    .fold(f64::NEG_INFINITY, f64::max),
            );
            sample.push(step);
        }

        let count = peaks.len();
        if count == 0 {
            return Err(match sticks_moved {
                true => NoStepResponse::NoSteadyState {
                    band: self.steady_state_band.clone(),
                },
                false => NoStepResponse::SticksTooStill {
                    min_setpoint_dps: self.min_setpoint_dps,
                },
            });
        }

        let mean: Vec<f64> = sum.iter().map(|s| s / count as f64).collect();
        let time_ms: Arc<[f64]> = (0..plan.len).map(|i| i as f64 * 1e3 / plan.fs).collect();

        Ok(AxisStepResponse {
            metrics: StepMetrics::measure(&time_ms, &mean, plan.tail, &mut peaks),
            time_ms,
            sample: sample.traces,
            count,
            mean,
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::parser::Channel;

    const FS: f64 = 2000.0;

    /// Deterministic stick input: broadband, so the deconvolution has energy at
    /// every frequency it is asked to recover, but smoothed — a pilot's thumb
    /// has no content at half the loop rate, and white noise up there biases
    /// the recovered response high.
    fn stick_input(n: usize, amplitude: f64) -> Vec<f64> {
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        let mut smoothed = 0.0;
        let raw: Vec<f64> = (0..n)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                smoothed =
                    0.9 * smoothed + 0.1 * ((state >> 11) as f64 / (1u64 << 53) as f64 - 0.5);
                smoothed
            })
            .collect();

        let scale = amplitude / raw.iter().fold(f64::MIN_POSITIVE, |m, v| m.max(v.abs()));
        raw.iter().map(|v| v * scale).collect()
    }

    /// `ẍ = ωₙ²(u − x) − 2ζωₙẋ`, integrated at the sample rate. Unity DC gain,
    /// so its step response settles at 1 and overshoots by `exp(−πζ/√(1−ζ²))`.
    fn second_order(input: &[f64], freq_hz: f64, damping: f64) -> Vec<f64> {
        let wn = std::f64::consts::TAU * freq_hz;
        let dt = 1.0 / FS;
        let (mut x, mut v) = (0.0, 0.0);
        input
            .iter()
            .map(|&u| {
                let a = wn * wn * (u - x) - 2.0 * damping * wn * v;
                v += a * dt;
                x += v * dt;
                x
            })
            .collect()
    }

    /// When a curve first reaches `level`, in ms — the whole length if never.
    fn crossing_ms(values: &[f64], level: f64) -> f64 {
        values
            .iter()
            .position(|&v| v >= level)
            .unwrap_or(values.len()) as f64
            * 1e3
            / FS
    }

    fn log_with(setpoint: Vec<f64>, gyro: Vec<f64>) -> FlightData {
        let dt_us = (1e6 / FS) as u64;
        FlightData::default()
            .with_time(
                (0..setpoint.len() as u64)
                    .map(|i| 7_000_000 + i * dt_us)
                    .collect(),
            )
            .with_channel(Channel::Setpoint(Axis::Roll), setpoint)
            .with_channel(Channel::Gyro(Axis::Roll), gyro)
    }

    /// A log's ends are the craft on the ground and the hand launch, which
    /// answer the sticks like a different craft. Cutting them off has to bring
    /// the recovered overshoot back to the airborne system's.
    #[test]
    fn the_ends_of_the_log_do_not_reach_the_curve() {
        let (freq_hz, damping) = (20.0, 0.4);
        let setpoint = stick_input(32_000, 200.0);
        let mut gyro = second_order(&setpoint, freq_hz, damping);

        // Two seconds each end of a wallowing, overshoot-free craft.
        let grounded = second_order(&setpoint, 5.0, 1.0);
        let ends = 2 * FS as usize;
        gyro[..ends].copy_from_slice(&grounded[..ends]);
        let tail = gyro.len() - ends;
        gyro[tail..].copy_from_slice(&grounded[tail..]);

        let log = log_with(setpoint, gyro);
        let overshoot = |trim_s| {
            StepResponseAnalyzer {
                trim_s,
                ..Default::default()
            }
            .analyze(&log)
            .axis(Axis::Roll)
            .expect("roll analysed")
            .metrics
            .overshoot_pct
        };

        let root = (1.0f64 - damping * damping).sqrt();
        let expected = 100.0 * (-std::f64::consts::PI * damping / root).exp();
        let (trimmed, whole) = (overshoot(2.0), overshoot(0.0));

        assert!(
            (trimmed - expected).abs() < (whole - expected).abs(),
            "trimmed {trimmed:.0}% is no closer to {expected:.0}% than untrimmed {whole:.0}%"
        );
        assert!(
            (trimmed - expected).abs() < 5.0,
            "expected {expected:.0}% overshoot, got {trimmed:.0}%"
        );
    }

    /// The whole point: push a setpoint through a system whose overshoot is
    /// known analytically and get that overshoot back out of the mean curve.
    #[test]
    fn recovers_a_known_second_order_overshoot() {
        let (freq_hz, damping) = (20.0, 0.4);
        let setpoint = stick_input(24_000, 200.0);
        let gyro = second_order(&setpoint, freq_hz, damping);

        let roll = StepResponseAnalyzer::default()
            .analyze(&log_with(setpoint, gyro))
            .axis(Axis::Roll)
            .cloned()
            .expect("roll analysed");

        let peak = roll.mean.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let expected =
            1.0 + (-std::f64::consts::PI * damping / (1.0 - damping * damping).sqrt()).exp();

        assert!(
            (peak - expected).abs() < 0.05,
            "expected overshoot to {expected:.3}, got {peak:.3}"
        );
        assert!(roll.count > 10, "{} responses", roll.count);
        assert_eq!(roll.mean.len(), roll.time_ms.len());
    }

    /// The same claim as above, now made against the number the panel prints
    /// and the prompt will read rather than a hand-scan of the curve.
    #[test]
    fn the_reported_metrics_match_the_systems_analytic_response() {
        let (freq_hz, damping) = (20.0, 0.4);
        let setpoint = stick_input(24_000, 200.0);
        let gyro = second_order(&setpoint, freq_hz, damping);

        let roll = StepResponseAnalyzer::default()
            .analyze(&log_with(setpoint, gyro))
            .axis(Axis::Roll)
            .cloned()
            .expect("roll analysed");

        // `exp(−πζ/√(1−ζ²))` over the steady state, and the peak of a
        // second-order system lands at `π/(ωₙ√(1−ζ²))`.
        let root = (1.0f64 - damping * damping).sqrt();
        let expected_pct = 100.0 * (-std::f64::consts::PI * damping / root).exp();
        let expected_peak_ms = 1e3 / (2.0 * freq_hz * root);

        let m = &roll.metrics;
        assert!(
            (m.overshoot_pct - expected_pct).abs() < 5.0,
            "expected {expected_pct:.0}% overshoot, got {:.0}%",
            m.overshoot_pct
        );
        assert!(
            (m.peak_ms - expected_peak_ms).abs() < 10.0,
            "expected a peak at {expected_peak_ms:.0} ms, got {:.0} ms",
            m.peak_ms
        );
        assert!(
            m.delay_ms > 0.0 && m.delay_ms < m.peak_ms,
            "half the commanded rate is reached on the way up, got {:.0} ms",
            m.delay_ms
        );
        // One system driven throughout: the individual responses agree, and
        // the analytic overshoot sits inside the middle half of them.
        assert!(
            m.spread_pct.contains(&expected_pct),
            "expected {expected_pct:.0}% inside {:.0}–{:.0}%",
            m.spread_pct.start(),
            m.spread_pct.end()
        );
    }

    /// A craft that never overshoots must not have the drift in its settled
    /// tail reported as one, with the peak marker parked at the far edge of
    /// the plot. The peak is a feature of the rise or it is not a peak.
    #[test]
    fn an_overdamped_craft_does_not_report_its_tail_as_an_overshoot() {
        let setpoint = stick_input(24_000, 200.0);
        let gyro = second_order(&setpoint, 5.0, 1.0);

        let roll = StepResponseAnalyzer::default()
            .analyze(&log_with(setpoint, gyro))
            .axis(Axis::Roll)
            .cloned()
            .expect("roll analysed");

        let d = StepResponseAnalyzer::default();
        assert!(
            roll.metrics.peak_ms <= d.response_ms - d.tail_ms,
            "peak reported at {:.0} ms, inside the steady-state tail",
            roll.metrics.peak_ms
        );
    }

    /// The load-bearing one: the mean is accumulated over every surviving
    /// window, so what the sample retains cannot move it. This is what would
    /// rot silently if someone later averaged the sample instead.
    #[test]
    fn capping_what_is_retained_does_not_change_the_answer() {
        let setpoint = stick_input(24_000, 200.0);
        let gyro = second_order(&setpoint, 20.0, 0.4);
        let log = log_with(setpoint, gyro);

        let analysed = |max_traces| {
            StepResponseAnalyzer {
                max_traces,
                ..Default::default()
            }
            .analyze(&log)
            .axis(Axis::Roll)
            .cloned()
            .expect("roll analysed")
        };

        let (tight, huge) = (analysed(4), analysed(usize::MAX));

        assert_eq!(tight.mean, huge.mean);
        assert_eq!(tight.count, huge.count);
        assert_eq!(tight.metrics, huge.metrics);
    }

    /// What the panel prints is how much flight the curve came from, not how
    /// much of it was kept in memory.
    #[test]
    fn the_sample_stays_within_the_cap_while_the_count_stays_truthful() {
        let setpoint = stick_input(24_000, 200.0);
        let gyro = second_order(&setpoint, 20.0, 0.4);

        let roll = StepResponseAnalyzer {
            max_traces: 8,
            ..Default::default()
        }
        .analyze(&log_with(setpoint, gyro))
        .axis(Axis::Roll)
        .cloned()
        .expect("roll analysed");

        assert!(roll.sample.len() <= 8, "{} retained", roll.sample.len());
        assert!(
            roll.count > roll.sample.len(),
            "{} windows survived but {} were retained",
            roll.count,
            roll.sample.len()
        );
    }

    /// A short flight still gets its numbers — the panel's caveat has to have
    /// something to caveat, and "not much data" must read differently from
    /// "nothing was computed".
    #[test]
    fn a_thin_stack_still_reports_metrics() {
        let setpoint = stick_input(2_600, 200.0);
        let gyro = second_order(&setpoint, 20.0, 0.4);

        let roll = StepResponseAnalyzer::default()
            .analyze(&log_with(setpoint, gyro))
            .axis(Axis::Roll)
            .cloned()
            .expect("roll analysed");

        assert!(roll.count < 10, "{} responses", roll.count);
        assert!(roll.metrics.overshoot_pct.is_finite());
        assert!(roll.metrics.peak_ms > 0.0);
    }

    /// A hover with the sticks parked is not a step response, and the reason
    /// has to survive to the panel: this is not the same as an unlogged axis.
    #[test]
    fn sticks_that_never_move_name_the_threshold_that_rejected_them() {
        let setpoint = vec![0.5; 12_000];
        let gyro = second_order(&setpoint, 20.0, 0.4);

        let analysis = StepResponseAnalyzer::default().analyze(&log_with(setpoint, gyro));

        assert_eq!(
            analysis.axis(Axis::Roll).unwrap_err(),
            NoStepResponse::SticksTooStill {
                min_setpoint_dps: 52.0
            }
        );
    }

    /// A quad is not already a quarter of the way to its commanded rate the
    /// instant the stick moves. A curve that leaves the origin is the
    /// signature of untapered window edges leaking into `t = 0`.
    #[test]
    fn the_response_starts_near_zero_rather_than_leaping_off_the_origin() {
        let setpoint = stick_input(24_000, 200.0);
        let gyro = second_order(&setpoint, 20.0, 0.4);

        let roll = StepResponseAnalyzer::default()
            .analyze(&log_with(setpoint, gyro))
            .axis(Axis::Roll)
            .cloned()
            .expect("roll analysed");

        assert!(
            roll.mean[0].abs() < 0.05,
            "expected the curve to start at rest, got {:.3}",
            roll.mean[0]
        );
    }

    /// The same artefact read as a tune: a craft whose rise takes tens of
    /// milliseconds must not be drawn as if it got there sooner, because that
    /// is the reading a pilot uses to decide P against D.
    #[test]
    fn a_known_rise_is_not_reported_as_arriving_sooner_than_it_did() {
        let (freq_hz, damping) = (20.0, 0.4);
        let setpoint = stick_input(24_000, 200.0);
        let gyro = second_order(&setpoint, freq_hz, damping);

        let roll = StepResponseAnalyzer::default()
            .analyze(&log_with(setpoint, gyro))
            .axis(Axis::Roll)
            .cloned()
            .expect("roll analysed");

        // Ground truth: drive the same system with an actual step.
        let truth = crossing_ms(&second_order(&vec![1.0; 2_000], freq_hz, damping), 0.5);
        let measured = crossing_ms(&roll.mean, 0.5);

        assert!(
            measured > 0.7 * truth,
            "system reaches half in {truth:.1} ms, curve claims {measured:.1} ms"
        );
    }

    /// The band is what keeps a deconvolution that went nowhere plausible out
    /// of the mean: pushed somewhere this system cannot land, nothing survives.
    #[test]
    fn a_trace_settling_outside_the_band_does_not_reach_the_mean() {
        let setpoint = stick_input(24_000, 200.0);
        let gyro = second_order(&setpoint, 20.0, 0.4);
        let band = 5.0..=10.0;

        let analysis = StepResponseAnalyzer {
            steady_state_band: band.clone(),
            ..Default::default()
        }
        .analyze(&log_with(setpoint, gyro));

        assert_eq!(
            analysis.axis(Axis::Roll).unwrap_err(),
            NoStepResponse::NoSteadyState { band }
        );
    }

    /// A gyro answering the sticks backwards settles negative. Normalising by
    /// that steady state would flip the trace upright and average it in as if
    /// the craft had responded correctly.
    #[test]
    fn an_inverted_response_never_appears_the_right_way_up() {
        let setpoint = stick_input(24_000, 200.0);
        let gyro: Vec<f64> = second_order(&setpoint, 20.0, 0.4)
            .iter()
            .map(|v| -v)
            .collect();

        let analysis = StepResponseAnalyzer::default().analyze(&log_with(setpoint, gyro));

        assert_eq!(
            analysis.axis(Axis::Roll).unwrap_err(),
            NoStepResponse::NoSteadyState {
                band: StepResponseAnalyzer::default().steady_state_band
            }
        );
    }

    #[test]
    fn axes_without_setpoint_or_gyro_say_which_field_is_missing() {
        let setpoint = stick_input(24_000, 200.0);
        let gyro = second_order(&setpoint, 20.0, 0.4);
        // Long enough to survive trimming — this is "gyro missing", not
        // "setpoint too short to reach past the trimmed ends".
        let n = setpoint.len();
        let log =
            log_with(setpoint, gyro).with_channel(Channel::Setpoint(Axis::Pitch), vec![0.0; n]);

        let analysis = StepResponseAnalyzer::default().analyze(&log);

        assert_eq!(
            analysis.axis(Axis::Pitch).unwrap_err(),
            NoStepResponse::GyroNotLogged
        );
        assert_eq!(
            analysis.axis(Axis::Yaw).unwrap_err(),
            NoStepResponse::SetpointNotLogged
        );
    }

    #[test]
    fn a_log_shorter_than_one_window_says_so_rather_than_blaming_the_pilot() {
        let setpoint = stick_input(500, 200.0);
        let gyro = second_order(&setpoint, 20.0, 0.4);

        let analysis = StepResponseAnalyzer::default().analyze(&log_with(setpoint, gyro));

        assert_eq!(
            analysis.axis(Axis::Roll).unwrap_err(),
            NoStepResponse::LogTooShort
        );
    }

    /// A gyro that never responded leaves a steady state of ~0, well below the
    /// band — a different story from sticks that never moved.
    #[test]
    fn a_dead_gyro_is_reported_as_such_not_as_infinities() {
        let setpoint = stick_input(24_000, 200.0);
        let analysis =
            StepResponseAnalyzer::default().analyze(&log_with(setpoint, vec![0.0; 24_000]));

        assert_eq!(
            analysis.axis(Axis::Roll).unwrap_err(),
            NoStepResponse::NoSteadyState {
                band: StepResponseAnalyzer::default().steady_state_band
            }
        );
    }

    /// The mask is a threshold, not a fixed rule — a gentle cinematic log is
    /// analysable by lowering it.
    #[test]
    fn the_stick_mask_is_tunable() {
        let setpoint = stick_input(24_000, 5.0);
        let gyro = second_order(&setpoint, 20.0, 0.4);
        let log = log_with(setpoint, gyro);

        assert!(
            StepResponseAnalyzer::default()
                .analyze(&log)
                .axis(Axis::Roll)
                .is_err()
        );
        assert!(
            StepResponseAnalyzer {
                min_setpoint_dps: 1.0,
                ..Default::default()
            }
            .analyze(&log)
            .axis(Axis::Roll)
            .is_ok()
        );
    }

    /// So is the band: a craft whose response settles well above it is
    /// rejected by default and recovered by widening the band.
    #[test]
    fn the_steady_state_band_is_tunable() {
        let setpoint = stick_input(24_000, 200.0);
        let gyro: Vec<f64> = second_order(&setpoint, 20.0, 0.4)
            .iter()
            .map(|v| v * 5.0)
            .collect();
        let log = log_with(setpoint, gyro);

        assert!(
            StepResponseAnalyzer::default()
                .analyze(&log)
                .axis(Axis::Roll)
                .is_err()
        );
        assert!(
            StepResponseAnalyzer {
                steady_state_band: 0.5..=10.0,
                ..Default::default()
            }
            .analyze(&log)
            .axis(Axis::Roll)
            .is_ok()
        );
    }
}
