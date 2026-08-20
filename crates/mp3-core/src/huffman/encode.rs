//! Region splitting, table selection, and bit emission — plus the cheap
//! bit-count estimator the quantization inner loop depends on. See
//! `docs/mp3-encoder/09-phase6-huffman-coding.md` §3-4.

use crate::bitstream::writer::BitWriter;

/// Side-info fields this stage owns, handed to
/// [`crate::bitstream::side_info`] for final assembly. See
/// `docs/mp3-encoder/09-phase6-huffman-coding.md` §1.
#[derive(Debug, Clone, Copy)]
pub struct HuffmanSideInfo {
    /// Count of values coded in the `big_values` region (×2 = actual
    /// sample count covered, since values are coded in pairs there).
    pub big_values: u16,
    /// Scalefactor-band boundary of the first `big_values` sub-region.
    pub region0_count: u8,
    /// Scalefactor-band boundary of the second `big_values` sub-region.
    pub region1_count: u8,
    /// Huffman table used per `big_values` sub-region (0, 1, 2).
    pub table_select: [u8; 3],
    /// Which of the 2 `count1` tables was used.
    pub count1table_select: bool,
}

/// Fast, allocation-free bit-count estimate for the quantization inner
/// loop ([`crate::quantize::loop_control::quantize_granule`]). Must
/// over-estimate rather than under-estimate on ties, so the inner loop
/// never produces a bitstream that overflows its budget once
/// [`encode_granule`] runs for real. See
/// `docs/mp3-encoder/09-phase6-huffman-coding.md` §4.
///
/// # Panics
///
/// Always, in this scaffold.
#[must_use]
pub fn estimate_bits(ix: &[i32; 576]) -> u32 {
    let _ = ix;
    todo!("M6: cheap heuristic bit-count estimate — see 09-phase6 §4")
}

/// Full Huffman encode: region splitting + exhaustive per-region table
/// selection + `count1` region + escape (`linbits`) handling, emitting
/// bits via `writer`. Called once per granule, after the quantization
/// loop has converged — unlike [`estimate_bits`], this does the full
/// table-selection search. See
/// `docs/mp3-encoder/09-phase6-huffman-coding.md` §3-4.
///
/// # Panics
///
/// Always, in this scaffold.
pub fn encode_granule(ix: &[i32; 576], writer: &mut BitWriter<'_>) -> HuffmanSideInfo {
    let _ = (ix, writer);
    todo!("M6: region split + table select + count1 + emit — see 09-phase6 §3-4")
}

#[cfg(test)]
mod tests {
    // TODO(M6):
    // - estimate_bits never under-counts relative to encode_granule's
    //   actual output length, across a battery of synthetic ix[]
    //   fixtures.
    // - Round-trip against an independent decoder's Huffman module.
    // - Escape (linbits) mechanism round-trips a large magnitude.
    // See docs/mp3-encoder/09-phase6-huffman-coding.md §5.
}
