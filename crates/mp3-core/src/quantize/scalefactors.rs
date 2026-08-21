//! Scalefactor band boundary tables and per-band scale factors. See
//! `docs/mp3-encoder/08-phase5-quantization-loop.md` §1.
//!
//! These boundaries are distinct from the psychoacoustic model's
//! *partition* table ([`crate::psychoacoustic::compute_partition_map`])
//! even though both approximate critical bands — do not conflate the
//! two; the partition table groups FFT bins for masking estimation (~48
//! partitions), while this one groups MDCT lines for quantization/coding
//! (22 long-block / 13 short-block bands). See
//! `docs/mp3-encoder/08-phase5-quantization-loop.md` §1 for the explicit
//! warning.
//!
//! Unlike the partition table, the scalefactor-band grid itself is
//! **shared** between this module and the psychoacoustic model: chapter
//! 07 §4.5 maps partition thresholds onto "the 32-subband/scalefactor-band
//! grid used by chapters 06/08" to compute per-band SMR, i.e. M4's SMR
//! output and M5's quantization both need the exact same Annex B table.
//! It is defined once, in [`crate::psychoacoustic::tables`] (where it was
//! sourced and table-provenance-tested for M4), and re-exported here
//! rather than duplicated — an earlier version of this file kept an
//! independent, differently-shaped placeholder (`[[usize; 22]; 6]` for
//! long / `[[usize; 13]; 6]` implying 21/12 bands), which would have
//! silently diverged from the psychoacoustic module's 22/13-band table.

pub use crate::psychoacoustic::{SFB_LONG_BOUNDARIES, SFB_SHORT_BOUNDARIES};

/// Per-scalefactor-band scale factors for one granule/channel. Shape
/// depends on block type: long blocks use up to 22 bands; short blocks
/// use up to 13 bands × 3 windows. Represented as a flat array sized for
/// the worst case (short-block layout, which has more total entries);
/// [`crate::mdct::BlockType`] on the owning
/// [`crate::quantize::QuantizationResult`] says how to interpret it.
#[derive(Debug, Clone, Copy)]
pub struct ScaleFactors {
    /// Raw scale factor values, before `scalefac_compress`-based bit-width
    /// selection (side info concern, see
    /// `docs/mp3-encoder/11-phase8-bitstream-multiplexing.md` §4).
    pub values: [u8; 39], // 22 long-block bands, or 13*3=39 short-block
}

#[cfg(test)]
mod tests {
    use super::*;

    // Table-provenance tests for SFB_LONG_BOUNDARIES / SFB_SHORT_BOUNDARIES
    // themselves (checksum + monotonicity) live in
    // `psychoacoustic::tables::tests`, next to their single definition —
    // see the module doc comment above. This test only guards the shape
    // this module's own consumers (M5's quantization loop) will rely on.
    #[test]
    fn reexported_sfb_tables_have_expected_shape() {
        assert_eq!(SFB_LONG_BOUNDARIES.len(), 6, "one row per sample rate");
        assert_eq!(SFB_SHORT_BOUNDARIES.len(), 6, "one row per sample rate");
        for row in &SFB_LONG_BOUNDARIES {
            assert_eq!(row[row.len() - 1], 576, "long-block rows end at line 576");
        }
        for row in &SFB_SHORT_BOUNDARIES {
            assert_eq!(row[row.len() - 1], 192, "short-block rows end at line 192");
        }
    }
}
