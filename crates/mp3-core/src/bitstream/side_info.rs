//! Granule/channel side info bit layout. See
//! `docs/mp3-encoder/11-phase8-bitstream-multiplexing.md` §4.

use crate::huffman::encode::HuffmanSideInfo;
use crate::mdct::BlockType;
use crate::quantize::QuantizationResult;

/// Full side-info block for one frame (both granules, both channels for
/// stereo modes). Frame-level fields (`main_data_begin`, `private_bits`,
/// `scfsi`) plus, per granule/channel, the fields chapters 08/09 produce.
/// See `docs/mp3-encoder/11-phase8-bitstream-multiplexing.md` §4 for the
/// exact bit-width table to transcribe from Annex B.
#[derive(Debug, Clone, Copy)]
pub struct SideInfo {
    /// How many bytes back (0-511 for MPEG-1) this frame's `main_data`
    /// starts, relative to this frame's own header — see
    /// `docs/mp3-encoder/10-phase7-bit-reservoir-and-rate-control.md` §1
    /// and `docs/mp3-encoder/11-phase8-bitstream-multiplexing.md` §5.
    pub main_data_begin: u16,
    /// Scalefactor-sharing-between-granules flags. This scaffold's
    /// planned first implementation always sets these to "not shared"
    /// (all `false`) — see
    /// `docs/mp3-encoder/11-phase8-bitstream-multiplexing.md` §4 — that
    /// is a deliberate, documented simplification, not an oversight, and
    /// should be reflected in the M8 status row of
    /// `docs/mp3-encoder/14-roadmap-and-milestones.md` (⚠️) if left as-is
    /// when M8 is otherwise complete.
    pub scfsi: [[bool; 4]; 2],
    /// Per-granule, per-channel side info, combining chapter 08's
    /// quantization output, chapter 09's Huffman output, and the block
    /// type. Indexed `[granule][channel]`.
    pub granules: [[GranuleSideInfo; 2]; 2],
}

/// Side info for one granule of one channel.
#[derive(Debug, Clone, Copy)]
pub struct GranuleSideInfo {
    /// Total bits this granule's scalefactors + Huffman data consume.
    pub part2_3_length: u16,
    /// Long/start/short/stop — see [`crate::mdct::BlockType`].
    pub block_type: BlockType,
    /// Whether this granule mixes long-block windows on the lowest 2
    /// subbands with short-block windows elsewhere — see
    /// `docs/mp3-encoder/06-phase3-mdct-and-windowing.md` §1.
    pub mixed_block_flag: bool,
    /// Quantization output for this granule/channel (chapter 08).
    pub quant: QuantizationResult,
    /// Huffman coding output for this granule/channel (chapter 09).
    pub huffman: HuffmanSideInfo,
}

#[cfg(test)]
mod tests {
    // TODO(M8): assemble a SideInfo from synthetic QuantizationResult /
    // HuffmanSideInfo fixtures and confirm bit-packing (once
    // implemented) round-trips through a test-only inverse parser field
    // by field. See
    // docs/mp3-encoder/11-phase8-bitstream-multiplexing.md §4 and §7.
}
