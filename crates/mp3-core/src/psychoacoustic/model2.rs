//! Psychoacoustic Model II proper: partitions, spreading function, SMR,
//! and the block-switching decision. See
//! `docs/mp3-encoder/07-phase4-psychoacoustic-model.md` §3-5.

use crate::mdct::BlockType;

/// FFT magnitude + phase for one bin, kept across frames for the
/// unpredictability (tonality) measure. See
/// `docs/mp3-encoder/07-phase4-psychoacoustic-model.md` §3.
// Not yet read: analyze_granule (the only reader) is `todo!()` pending
// M4 — see docs/mp3-encoder/14-roadmap-and-milestones.md.
#[allow(dead_code)]
#[derive(Clone, Copy, Default)]
struct FftBin {
    magnitude: f32,
    phase: f32,
}

/// Signal-to-mask ratio per scalefactor band for one granule/channel —
/// the output this module hands to
/// [`crate::quantize::loop_control::quantize_granule`]'s outer loop. See
/// `docs/mp3-encoder/07-phase4-psychoacoustic-model.md` §4 step 5.
#[derive(Debug, Clone, Copy)]
pub struct ScalefactorBandSmr {
    /// SMR (dB or linear ratio — decide and document the convention
    /// consistently with `docs/mp3-encoder/08-phase5-quantization-loop.md`
    /// when implementing) per scalefactor band. Sized for the maximum
    /// long-block band count; short-block granules use a subset — see
    /// `docs/mp3-encoder/08-phase5-quantization-loop.md` §1 for why this
    /// grouping differs from the psychoacoustic partition grouping below.
    pub bands: [f32; 22],
}

/// Holds the FFT-history state the unpredictability measure needs across
/// frames. One instance per channel — do not share between channels.
pub struct PsychoacousticModel {
    // FFT magnitude+phase history, 2 frames back, for the long-block
    // (1024-point) analysis. Short-block (256-point) analysis uses a
    // separate, smaller history — add it here once M4 needs it; not
    // included yet, to avoid guessing its exact size before implementing
    // 07-phase4-psychoacoustic-model.md §3 for real.
    history: [[FftBin; 513]; 2],
}

impl Default for PsychoacousticModel {
    fn default() -> Self {
        Self::new()
    }
}

impl PsychoacousticModel {
    /// Creates a model with empty (silent) history.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            history: [[FftBin {
                magnitude: 0.0,
                phase: 0.0,
            }; 513]; 2],
        }
    }

    /// Analyzes one granule's worth of raw PCM (see this module's parent
    /// doc comment for why this is *not* filterbank/MDCT output) and
    /// produces the SMR values plus block-type decision for that granule.
    ///
    /// # Panics
    ///
    /// Always, in this scaffold. Implements
    /// `docs/mp3-encoder/07-phase4-psychoacoustic-model.md` §3-5:
    /// unpredictability measure, partition grouping + minimum masking
    /// ratios (Annex D — cite source before filling in), spreading
    /// function convolution, absolute threshold of hearing, mapping to
    /// scalefactor bands, and the transient-detection block-type call.
    pub fn analyze_granule(&mut self, pcm_window: &[f32]) -> (ScalefactorBandSmr, BlockType) {
        let _ = (&mut self.history, pcm_window);
        todo!("M4: see 07-phase4-psychoacoustic-model.md §3-5")
    }
}

#[cfg(test)]
mod tests {
    // TODO(M4):
    // - SMR qualitative shape test: a pure tone produces high SMR at its
    //   own frequency, decreasing with distance (spreading function
    //   visible); white noise produces a comparatively flat SMR.
    // - Transient detection: a burst of full-scale noise in otherwise
    //   silent audio is flagged Short; a stationary sine or silence is
    //   not.
    // - No NaN/Inf across silence and full-scale fixtures.
    // See docs/mp3-encoder/07-phase4-psychoacoustic-model.md §6.
}
