//! Builds the per-panel user message: the computed metrics as plain
//! sentences, not raw timeseries. Mirrors `buildStepResponseMessage` /
//! `buildPsdMessage` from the prototype HTML, now reading the real analysis
//! types instead of a JS mock.

use crate::analysis::{SpectralAnalysis, StepResponseAnalysis};
use crate::parser::Axis;

/// Current PID gains as Betaflight logs them (`"P,I,D"`), one string per
/// axis. `None` when the header didn't carry that key — an older firmware,
/// or a log with the field stripped.
pub struct PidGains<'a> {
    pub roll: Option<&'a str>,
    pub pitch: Option<&'a str>,
    pub yaw: Option<&'a str>,
}

pub fn step_response_message(step: &StepResponseAnalysis, pid: &PidGains<'_>) -> String {
    let mut out =
        String::from("Step response analysis (per axis, measured on the averaged trace):\n\n");

    for axis in Axis::ALL {
        match step.axis(axis) {
            Ok(r) => {
                let m = &r.metrics;
                out += &format!(
                    "{}: overshoot {:.0}%, peak at {:.0} ms, delay to 50% crossing {:.0} ms, \
                     spread (IQR of per-trace peak) {:.0}-{:.0}%, from {} surviving windows.\n",
                    axis.name(),
                    m.overshoot_pct,
                    m.peak_ms,
                    m.delay_ms,
                    m.spread_pct.start(),
                    m.spread_pct.end(),
                    r.count
                );
            }
            Err(reason) => out += &format!("{}: no step response ({reason:?}).\n", axis.name()),
        }
    }

    out += "\nCurrent PID values (P,I,D as Betaflight logs them):\n";
    for (name, gains) in [("Roll", pid.roll), ("Pitch", pid.pitch), ("Yaw", pid.yaw)] {
        out += &format!("{name}: {}\n", gains.unwrap_or("not available in this log"));
    }
    out
}

pub fn psd_message(spectral: &SpectralAnalysis) -> String {
    let mut out = String::from("Noise spectrum analysis (per axis, pre-filter gyro):\n\n");

    for axis in Axis::ALL {
        match spectral.axis(axis) {
            Some(s) => {
                let peaks = s
                    .peaks
                    .iter()
                    .map(|p| format!("{:.0} Hz @ {:.0} dB", p.freq_hz, p.amplitude_db))
                    .collect::<Vec<_>>()
                    .join("; ");
                out += &format!(
                    "{}: noise floor {:.0} dB. Peaks: {}.\n",
                    axis.name(),
                    s.noise_floor_db,
                    if peaks.is_empty() {
                        "none reported".to_string()
                    } else {
                        peaks
                    }
                );
            }
            None => out += &format!("{}: no pre-filter gyro available.\n", axis.name()),
        }
    }

    out += "\nCurrent filter settings:\n";
    if spectral.filter_markers.is_empty() {
        out += "none reported.\n";
    }
    for marker in &spectral.filter_markers {
        out += &format!(
            "{}: center {:.0} Hz{}\n",
            marker.label,
            marker.center_hz,
            marker
                .cutoff_hz
                .map(|c| format!(", cutoff {c:.0} Hz"))
                .unwrap_or_default()
        );
    }
    out
}
