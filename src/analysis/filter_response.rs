//! What a filter stage does to a frequency, as the shape it really has.
//!
//! A notch is a V, deep at its centre and recovering either side at a rate its
//! Q sets. A lowpass is a rolloff, not a wall at its corner. Drawn as a line or
//! a band, both read as "everything here is gone", which is the one thing they
//! do not do.
//!
//! The digital responses, not the analogue approximations: these filters run
//! at the gyro loop rate and this is the shape they have there.

use crate::parser::metadata::FilterType;

/// The floor a response is clamped to. A notch's null is unbounded; a plot
/// axis is not, and 40 dB down is already "gone".
pub const MIN_GAIN_DB: f64 = -40.0;

/// Points a curve is drawn from. A notch's null is narrow, so a coarse grid
/// would miss the bottom of the V and understate the cut.
const RESPONSE_POINTS: usize = 512;

/// Betaflight's cutoff corrections, so a PT2 or PT3 keeps its −3 dB point at
/// the configured frequency instead of sagging by the order of the cascade.
/// `1 / sqrt(2^(1/order) - 1)`.
const PT2_CORRECTION: f64 = 1.553_773_974;
const PT3_CORRECTION: f64 = 1.961_459_177;

/// Betaflight's biquad Q, `1 / sqrt(2)` — the Butterworth corner.
const BIQUAD_Q: f64 = std::f64::consts::FRAC_1_SQRT_2;

/// One filter stage, at one setting.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Stage {
    /// A biquad notch, as `centre / q` wide at −3 dB.
    Notch { centre_hz: f64, q: f64 },
    Lowpass {
        cutoff_hz: f64,
        filter_type: FilterType,
    },
}

impl Stage {
    /// |H(f)|², the share of the power at `freq_hz` this stage passes.
    pub fn power_gain(&self, freq_hz: f64, sample_rate_hz: f64) -> f64 {
        match *self {
            Self::Notch { centre_hz, q } => notch_gain(freq_hz, centre_hz, q, sample_rate_hz),
            Self::Lowpass {
                cutoff_hz,
                filter_type,
            } => lowpass_gain(freq_hz, cutoff_hz, filter_type, sample_rate_hz),
        }
    }

    /// The band worth drawing this stage over: a notch's skirts either side of
    /// its centre, and a lowpass from nothing out to where it has rolled off.
    pub fn interesting_band(&self) -> (f64, f64) {
        match *self {
            Self::Notch { centre_hz, q } => {
                let span = (centre_hz / q.max(0.01) * 3.0).max(40.0);
                ((centre_hz - span).max(1.0), centre_hz + span)
            }
            Self::Lowpass { cutoff_hz, .. } => (1.0, cutoff_hz * 8.0),
        }
    }
}

/// A filter's gain against frequency, ready to draw.
#[derive(Debug, Clone, PartialEq)]
pub struct FilterResponse {
    pub freq_hz: Vec<f64>,
    /// Power gain in dB, at or below zero and floored at [`MIN_GAIN_DB`].
    pub gain_db: Vec<f64>,
}

impl FilterResponse {
    /// Where this filter starts taking anything: the first point at or past
    /// −3 dB. A notch's is the near edge of its V, a lowpass's is its corner —
    /// one rule, and the meaningful place to hang a label in both cases.
    pub fn corner(&self) -> Option<(f64, f64)> {
        self.freq_hz
            .iter()
            .zip(&self.gain_db)
            .find(|&(_, &g)| g <= -3.0)
            .map(|(&f, &g)| (f, g))
    }

    /// The frequency this response cuts hardest at, and by how much.
    pub fn deepest(&self) -> Option<(f64, f64)> {
        self.freq_hz
            .iter()
            .zip(&self.gain_db)
            .min_by(|a, b| a.1.total_cmp(b.1))
            .map(|(&f, &g)| (f, g))
    }
}

/// One stage's response across the band it is worth drawing over.
pub fn of(stage: Stage, sample_rate_hz: f64) -> Option<FilterResponse> {
    let (from, to) = stage.interesting_band();
    weighted(&[(stage, 1.0)], from, to, sample_rate_hz)
}

/// The mean response of one stage across every setting it actually ran at,
/// weighted by how long it ran at each.
///
/// Averaged in power rather than in decibels because that is what the noise
/// does: a frequency notched hard for a tenth of the flight and untouched for
/// the rest kept nine tenths of its energy, which averaging the decibels would
/// report as a 10 dB cut it never got.
///
/// Weights are expected to sum to one. A stage that sat still leaves its own
/// curve; one that moved leaves a shallower, wider one, because no single
/// frequency ever got the full cut.
pub fn weighted(
    stages: &[(Stage, f64)],
    from_hz: f64,
    to_hz: f64,
    sample_rate_hz: f64,
) -> Option<FilterResponse> {
    let nyquist = sample_rate_hz / 2.0;
    let (from, to) = (from_hz.max(1.0), to_hz.min(nyquist));
    if stages.is_empty() || sample_rate_hz <= 0.0 || to <= from {
        return None;
    }

    let step = (to - from) / (RESPONSE_POINTS - 1) as f64;
    let (freq_hz, gain_db) = (0..RESPONSE_POINTS)
        .map(|i| {
            let freq = from + i as f64 * step;
            let power: f64 = stages
                .iter()
                .map(|&(stage, weight)| weight * stage.power_gain(freq, sample_rate_hz))
                .sum();

            (freq, (10.0 * power.log10()).clamp(MIN_GAIN_DB, 0.0))
        })
        .unzip();

    Some(FilterResponse { freq_hz, gain_db })
}

/// The RBJ cookbook notch: `b = [1, -2cos w0, 1]`, `a = [1 + α, -2cos w0, 1 - α]`.
fn notch_gain(freq_hz: f64, centre_hz: f64, q: f64, sample_rate_hz: f64) -> f64 {
    use std::f64::consts::TAU;

    let w0 = TAU * centre_hz / sample_rate_hz;
    let alpha = w0.sin() / (2.0 * q);
    let (b1, a0, a2) = (-2.0 * w0.cos(), 1.0 + alpha, 1.0 - alpha);

    let w = TAU * freq_hz / sample_rate_hz;
    let (cos1, sin1, cos2, sin2) = (w.cos(), w.sin(), (2.0 * w).cos(), (2.0 * w).sin());

    let num = (1.0 + b1 * cos1 + cos2).powi(2) + (b1 * sin1 + sin2).powi(2);
    let den = (a0 + b1 * cos1 + a2 * cos2).powi(2) + (b1 * sin1 + a2 * sin2).powi(2);

    match den > 0.0 {
        true => num / den,
        false => 1.0,
    }
}

/// PT1 is one pole; PT2 and PT3 are two and three of them in cascade, at a
/// cutoff Betaflight scales up so the corner stays where it was configured.
/// Biquad is the RBJ lowpass at the Butterworth Q.
fn lowpass_gain(freq_hz: f64, cutoff_hz: f64, filter_type: FilterType, sample_rate_hz: f64) -> f64 {
    if cutoff_hz <= 0.0 {
        return 1.0;
    }
    let (correction, poles) = match filter_type {
        FilterType::Pt1 => (1.0, 1),
        FilterType::Pt2 => (PT2_CORRECTION, 2),
        FilterType::Pt3 => (PT3_CORRECTION, 3),
        FilterType::Biquad => return biquad_lowpass_gain(freq_hz, cutoff_hz, sample_rate_hz),
    };

    pt1_gain(freq_hz, cutoff_hz * correction, sample_rate_hz).powi(poles)
}

/// The one-pole Betaflight actually runs: `y += k (x - y)`, so
/// `H(z) = k / (1 - (1-k) z⁻¹)`.
fn pt1_gain(freq_hz: f64, cutoff_hz: f64, sample_rate_hz: f64) -> f64 {
    use std::f64::consts::TAU;

    let dt = 1.0 / sample_rate_hz;
    let rc = 1.0 / (TAU * cutoff_hz);
    let k = dt / (rc + dt);
    let pole = 1.0 - k;

    let w = TAU * freq_hz / sample_rate_hz;
    k * k / (1.0 - 2.0 * pole * w.cos() + pole * pole)
}

/// The RBJ cookbook lowpass at `Q = 1/sqrt(2)`.
fn biquad_lowpass_gain(freq_hz: f64, cutoff_hz: f64, sample_rate_hz: f64) -> f64 {
    use std::f64::consts::TAU;

    let w0 = TAU * cutoff_hz / sample_rate_hz;
    let (cos0, alpha) = (w0.cos(), w0.sin() / (2.0 * BIQUAD_Q));
    let (b0, b1) = ((1.0 - cos0) / 2.0, 1.0 - cos0);
    let (a0, a1, a2) = (1.0 + alpha, -2.0 * cos0, 1.0 - alpha);

    let w = TAU * freq_hz / sample_rate_hz;
    let (cos1, sin1, cos2, sin2) = (w.cos(), w.sin(), (2.0 * w).cos(), (2.0 * w).sin());

    let num = (b0 + b1 * cos1 + b0 * cos2).powi(2) + (b1 * sin1 + b0 * sin2).powi(2);
    let den = (a0 + a1 * cos1 + a2 * cos2).powi(2) + (a1 * sin1 + a2 * sin2).powi(2);

    match den > 0.0 {
        true => num / den,
        false => 1.0,
    }
}

#[cfg(test)]
mod test {
    use super::*;

    const FS: f64 = 8000.0;

    fn db(gain: f64) -> f64 {
        10.0 * gain.log10()
    }

    /// A lowpass passes what is below it and takes what is above, and the
    /// corner is the −3 dB point.
    ///
    /// Only about −3 dB: these are the discrete filters Betaflight runs, and a
    /// digital one-pole sits a little below its nominal corner — further as
    /// the cutoff climbs toward the loop rate, and multiplied by the order of
    /// the cascade. That is the firmware's behaviour, not an approximation of
    /// it, so the curve drawn keeps it.
    #[test]
    fn every_lowpass_type_is_about_three_db_down_at_its_cutoff() {
        for filter_type in [
            FilterType::Pt1,
            FilterType::Pt2,
            FilterType::Pt3,
            FilterType::Biquad,
        ] {
            let at = |f| db(lowpass_gain(f, 100.0, filter_type, FS));

            assert!(
                (at(100.0) + 3.0).abs() < 0.5,
                "{filter_type:?} is {:.2} dB down at its cutoff",
                at(100.0)
            );
            assert!(at(10.0) > -0.5, "{filter_type:?} cuts the passband");
            assert!(at(1000.0) < at(100.0), "{filter_type:?} does not roll off");
        }
    }

    /// The order is the rolloff: past the corner a PT3 falls three times as
    /// fast in dB as a PT1.
    #[test]
    fn a_higher_order_lowpass_falls_away_faster() {
        let decade = |t| db(lowpass_gain(1000.0, 100.0, t, FS));

        assert!(decade(FilterType::Pt2) < decade(FilterType::Pt1) - 10.0);
        assert!(decade(FilterType::Pt3) < decade(FilterType::Pt2) - 10.0);
    }

    /// A notch takes everything at its centre and nothing far from it.
    #[test]
    fn a_notch_nulls_its_centre_and_leaves_the_rest() {
        let at = |f| db(notch_gain(f, 200.0, 4.0, FS));

        assert!(at(200.0) < -40.0, "the null is only {:.1} dB", at(200.0));
        assert!(at(50.0) > -0.5);
        assert!(at(800.0) > -0.5);
    }

    /// `centre / q` is the −3 dB width, which is what the band drawn before
    /// this claimed to be and the curve now has to actually be.
    #[test]
    fn a_notch_is_three_db_down_at_the_edges_of_its_bandwidth() {
        let (centre, q) = (200.0, 4.0);
        let half = centre / q / 2.0;

        for edge in [centre - half, centre + half] {
            let gain = db(notch_gain(edge, centre, q, FS));
            assert!((gain + 3.0).abs() < 0.6, "{gain:.2} dB at {edge} Hz");
        }
    }

    #[test]
    fn a_higher_q_notch_is_narrower() {
        let quarter_off = |q| db(notch_gain(250.0, 200.0, q, FS));

        assert!(quarter_off(10.0) > quarter_off(2.0));
    }

    /// The averaging that makes a swept filter read as swept: the same time
    /// spread across many settings cuts every frequency less hard than sitting
    /// on one.
    #[test]
    fn spreading_a_stage_over_its_range_shallows_the_cut() {
        let notch = |centre| Stage::Notch {
            centre_hz: centre,
            q: 4.0,
        };
        let still = weighted(&[(notch(300.0), 1.0)], 100.0, 600.0, FS).unwrap();
        let swept: Vec<(Stage, f64)> = (0..5)
            .map(|i| (notch(200.0 + 50.0 * i as f64), 0.2))
            .collect();
        let swept = weighted(&swept, 100.0, 600.0, FS).unwrap();

        assert!(swept.deepest().unwrap().1 > still.deepest().unwrap().1 + 10.0);
    }

    /// Nothing is ever amplified, and nothing falls through the floor a plot
    /// axis can draw.
    #[test]
    fn a_response_stays_between_the_floor_and_no_cut_at_all() {
        let response = of(
            Stage::Notch {
                centre_hz: 200.0,
                q: 4.0,
            },
            FS,
        )
        .unwrap();

        assert_eq!(response.freq_hz.len(), response.gain_db.len());
        assert!(
            response
                .gain_db
                .iter()
                .all(|&g| (MIN_GAIN_DB..=0.0).contains(&g))
        );
    }

    /// The label anchor: a lowpass starts taking at its cutoff, a notch at the
    /// near edge of its bandwidth — not at the far end of the drawn grid.
    #[test]
    fn the_corner_is_where_the_filter_starts_taking_something() {
        let lowpass = of(
            Stage::Lowpass {
                cutoff_hz: 100.0,
                filter_type: FilterType::Pt1,
            },
            FS,
        )
        .unwrap();
        let (corner, _) = lowpass.corner().expect("a lowpass has a corner");
        assert!((corner - 100.0).abs() < 10.0, "corner at {corner:.0} Hz");

        let notch = of(
            Stage::Notch {
                centre_hz: 200.0,
                q: 4.0,
            },
            FS,
        )
        .unwrap();
        let (edge, _) = notch.corner().expect("a notch has a near edge");
        assert!((edge - 175.0).abs() < 10.0, "near edge at {edge:.0} Hz");
        assert!(notch.deepest().unwrap().0 > edge);
    }

    /// Nothing above Nyquist exists to draw, and a stage with no settings has
    /// no curve rather than an empty one.
    #[test]
    fn a_curve_that_cannot_be_drawn_is_absent() {
        let stage = Stage::Lowpass {
            cutoff_hz: 500.0,
            filter_type: FilterType::Pt1,
        };

        assert!(weighted(&[(stage, 1.0)], 1.0, 100.0, 0.0).is_none());
        assert!(weighted(&[], 1.0, 100.0, FS).is_none());

        let response = of(stage, FS).unwrap();
        assert!(response.freq_hz.iter().all(|&f| f <= FS / 2.0));
    }
}
