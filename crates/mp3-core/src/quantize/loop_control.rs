//! The inner (rate) loop and outer (distortion) loop. See
//! `docs/mp3-encoder/08-phase5-quantization-loop.md` §3 — the
//! algorithmic heart of the encoder, where quality/bitrate trade-offs
//! actually happen.

use crate::mdct::BlockType;
use crate::psychoacoustic::ScalefactorBandSmr;
use crate::quantize::scalefactors::ScaleFactors;

/// Everything the quantization loop produces for one granule/channel:
/// the quantized values themselves plus every side-info field this stage
/// owns. See `docs/mp3-encoder/08-phase5-quantization-loop.md` (inputs/
/// outputs) and
/// `docs/mp3-encoder/11-phase8-bitstream-multiplexing.md` §4 (where these
/// fields land in the bitstream).
#[derive(Debug, Clone, Copy)]
pub struct QuantizationResult {
    /// Quantized magnitudes (sign is tracked separately — see
    /// `docs/mp3-encoder/08-phase5-quantization-loop.md` §2 for why).
    pub ix: [i32; 576],
    /// Sign of each nonzero spectral line (`true` = negative).
    pub sign: [bool; 576],
    /// Per-band scale factors.
    pub scalefac: ScaleFactors,
    /// `global_gain` side-info field (overall quantizer step size).
    pub global_gain: u8,
    /// `scalefac_scale` side-info field.
    pub scalefac_scale: bool,
    /// `preflag` side-info field.
    pub preflag: bool,
    /// `subblock_gain` per short window — `None` for long/start/stop
    /// blocks.
    pub subblock_gain: Option<[u8; 3]>,
}

/// Runs the inner (rate) loop nested inside the outer (distortion) loop
/// to quantize one granule's spectrum against its SMR-derived allowed
/// distortion and bit budget.
///
/// # Panics
///
/// Always, in this scaffold. Implements
/// `docs/mp3-encoder/08-phase5-quantization-loop.md` §2-3:
///
/// - Non-uniform (power-law) quantizer — note the ¾ power applies to
///   the *already-scaled* value (see
///   `docs/mp3-encoder/08-phase5-quantization-loop.md` §2 for why the
///   other circulating formulation is wrong, and for the `global_gain`
///   −210 bias):
///   `ix[i] = nint((|xr[i]| * 2^(-quantizer_step/4))^0.75 - 0.0946)`.
/// - Inner loop: increase `quantizer_step` until the Huffman-estimated
///   bit count ([`crate::huffman::encode::estimate_bits`]) fits
///   `bit_budget`.
/// - Outer loop: per scalefactor band, if quantization noise exceeds the
///   SMR-derived allowed distortion, amplify that band's scale factor and
///   re-run the inner loop. **Must terminate** within a documented
///   maximum iteration count even if not fully converged — see that
///   chapter's §3 for why convergence isn't guaranteed by the math alone.
pub fn quantize_granule(
    spectrum: &[f32; 576],
    smr: &ScalefactorBandSmr,
    bit_budget: u32,
    block_type: BlockType,
) -> QuantizationResult {
    let _ = (spectrum, smr, bit_budget, block_type);
    todo!("M5: inner loop (rate) + outer loop (distortion) — see 08-phase5 §3")
}

#[cfg(test)]
mod tests {
    // TODO(M5):
    // - Quantizer round-trip: a test-only dequantizer (inverse of §2's
    //   formula) shows bounded, step-size-monotonic quantization error.
    // - Inner loop converges under a tight bit budget; converges quickly
    //   under a generous one (regression guard against accidental
    //   over-coarsening when bits are plentiful).
    // - Outer loop amplifies an "unmasked" band's scale factor relative
    //   to a uniform-SMR control run.
    // - Non-convergence path (crafted oscillating input) terminates
    //   within the documented max-iteration bound rather than looping
    //   forever or panicking.
    // - No heap allocation inside the loop bodies.
    // See docs/mp3-encoder/08-phase5-quantization-loop.md §5.
}
