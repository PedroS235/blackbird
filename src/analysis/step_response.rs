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
#[derive(Debug, Clone)]
pub struct StepResponseAnalyzer {
    /// Long enough to hold the low frequencies, short enough that the tune is
    /// constant across it.
    pub window_s: f64,
    /// 87% overlap at the default window — a dense stack for the all-traces view.
    pub hop_s: f64,
    /// Regularisation, relative to the setpoint's own mean power.
    pub lambda_k: f64,
    /// Windows where the sticks barely moved deconvolve to noise.
    pub min_setpoint_dps: f64,
    /// Covers rise, overshoot and settle at FPV timescales.
    pub response_ms: f64,
    /// The stretch of the response averaged to find the steady state each
    /// trace is normalised against.
    pub tail_ms: f64,
    /// A trace whose steady state is this close to zero carries no usable
    /// gain — dividing by it would blow the curve up instead of scaling it.
    pub min_steady_state: f64,
}

impl Default for StepResponseAnalyzer {
    fn default() -> Self {
        Self {
            window_s: 2.0,
            hop_s: 0.25,
            lambda_k: 0.01,
            min_setpoint_dps: 20.0,
            response_ms: 500.0,
            tail_ms: 100.0,
            min_steady_state: 1e-3,
        }
    }
}

/// One axis' worth of step responses: every surviving window's trace, plus
/// their pointwise mean. The spread across traces is itself diagnostic, so
/// both are kept.
#[derive(Debug, Clone)]
pub struct AxisStepResponse {
    /// Shared across traces — they are all the same length on the same grid.
    pub time_ms: Arc<[f64]>,
    pub traces: Vec<Vec<f64>>,
    pub mean: Vec<f64>,
}

/// Why an axis has no step response. Each cause reads differently to a pilot —
/// a field they never enabled, a flight that never asked the craft a question,
/// a craft that never answered — so the analyser names the cause rather than
/// leaving the panel to guess it back out of the flight data.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NoStepResponse {
    SetpointNotLogged,
    GyroNotLogged,
    /// Fewer samples than one window, or no usable sample rate.
    LogTooShort,
    /// No window cleared the stick mask, whose threshold this carries.
    SticksTooStill {
        min_setpoint_dps: f64,
    },
    /// The sticks moved but every response settled at ~0 — nothing to
    /// normalise against, so nothing that can be plotted.
    NoSteadyState,
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
        self.axes[axis].as_ref().map_err(|&reason| reason)
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
    deconvolver: WienerDeconvolver,
}

impl StepResponseAnalyzer {
    pub fn analyze(&self, fd: &FlightData) -> StepResponseAnalysis {
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

        let traces: Vec<Vec<f64>> = (0..=last_start)
            .step_by(plan.hop)
            .filter_map(|start| {
                let sp = &setpoint[start..start + plan.window];
                if sp.iter().fold(0.0, |m: f64, v| m.max(v.abs())) < self.min_setpoint_dps {
                    return None;
                }
                sticks_moved = true;

                // The step response is the cumulative sum of the impulse
                // response, truncated to the region the panel draws.
                let mut step: Vec<f64> = plan
                    .deconvolver
                    .impulse_response(sp, &gyro[start..start + plan.window])
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
                if steady.abs() < self.min_steady_state {
                    return None;
                }
                step.iter_mut().for_each(|v| *v /= steady);
                Some(step)
            })
            .collect();

        if traces.is_empty() {
            return Err(match sticks_moved {
                true => NoStepResponse::NoSteadyState,
                false => NoStepResponse::SticksTooStill {
                    min_setpoint_dps: self.min_setpoint_dps,
                },
            });
        }

        let mean = (0..plan.len)
            .map(|i| traces.iter().map(|t| t[i]).sum::<f64>() / traces.len() as f64)
            .collect();

        Ok(AxisStepResponse {
            time_ms: (0..plan.len).map(|i| i as f64 * 1e3 / plan.fs).collect(),
            traces,
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
        assert!(roll.traces.len() > 10, "{} traces", roll.traces.len());
        assert_eq!(roll.mean.len(), roll.time_ms.len());
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
                min_setpoint_dps: 20.0
            }
        );
    }

    #[test]
    fn axes_without_setpoint_or_gyro_say_which_field_is_missing() {
        let setpoint = stick_input(24_000, 200.0);
        let gyro = second_order(&setpoint, 20.0, 0.4);
        let log = log_with(setpoint, gyro).with_channel(Channel::Setpoint(Axis::Pitch), vec![0.0]);

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

    /// A gyro that never responded leaves a steady state of ~0 to divide by —
    /// which is a different story from sticks that never moved.
    #[test]
    fn a_dead_gyro_is_reported_as_such_not_as_infinities() {
        let setpoint = stick_input(24_000, 200.0);
        let analysis =
            StepResponseAnalyzer::default().analyze(&log_with(setpoint, vec![0.0; 24_000]));

        assert_eq!(
            analysis.axis(Axis::Roll).unwrap_err(),
            NoStepResponse::NoSteadyState
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
}
