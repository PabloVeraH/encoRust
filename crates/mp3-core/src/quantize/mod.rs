//! Non-uniform quantization and the inner/outer iteration loops. See
//! `docs/mp3-encoder/08-phase5-quantization-loop.md`.

pub mod loop_control;
pub mod scalefactors;

pub use loop_control::{quantize_granule, QuantizationResult};
pub use scalefactors::ScaleFactors;
