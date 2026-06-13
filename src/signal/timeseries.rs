use scirs2_signal::savgol;

pub enum SmoothingFactor {
    Low,
    Medium,
    High,
}

pub fn smooth_slice(samples: &[f64], factor: SmoothingFactor) -> Option<Vec<f64>> {
    let (window, poly) = match factor {
        SmoothingFactor::Low    => (5usize,  3usize),
        SmoothingFactor::Medium => (11,      3),
        SmoothingFactor::High   => (21,      3),
    };
    savgol::savgol_filter(samples, window, poly, None, None, None, None).ok()
}
