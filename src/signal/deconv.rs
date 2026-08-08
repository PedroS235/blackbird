use std::sync::Arc;

use rustfft::{Fft, FftPlanner, num_complex::Complex};

/// Wiener deconvolution: recovers the impulse response `h` of the system that
/// turned `input` into `output`, given `output ≈ h * input`.
///
/// `H = (Y · conj(X)) / (|X|² + λ)`. The λ term is what makes this Wiener
/// rather than a plain spectral division — at frequencies the input never
/// excited, `|X|²` is ~0 and the division would amplify noise without bound.
///
/// Plan once per window length, reuse across every window of a log.
pub struct WienerDeconvolver {
    len: usize,
    /// FFT length, at least `2 * len` — see `new`.
    padded: usize,
    lambda_k: f64,
    forward: Arc<dyn Fft<f64>>,
    inverse: Arc<dyn Fft<f64>>,
}

impl WienerDeconvolver {
    /// `lambda_k` scales the regularisation relative to the input's own mean
    /// power, so it behaves the same on a gentle cruise and on a hard flip.
    pub fn new(len: usize, lambda_k: f64) -> Self {
        // Zero-padding to twice the window makes the FFT's circular
        // convolution behave like a linear one. Unpadded, the tail of the
        // window wraps onto the head and corrupts the first milliseconds of
        // the response — exactly the part being measured.
        let padded = (2 * len).next_power_of_two();
        let mut planner = FftPlanner::new();

        Self {
            len,
            padded,
            lambda_k,
            forward: planner.plan_fft_forward(padded),
            inverse: planner.plan_fft_inverse(padded),
        }
    }

    /// The first `len` samples of the recovered impulse response. `input` and
    /// `output` are truncated to `len`; anything shorter is zero-padded, which
    /// is a caller bug but not worth a panic.
    ///
    /// Never returns `NaN`: an input with no energy anywhere yields zeros, so
    /// one dead window cannot poison an average downstream.
    pub fn impulse_response(&self, input: &[f64], output: &[f64]) -> Vec<f64> {
        let mut h = self.spectrum(input);
        let y = self.spectrum(output);

        let lambda =
            self.lambda_k * h.iter().map(Complex::norm_sqr).sum::<f64>() / self.padded as f64;

        for (h, y) in h.iter_mut().zip(y) {
            let denominator = (h.norm_sqr() + lambda).max(f64::MIN_POSITIVE);
            *h = y * h.conj() / denominator;
        }

        self.inverse.process(&mut h);
        // rustfft is unnormalised — an unscaled forward/inverse round trip
        // multiplies by the transform length.
        h.iter()
            .take(self.len)
            .map(|c| c.re / self.padded as f64)
            .collect()
    }

    fn spectrum(&self, signal: &[f64]) -> Vec<Complex<f64>> {
        let mut buffer = vec![Complex { re: 0.0, im: 0.0 }; self.padded];
        for (slot, &s) in buffer.iter_mut().zip(signal.iter().take(self.len)) {
            slot.re = s;
        }
        self.forward.process(&mut buffer);
        buffer
    }
}

#[cfg(test)]
mod test {
    use super::*;

    const LEN: usize = 512;

    /// Deterministic broadband excitation — deconvolution needs energy at every
    /// frequency it is asked to recover.
    fn noise(n: usize) -> Vec<f64> {
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        (0..n)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state >> 11) as f64 / (1u64 << 53) as f64 - 0.5
            })
            .collect()
    }

    fn convolve(input: &[f64], kernel: &[f64]) -> Vec<f64> {
        (0..input.len())
            .map(|i| {
                kernel
                    .iter()
                    .enumerate()
                    .filter(|&(k, _)| k <= i)
                    .map(|(k, &h)| h * input[i - k])
                    .sum()
            })
            .collect()
    }

    /// Single-pole lowpass `y[i] = a·y[i-1] + (1 - a)·x[i]`, whose step
    /// response is exactly `1 - a^(t+1)`.
    fn first_order_lag(input: &[f64], pole: f64) -> Vec<f64> {
        let mut y = 0.0;
        input
            .iter()
            .map(|&x| {
                y = pole * y + (1.0 - pole) * x;
                y
            })
            .collect()
    }

    fn cumsum(values: &[f64]) -> Vec<f64> {
        values
            .iter()
            .scan(0.0, |acc, &v| {
                *acc += v;
                Some(*acc)
            })
            .collect()
    }

    #[test]
    fn recovers_a_known_fir_kernel() {
        let kernel = [0.5, 0.3, 0.15, 0.05];
        let input = noise(LEN);
        let output = convolve(&input, &kernel);

        let h = WienerDeconvolver::new(LEN, 1e-6).impulse_response(&input, &output);

        for (i, &expected) in kernel.iter().enumerate() {
            assert!(
                (h[i] - expected).abs() < 0.01,
                "tap {i}: expected {expected}, got {}",
                h[i]
            );
        }
        assert!(
            h[kernel.len()..].iter().all(|v| v.abs() < 0.01),
            "nothing beyond the kernel's length"
        );
    }

    /// The step response is the cumulative sum of the impulse response, and for
    /// a single-pole lag that is exactly `1 - a^(t+1)`.
    #[test]
    fn cumsum_of_the_response_matches_a_first_order_lag() {
        let pole = 0.95;
        // The lag's tail outlives the excitation, so the input goes quiet
        // early and the whole response fits inside the window. A real window
        // is a slice of continuous flight and does not have that luxury —
        // that truncation is part of what λ absorbs.
        let mut input = noise(LEN);
        input[LEN - 200..].fill(0.0);
        let output = first_order_lag(&input, pole);

        let step = cumsum(&WienerDeconvolver::new(LEN, 1e-6).impulse_response(&input, &output));

        for t in [10usize, 20, 40, 80] {
            let expected = 1.0 - pole.powi(t as i32 + 1);
            assert!(
                (step[t] - expected).abs() < 0.005,
                "t={t}: expected {expected:.4}, got {:.4}",
                step[t]
            );
        }
    }

    /// A window where the sticks never moved has no energy to divide by. The
    /// caller masks these out, but the primitive still must not emit NaN.
    #[test]
    fn dead_input_yields_zeros_not_nan() {
        let h = WienerDeconvolver::new(LEN, 0.01).impulse_response(&vec![0.0; LEN], &noise(LEN));

        assert!(h.iter().all(|v| v.is_finite()), "no NaN or inf");
        assert!(h.iter().all(|&v| v == 0.0), "no signal in, no signal out");
    }

    /// Regularisation biases the estimate towards zero — that is the trade it
    /// makes for not amplifying noise, and it should be visible.
    #[test]
    fn larger_lambda_attenuates_the_response() {
        let kernel = [0.5, 0.3, 0.15, 0.05];
        let input = noise(LEN);
        let output = convolve(&input, &kernel);

        let gentle = WienerDeconvolver::new(LEN, 1e-6).impulse_response(&input, &output);
        let heavy = WienerDeconvolver::new(LEN, 10.0).impulse_response(&input, &output);

        assert!(
            heavy[0].abs() < gentle[0].abs(),
            "expected attenuation: gentle {:.4}, heavy {:.4}",
            gentle[0],
            heavy[0]
        );
    }

    #[test]
    fn short_inputs_are_zero_padded_rather_than_panicking() {
        let h = WienerDeconvolver::new(LEN, 0.01).impulse_response(&noise(8), &noise(8));
        assert_eq!(h.len(), LEN);
        assert!(h.iter().all(|v| v.is_finite()));
    }
}
