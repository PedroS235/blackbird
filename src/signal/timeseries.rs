/// Downsample `samples` to at most `bucket_count * 2` points using min-max decimation.
///
/// Each bucket emits two points — the min and max — preserving visual peaks and troughs
/// that a plain average would erase. Time is returned as seconds relative to `t0`.
pub fn minmax_downsample(
    time_us: &[u64],
    samples: &[f64],
    t0: u64,
    bucket_count: usize,
) -> Vec<[f64; 2]> {
    let to_s = |t: u64| t.saturating_sub(t0) as f64 / 1_000_000.0;

    if samples.is_empty() {
        return Vec::new();
    }

    if samples.len() <= bucket_count * 2 {
        return time_us
            .iter()
            .zip(samples)
            .map(|(&t, &s)| [to_s(t), s])
            .collect();
    }

    let bucket_size = samples.len().div_ceil(bucket_count);
    let mut out = Vec::with_capacity(bucket_count * 2);

    for (time_chunk, sample_chunk) in time_us.chunks(bucket_size).zip(samples.chunks(bucket_size)) {
        let pairs = || time_chunk.iter().zip(sample_chunk);
        let (&min_t, &min_v) = pairs().min_by(|(_, a), (_, b)| a.total_cmp(b)).unwrap();
        let (&max_t, &max_v) = pairs().max_by(|(_, a), (_, b)| a.total_cmp(b)).unwrap();

        if min_t <= max_t {
            out.push([to_s(min_t), min_v]);
            out.push([to_s(max_t), max_v]);
        } else {
            out.push([to_s(max_t), max_v]);
            out.push([to_s(min_t), min_v]);
        }
    }

    out
}

/// Smooth `samples` with a centred moving average of `window` samples.
///
/// Returns `None` if `window <= 1` or shorter than the input. Output length equals input length;
/// edge samples use a narrower window rather than padding.
pub fn moving_average(samples: &[f64], window: usize) -> Option<Vec<f64>> {
    if window <= 1 || samples.len() < window {
        return None;
    }
    let n = samples.len();
    let half = window / 2;
    Some(
        (0..n)
            .map(|i| {
                let start = i.saturating_sub(half);
                let end = (i + (window - half)).min(n);
                samples[start..end].iter().sum::<f64>() / (end - start) as f64
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_minmax_downsample() {
        let t = [0, 5, 10, 15, 20, 25, 30, 35];
        let samples = [1.0, 2.0, 3.0, 5.0, 3.0, 4.0, 5.5, 5.0];
        let expected = vec![[0.0, 1.0], [1.5e-5, 5.0], [2e-5, 3.0], [3e-5, 5.5]];
        let downsampled = minmax_downsample(&t, &samples, 0, 2);
        assert_eq!(expected, downsampled);
    }

    #[test]
    fn test_minmax_downsample_empty_input_returns_empty() {
        let downsampled = minmax_downsample(&[], &[], 0, 4);
        assert_eq!(downsampled, Vec::<[f64; 2]>::new());
    }

    #[test]
    fn test_minmax_downsample_passthrough_when_within_2x_buckets() {
        // len(3) <= bucket_count(2) * 2 -> every sample returned as-is, no decimation.
        let t = [0, 10, 20];
        let samples = [1.0, 2.0, 3.0];
        let expected = vec![[0.0, 1.0], [1e-5, 2.0], [2e-5, 3.0]];
        let downsampled = minmax_downsample(&t, &samples, 0, 2);
        assert_eq!(downsampled, expected);
    }

    #[test]
    fn test_minmax_downsample_passthrough_exact_boundary() {
        // len(4) == bucket_count(2) * 2 -> boundary still counts as passthrough.
        let t = [0, 10, 20, 30];
        let samples = [1.0, 2.0, 3.0, 4.0];
        let expected = vec![[0.0, 1.0], [1e-5, 2.0], [2e-5, 3.0], [3e-5, 4.0]];
        let downsampled = minmax_downsample(&t, &samples, 0, 2);
        assert_eq!(downsampled, expected);
    }

    #[test]
    fn test_minmax_downsample_single_sample_passthrough() {
        let downsampled = minmax_downsample(&[0], &[1.0], 0, 1);
        assert_eq!(downsampled, vec![[0.0, 1.0]]);
    }

    #[test]
    fn test_minmax_downsample_uneven_bucket_sizes() {
        // 8 samples over 3 buckets -> sizes 3, 3, 2 (last bucket short).
        let t = [0, 5, 10, 15, 20, 25, 30, 35];
        let samples = [1.0, 2.0, 3.0, 5.0, 3.0, 4.0, 5.5, 5.0];
        let expected = vec![
            [0.0, 1.0],
            [1e-5, 3.0],
            [1.5e-5, 5.0],
            [2e-5, 3.0],
            [3e-5, 5.5],
            [3.5e-5, 5.0],
        ];
        let downsampled = minmax_downsample(&t, &samples, 0, 3);
        assert_eq!(downsampled, expected);
    }

    #[test]
    fn test_minmax_downsample_orders_points_chronologically_within_bucket() {
        // Single bucket where the max occurs before the min in time; output must
        // still be emitted oldest-first so the plotted line doesn't run backward.
        let t = [0, 5, 10, 15];
        let samples = [1.0, 4.0, 2.0, 0.0];
        let downsampled = minmax_downsample(&t, &samples, 0, 1);
        assert_eq!(downsampled, vec![[5e-6, 4.0], [1.5e-5, 0.0]]);
    }

    #[test]
    fn test_minmax_downsample_relative_to_t0() {
        let t = [100, 105, 110, 115, 120, 125, 130, 135];
        let samples = [1.0, 2.0, 3.0, 5.0, 3.0, 4.0, 5.5, 5.0];
        let expected = vec![[0.0, 1.0], [1.5e-5, 5.0], [2e-5, 3.0], [3e-5, 5.5]];
        let downsampled = minmax_downsample(&t, &samples, 100, 2);
        assert_eq!(downsampled, expected);
    }

    #[test]
    fn test_minmax_downsample_t_before_t0_saturates_to_zero() {
        // Timestamps earlier than t0 must not underflow the u64 subtraction.
        let t = [0, 5];
        let samples = [1.0, 2.0];
        let downsampled = minmax_downsample(&t, &samples, 50, 1);
        assert_eq!(downsampled, vec![[0.0, 1.0], [0.0, 2.0]]);
    }

    #[test]
    #[should_panic]
    fn test_minmax_downsample_zero_buckets_panics() {
        // bucket_count=0 forces div_ceil(len, 0) once decimation kicks in.
        let t = [0, 5, 10, 15, 20];
        let samples = [1.0, 2.0, 3.0, 4.0, 5.0];
        let _ = minmax_downsample(&t, &samples, 0, 0);
    }

    #[test]
    fn test_moving_average() {
        let samples = [1.0, 2.0, 3.0, 4.0, 5.0];
        let expected = vec![1.5, 2.0, 3.0, 4.0, 4.5];
        let avg = moving_average(&samples, 3).unwrap();
        assert_eq!(avg, expected);
    }

    #[test]
    fn test_moving_average_window_zero_returns_none() {
        let samples = [1.0, 2.0, 3.0];
        assert_eq!(moving_average(&samples, 0), None);
    }

    #[test]
    fn test_moving_average_window_one_returns_none() {
        let samples = [1.0, 2.0, 3.0];
        assert_eq!(moving_average(&samples, 1), None);
    }

    #[test]
    fn test_moving_average_window_larger_than_input_returns_none() {
        let samples = [1.0, 2.0, 3.0];
        assert_eq!(moving_average(&samples, 4), None);
    }

    #[test]
    fn test_moving_average_empty_input_returns_none() {
        let samples: [f64; 0] = [];
        assert_eq!(moving_average(&samples, 2), None);
    }

    #[test]
    fn test_moving_average_window_equals_input_len() {
        let samples = [1.0, 2.0, 3.0];
        let avg = moving_average(&samples, 3).unwrap();
        assert_eq!(avg, vec![1.5, 2.0, 2.5]);
    }

    #[test]
    fn test_moving_average_even_window() {
        // window=2, half=1: each point averages [i-1, i+1) clipped to bounds.
        let samples = [1.0, 2.0, 3.0, 4.0];
        let avg = moving_average(&samples, 2).unwrap();
        assert_eq!(avg, vec![1.0, 1.5, 2.5, 3.5]);
    }

    #[test]
    fn test_moving_average_constant_input() {
        let samples = [5.0; 6];
        let avg = moving_average(&samples, 3).unwrap();
        assert_eq!(avg, vec![5.0; 6]);
    }

    #[test]
    fn test_moving_average_preserves_length() {
        let samples = [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        let avg = moving_average(&samples, 4).unwrap();
        assert_eq!(avg.len(), samples.len());
    }

    #[test]
    fn test_moving_average_single_sample_window_one_still_none() {
        // len == window == 1, but window <= 1 short-circuits before length check.
        let samples = [42.0];
        assert_eq!(moving_average(&samples, 1), None);
    }

    #[test]
    fn test_moving_average_negative_values() {
        let samples = [-3.0, -1.0, 1.0, 3.0, 5.0];
        let avg = moving_average(&samples, 3).unwrap();
        assert_eq!(avg, vec![-2.0, -1.0, 1.0, 3.0, 4.0]);
    }
}
