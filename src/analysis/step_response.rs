const SLOPE_HALF_MS: f64 = 20.0;
const SLOPE_THRESHOLD: f64 = 20.0;
const MIN_STEP: f64 = 20.0;

const PRE_MS: f64 = 50.0;
const POST_MS: f64 = 500.0;
const NORM_MS: f64 = 100.0;
const STABILITY_THRESHOLD: f64 = 30.0;
const SUSTAIN_MS: f64 = 150.0;
const SUSTAIN_TOL: f64 = 0.25;

const SETTLING_BAND: f64 = 0.05;
const SETTLING_MIN_SAMPLES: usize = 20;

#[derive(Debug, Clone)]
pub struct StepResponseResult {
    pub curve: Vec<f32>,
    pub positive_curve: Vec<f32>,
    pub negative_curve: Vec<f32>,
    pub overshoot_pct: f32,
    /// NaN if never settles within the window.
    pub settling_time_ms: f32,
    /// NaN if rise cannot be measured.
    pub rise_time_ms: f32,
    pub step_count: usize,
    pub positive_count: usize,
    pub negative_count: usize,
    pub pre_samples: usize,
    pub sample_rate_hz: f32,
}

impl Default for StepResponseResult {
    fn default() -> Self {
        Self {
            curve: Vec::new(),
            positive_curve: Vec::new(),
            negative_curve: Vec::new(),
            overshoot_pct: 0.0,
            settling_time_ms: f32::NAN,
            rise_time_ms: f32::NAN,
            step_count: 0,
            positive_count: 0,
            negative_count: 0,
            pre_samples: 0,
            sample_rate_hz: 1000.0,
        }
    }
}

pub fn compute_step_response(
    setpoint: &[f64],
    gyro: &[f64],
    throttle: Option<&[f64]>,
    throttle_min: f64,
    throttle_max: f64,
    sample_rate_hz: f32,
) -> StepResponseResult {
    let n = setpoint.len().min(gyro.len());
    let half = (sample_rate_hz as f64 * SLOPE_HALF_MS / 1000.0).round() as usize;
    let pre = (sample_rate_hz as f64 * PRE_MS / 1000.0).round() as usize;
    let post = (sample_rate_hz as f64 * POST_MS / 1000.0).round() as usize;
    let norm_samp = (sample_rate_hz as f64 * NORM_MS / 1000.0).round() as usize;
    let sustain_samp = (sample_rate_hz as f64 * SUSTAIN_MS / 1000.0).round() as usize;
    let window = pre + post;

    if n < window + 2 * half + 1 {
        return StepResponseResult {
            sample_rate_hz,
            pre_samples: pre,
            ..Default::default()
        };
    }

    let norm_throttle: Option<Vec<f64>> = throttle.map(|t| {
        let (tmin, tmax) = t
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(mn, mx), &v| {
                (mn.min(v), mx.max(v))
            });
        let range = (tmax - tmin).max(1e-6);
        t.iter()
            .map(|&v| ((v - tmin) / range).clamp(0.0, 1.0))
            .collect()
    });

    let mut acc_pos = vec![0.0f64; window];
    let mut acc_neg = vec![0.0f64; window];
    let mut cnt_pos = vec![0usize; window];
    let mut cnt_neg = vec![0usize; window];
    let mut count_pos = 0usize;
    let mut count_neg = 0usize;
    let mut last_start: Option<usize> = None;

    for s in half..(n - half) {
        let slope = setpoint[s + half] - setpoint[s - half];
        if slope.abs() < SLOPE_THRESHOLD {
            continue;
        }

        let s_start = s.saturating_sub(half);
        if s_start < pre || s_start + post > n {
            continue;
        }
        if let Some(ls) = last_start {
            if s_start.saturating_sub(ls) < window {
                continue;
            }
        }

        if let Some(ref t) = norm_throttle {
            let tv = t.get(s_start).copied().unwrap_or(0.5);
            if tv < throttle_min || tv > throttle_max {
                continue;
            }
        }

        let sp = &setpoint[s_start - pre..s_start + post];
        let gy = &gyro[s_start - pre..s_start + post];

        let baseline_sp = mean(&sp[..pre]);
        let baseline_gy = mean(&gy[..pre]);

        let (pre_min, pre_max) = sp[..pre]
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(mn, mx), &v| {
                (mn.min(v), mx.max(v))
            });
        if pre_max - pre_min > STABILITY_THRESHOLD {
            continue;
        }

        let norm_end = (pre + norm_samp).min(window);
        let post_sp = &sp[pre..norm_end];
        let flip = slope < 0.0;
        let step_size = if flip {
            baseline_sp - post_sp.iter().copied().fold(f64::INFINITY, f64::min)
        } else {
            post_sp.iter().copied().fold(f64::NEG_INFINITY, f64::max) - baseline_sp
        };

        if step_size < MIN_STEP {
            continue;
        }

        let tgt = if flip {
            baseline_sp - step_size
        } else {
            baseline_sp + step_size
        };
        let hold_tol = step_size * SUSTAIN_TOL;

        let sustain_start = pre + norm_samp;
        let sustain_end = (sustain_start + sustain_samp).min(window);
        if sustain_end > sustain_start
            && !sp[sustain_start..sustain_end]
                .iter()
                .all(|&v| (v - tgt).abs() <= hold_tol)
        {
            continue;
        }

        let eff_end = sp[sustain_start..]
            .iter()
            .position(|&v| (v - tgt).abs() > hold_tol)
            .map(|p| sustain_start + p)
            .unwrap_or(window);

        let mut shifted: Vec<f64> = gy.iter().map(|&v| v - baseline_gy).collect();
        if flip {
            for v in &mut shifted {
                *v = -*v;
            }
        }

        if flip {
            for i in 0..eff_end {
                acc_neg[i] += shifted[i] / step_size;
                cnt_neg[i] += 1;
            }
            count_neg += 1;
        } else {
            for i in 0..eff_end {
                acc_pos[i] += shifted[i] / step_size;
                cnt_pos[i] += 1;
            }
            count_pos += 1;
        }
        last_start = Some(s_start);
    }

    let count = count_pos + count_neg;
    if count == 0 {
        return StepResponseResult {
            sample_rate_hz,
            pre_samples: pre,
            ..Default::default()
        };
    }

    let positive_curve_f64 = per_pos_avg(&acc_pos, &cnt_pos, count_pos);
    let negative_curve_f64 = per_pos_avg(&acc_neg, &cnt_neg, count_neg);

    let combined_acc: Vec<f64> = (0..window).map(|i| acc_pos[i] + acc_neg[i]).collect();
    let combined_cnt: Vec<usize> = (0..window).map(|i| cnt_pos[i] + cnt_neg[i]).collect();
    let curve_f64 = per_pos_avg(&combined_acc, &combined_cnt, count);

    let step_moment = pre + half;

    let overshoot_pct =
        (curve_f64.iter().copied().fold(f64::NEG_INFINITY, f64::max) - 1.0).max(0.0) as f32 * 100.0;
    let settling_time_ms = find_settling_time(&curve_f64, step_moment, sample_rate_hz);
    let rise_time_ms = find_rise_time(&curve_f64, step_moment, sample_rate_hz);

    let to_f32 = |v: Vec<f64>| v.into_iter().map(|x| x as f32).collect::<Vec<f32>>();

    StepResponseResult {
        curve: to_f32(curve_f64),
        positive_curve: to_f32(positive_curve_f64),
        negative_curve: to_f32(negative_curve_f64),
        overshoot_pct,
        settling_time_ms,
        rise_time_ms,
        step_count: count,
        positive_count: count_pos,
        negative_count: count_neg,
        pre_samples: step_moment,
        sample_rate_hz,
    }
}

fn per_pos_avg(acc: &[f64], cnt: &[usize], total_count: usize) -> Vec<f64> {
    if total_count == 0 {
        return Vec::new();
    }
    let mut curve: Vec<f64> = acc
        .iter()
        .zip(cnt.iter())
        .map(|(&a, &c)| if c == 0 { f64::NAN } else { a / c as f64 })
        .collect();
    let mut last = 0.0f64;
    for v in &mut curve {
        if v.is_nan() {
            *v = last;
        } else {
            last = *v;
        }
    }
    curve
}

fn mean(slice: &[f64]) -> f64 {
    if slice.is_empty() {
        return 0.0;
    }
    slice.iter().sum::<f64>() / slice.len() as f64
}

fn find_settling_time(curve: &[f64], pre: usize, sample_rate_hz: f32) -> f32 {
    let peak_idx = curve[pre..]
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
        .map(|(i, _)| i + pre)
        .unwrap_or(pre);

    let n = curve.len();
    let mut run = 0usize;
    for i in peak_idx..n {
        if (curve[i] - 1.0).abs() <= SETTLING_BAND {
            run += 1;
            if run >= SETTLING_MIN_SAMPLES {
                let settled_at = i + 1 - run;
                return (settled_at as f32 - pre as f32) / sample_rate_hz * 1000.0;
            }
        } else {
            run = 0;
        }
    }
    f32::NAN
}

fn find_rise_time(curve: &[f64], step_moment: usize, sample_rate_hz: f32) -> f32 {
    let post = &curve[step_moment..];
    if post.is_empty() {
        return f32::NAN;
    }
    let tail = (post.len() * 9 / 10).max(1);
    let final_val = mean(&post[tail..]);
    if final_val <= 0.0 {
        return f32::NAN;
    }
    match post.iter().position(|&v| v >= 0.9 * final_val) {
        Some(i) => i as f32 / sample_rate_hz * 1000.0,
        None => f32::NAN,
    }
}
