use egui_plot::HoverPosition;

/// Hover readouts report every series' y at the pointer's x, not the pointer's
/// own y. `egui_plot`'s default snaps to a vertex within a few pixels of the
/// cursor, and a spectrum or a downsampled trace rarely has one there — so the
/// pilot gets the coordinate they hovered instead of the value on the line.
pub fn hover_x(pos: &HoverPosition<'_>) -> f64 {
    match pos {
        HoverPosition::NearDataPoint { position, .. } | HoverPosition::Elsewhere { position } => {
            position.x
        }
    }
}

/// y at `x`, linearly interpolated. `xs` ascends; `None` outside its range.
pub fn y_at(xs: &[f64], ys: &[f64], x: f64) -> Option<f64> {
    let n = xs.len().min(ys.len());
    if n < 2 || x < xs[0] || x > xs[n - 1] {
        return None;
    }
    let i = xs[..n].partition_point(|&v| v <= x).clamp(1, n - 1);
    let (x0, x1) = (xs[i - 1], xs[i]);
    let t = if x1 > x0 { (x - x0) / (x1 - x0) } else { 0.0 };
    Some(ys[i - 1] + t * (ys[i] - ys[i - 1]))
}

/// The same, where x is a timestamp in µs and the plot's x is seconds from `t0`.
pub fn y_at_us(time_us: &[u64], ys: &[f64], t0: u64, x_s: f64) -> Option<f64> {
    let n = time_us.len().min(ys.len());
    let target = t0 as f64 + x_s * 1e6;
    if n < 2 || target < time_us[0] as f64 || target > time_us[n - 1] as f64 {
        return None;
    }
    let i = time_us[..n]
        .partition_point(|&v| (v as f64) <= target)
        .clamp(1, n - 1);
    let (x0, x1) = (time_us[i - 1] as f64, time_us[i] as f64);
    let t = if x1 > x0 { (target - x0) / (x1 - x0) } else { 0.0 };
    Some(ys[i - 1] + t * (ys[i] - ys[i - 1]))
}

/// A `Plot::label_formatter` listing `label: y` per series, one line each.
pub fn readout<'a>(
    x_unit: &'a str,
    decimals: usize,
    series: Vec<(String, &'a [f64], &'a [f64])>,
) -> impl Fn(&HoverPosition<'_>) -> Option<String> + 'a {
    move |pos| {
        let x = hover_x(pos);
        let mut out = format!("{x:.1} {x_unit}");
        for (label, xs, ys) in &series {
            if let Some(y) = y_at(xs, ys, x) {
                out += &format!("\n{label}: {y:.decimals$}");
            }
        }
        Some(out)
    }
}
