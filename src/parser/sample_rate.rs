#[derive(Debug, Clone)]
pub struct SampleRateEstimate {
    pub rate_hz: f32,
    pub median_dt: u64,
    pub jitter_std: f32,
    pub min_dt: u64,
    pub max_dt: u64,
}

impl SampleRateEstimate {
    pub fn from_timestamps(time_us: &[u64]) -> Self {
        let diffs = Self::compute_diffs(time_us);

        if diffs.is_empty() {
            return Self::empty();
        }

        let median_dt = Self::median(&diffs);
        let (_, jitter_std) = Self::mean_and_std(&diffs);
        let (min_dt, max_dt) = Self::min_max(&diffs);
        let rate_hz = Self::rate_from_dt(median_dt);

        Self {
            rate_hz,
            median_dt,
            jitter_std,
            min_dt,
            max_dt,
        }
    }

    pub fn from_looptime(looptime_us: u32) -> Self {
        if looptime_us == 0 {
            return Self::empty();
        }
        let rate_hz = 1_000_000.0 / looptime_us as f32;
        Self {
            rate_hz,
            median_dt: looptime_us as u64,
            jitter_std: 0.0,
            min_dt: looptime_us as u64,
            max_dt: looptime_us as u64,
        }
    }

    pub fn from_hz(hz: f32) -> Self {
        Self::from_looptime((1e6 as u32) / (hz as u32))
    }

    pub fn empty() -> Self {
        Self {
            rate_hz: 0.0,
            median_dt: 0,
            jitter_std: 0.0,
            min_dt: 0,
            max_dt: 0,
        }
    }

    fn compute_diffs(time_us: &[u64]) -> Vec<u64> {
        time_us
            .windows(2)
            .map(|w| w[1].saturating_sub(w[0]))
            .filter(|&dt| dt > 0)
            .collect()
    }

    fn median(diffs: &[u64]) -> u64 {
        let mut copy = diffs.to_vec();
        let mid = copy.len() / 2;
        *copy.select_nth_unstable(mid).1
    }

    fn mean_and_std(diffs: &[u64]) -> (f32, f32) {
        let len = diffs.len() as f32;
        let mean = diffs.iter().sum::<u64>() as f32 / len;
        let variance = diffs
            .iter()
            .map(|&dt| {
                let d = dt as f32 - mean;
                d * d
            })
            .sum::<f32>()
            / len;
        (mean, variance.sqrt())
    }

    fn min_max(diffs: &[u64]) -> (u64, u64) {
        (*diffs.iter().min().unwrap(), *diffs.iter().max().unwrap())
    }

    fn rate_from_dt(median_dt: u64) -> f32 {
        if median_dt == 0 {
            return 0.0;
        }
        1_000_000.0 / median_dt as f32
    }
}
