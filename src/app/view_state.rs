/// Per-tab UI state (checkbox visibility, slider values) for `MainTab::Timeseries`.
pub(super) struct TimeseriesTabState {
    pub(super) gyro_filtered_visible: [bool; 3],
    pub(super) gyro_raw_visible: [bool; 3],
    pub(super) vbat_visible: bool,
    pub(super) current_visible: bool,
    pub(super) rssi_visible: bool,
}

impl Default for TimeseriesTabState {
    fn default() -> Self {
        Self {
            gyro_filtered_visible: [true; 3],
            gyro_raw_visible: [true; 3],
            vbat_visible: true,
            current_visible: true,
            rssi_visible: true,
        }
    }
}

/// Per-tab UI state for `MainTab::FilterAnalysis`. `psd_filtered_visible` and
/// `frequency_filtered_visible` are deliberately separate fields — they used
/// to be the same field shared across the Psd and Frequency sub-tabs, which
/// meant toggling one silently toggled the other.
pub(super) struct FilterAnalysisTabState {
    pub(super) psd_filtered_visible: [bool; 3],
    pub(super) frequency_filtered_visible: [bool; 3],
    pub(super) frequency_peak_min_hz: f32,
    pub(super) heatmap_floor_db: f32,
    pub(super) spectrogram_floor_db: f32,
}

impl Default for FilterAnalysisTabState {
    fn default() -> Self {
        Self {
            psd_filtered_visible: [false; 3],
            frequency_filtered_visible: [false; 3],
            frequency_peak_min_hz: 100.0,
            heatmap_floor_db: -60.0,
            spectrogram_floor_db: -60.0,
        }
    }
}

/// Per-tab UI state for `MainTab::PidAnalysis`. `gyro_filtered_visible` is its
/// own field, independent from `TimeseriesTabState`'s — see that struct's doc
/// comment for why they used to be (incorrectly) shared.
pub(super) struct PidAnalysisTabState {
    pub(super) gyro_filtered_visible: [bool; 3],
    pub(super) setpoint_visible: [bool; 3],
}

impl Default for PidAnalysisTabState {
    fn default() -> Self {
        Self {
            gyro_filtered_visible: [true; 3],
            setpoint_visible: [true; 3],
        }
    }
}

#[derive(Default)]
pub(super) struct MainViewState {
    pub(super) timeseries: TimeseriesTabState,
    pub(super) filter_analysis: FilterAnalysisTabState,
    pub(super) pid_analysis: PidAnalysisTabState,
}
