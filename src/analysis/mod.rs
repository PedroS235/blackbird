pub mod spectral;
pub mod step_response;

pub use spectral::{GyroNoiseAnalyzer, SpectralAnalysis};
pub use step_response::{
    AxisStepResponse, NoStepResponse, StepResponseAnalysis, StepResponseAnalyzer,
};

/// Everything computed from one sublog at load time. Bundled so a new
/// analyser adds a field here rather than another `Vec` to the loader, the
/// log store and the tab context.
#[derive(Debug, Clone, Default)]
pub struct Analysis {
    pub spectral: SpectralAnalysis,
    pub step: StepResponseAnalysis,
}
