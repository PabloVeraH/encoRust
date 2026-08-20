//! Scalefactor band boundary tables and per-band scale factors. See
//! `docs/mp3-encoder/08-phase5-quantization-loop.md` §1.
//!
//! These boundaries are distinct from the psychoacoustic model's
//! partition table ([`crate::psychoacoustic`]) even though both
//! approximate critical bands — do not conflate the two; one groups FFT
//! bins for masking estimation, this one groups MDCT lines for
//! quantization/coding. See
//! `docs/mp3-encoder/08-phase5-quantization-loop.md` §1 for the explicit
//! warning.

/// Scalefactor band boundaries (in MDCT line index) for long blocks, one
/// row per supported sample rate (ordered
/// `[44100, 48000, 32000, 22050, 24000, 16000]` Hz — keep in sync with
/// [`crate::types::SampleRate`]'s declaration order), 21 bands (22
/// boundary points) per row.
///
/// # ⚠️ Placeholder — not implemented
///
/// Zeroed. Must be sourced from ISO/IEC 11172-3 Annex B before use — see
/// `docs/mp3-encoder/00-overview.md` §4.1 and
/// `docs/mp3-encoder/13-testing-and-validation.md` §Table provenance.
pub const SFB_LONG_BOUNDARIES: [[usize; 22]; 6] = [[0; 22]; 6];

/// Scalefactor band boundaries for short blocks (12 bands, 13 boundary
/// points), same row ordering as [`SFB_LONG_BOUNDARIES`].
///
/// # ⚠️ Placeholder — not implemented
pub const SFB_SHORT_BOUNDARIES: [[usize; 13]; 6] = [[0; 13]; 6];

/// Per-scalefactor-band scale factors for one granule/channel. Shape
/// depends on block type: long blocks use up to 21 bands; short blocks
/// use up to 12 bands × 3 windows. Represented as a flat array sized for
/// the worst case (short-block layout, which has more total entries);
/// [`crate::mdct::BlockType`] on the owning
/// [`crate::quantize::QuantizationResult`] says how to interpret it.
#[derive(Debug, Clone, Copy)]
pub struct ScaleFactors {
    /// Raw scale factor values, before `scalefac_compress`-based bit-width
    /// selection (side info concern, see
    /// `docs/mp3-encoder/11-phase8-bitstream-multiplexing.md` §4).
    pub values: [u8; 39], // 21 long-block bands, or 12*3=36 short-block (+3 spare not used long)
}

#[cfg(test)]
mod tests {
    // TODO(M5): table-provenance test for SFB_LONG_BOUNDARIES /
    // SFB_SHORT_BOUNDARIES once populated from Annex B. See
    // docs/mp3-encoder/13-testing-and-validation.md §Table provenance.
}
