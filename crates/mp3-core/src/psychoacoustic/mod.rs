//! Psychoacoustic Model II: masking-threshold estimation and
//! block-switching decisions. See
//! `docs/mp3-encoder/07-phase4-psychoacoustic-model.md`.
//!
//! This runs on **raw PCM through its own FFT**, independently of the
//! filterbank/MDCT path in [`crate::filterbank`]/[`crate::mdct`] — see
//! that chapter's §1 for why the two paths are not pipelined together.

mod fft;
mod model2;
mod tables;

pub use fft::{fft_magnitude, fft_windowed_complex};
pub use model2::{PsychoacousticModel, ScalefactorBandSmr};
pub use tables::{
    absolute_threshold_db, bark_of_hz, spreading_db, SFB_LONG_BOUNDARIES, SFB_SHORT_BOUNDARIES,
};
