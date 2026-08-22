pub mod filter_response;
pub mod overlays;
pub mod spectral;
pub mod step_response;

pub use filter_response::{FilterResponse, Stage};
pub use overlays::{
    ByAxis, Driven, Dwell, FilterLoop, FilterOverlay, HarmonicBand, OverlayFamily, OverlayShape,
};
pub use spectral::{
    AxisSpectral, DynNotchReach, FrequencyPeak, GyroNoiseAnalyzer, SpectralAnalysis,
};
pub use step_response::{
    AxisStepResponse, NoStepResponse, StepMetrics, StepResponseAnalysis, StepResponseAnalyzer,
};

/// How much of each end of a log every analyser leaves out by default. One
/// constant rather than one per analyser: it is a claim about where a flight
/// starts, not about any one measurement, and two copies of it would drift.
pub const DEFAULT_TRIM_S: f64 = 2.0;

/// Everything computed from one sublog at load time. Bundled so a new
/// analyser adds a field here rather than another `Vec` to the loader, the
/// log store and the tab context.
#[derive(Debug, Clone, Default)]
pub struct Analysis {
    pub spectral: SpectralAnalysis,
    pub step: StepResponseAnalysis,
}
