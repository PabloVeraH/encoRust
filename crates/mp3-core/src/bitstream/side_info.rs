//! Granule/channel side info bit layout. See
//! `docs/mp3-encoder/11-phase8-bitstream-multiplexing.md` §4.

use crate::bitstream::writer::BitWriter;
use crate::huffman::encode::HuffmanSideInfo;
use crate::mdct::BlockType;
use crate::quantize::QuantizationResult;
use crate::types::ChannelMode;

/// Full side-info block for one frame (both granules, both channels for
/// stereo modes). Frame-level fields (`main_data_begin`, `private_bits`,
/// `scfsi`) plus, per granule/channel, the fields chapters 08/09 produce.
/// See `docs/mp3-encoder/11-phase8-bitstream-multiplexing.md` §4 for the
/// exact bit-width table to transcribe from Annex B.
#[derive(Debug, Clone, Copy)]
pub struct SideInfo {
    /// Byte offset (backward, into the bit reservoir) where this frame's
    /// main_data actually begins. 9-bit field.
    pub main_data_begin: u16,
    /// Scalefactor selection info: `scfsi[channel][band_group]`, `true`
    /// means granule 1 reuses granule 0's scalefactors for that band
    /// group instead of retransmitting them.
    pub scfsi: [[bool; 4]; 2],
    /// Per-granule, per-channel side info: `granules[granule][channel]`.
    pub granules: [[GranuleSideInfo; 2]; 2],
}

/// Side info for one granule of one channel.
#[derive(Debug, Clone, Copy)]
pub struct GranuleSideInfo {
    /// Length in bits of the scalefactors plus Huffman data for this
    /// granule/channel (the `part2_3_length` field of Annex B).
    pub part2_3_length: u16,
    /// Block type (long/start/short/stop) this granule was encoded with.
    pub block_type: BlockType,
    /// Whether this short-block granule mixes in long-block subbands at
    /// the low end (only meaningful when `block_type == BlockType::Short`).
    pub mixed_block_flag: bool,
    /// Index into the scalefactor-length table (`SLEN_TABLE`) selecting
    /// how many bits each scalefactor slot uses.
    pub scalefac_compress: u8,
    /// Quantization results for this granule/channel (global gain,
    /// scalefactor-related flags, subblock gains).
    pub quant: QuantizationResult,
    /// Huffman coding parameters for this granule/channel (region
    /// boundaries, table selections, count1 table choice).
    pub huffman: HuffmanSideInfo,
}

impl SideInfo {
    /// Packs this side-info block into bytes via `writer`, per
    /// ISO/IEC 11172-3 Annex B §2.4.2.7 (MPEG-1) layout.
    ///
    /// Stereo: 9 + 3 + 8 + 4×59 = 256 bits = 32 bytes.
    /// Mono: 9 + 5 + 4 + 2×59 = 136 bits = 17 bytes.
    pub fn write(&self, writer: &mut BitWriter<'_>, channel_mode: ChannelMode) {
        let stereo = channel_mode.is_stereo();

        // Running count of bits actually written, kept independently of
        // `writer`'s own internal state, so the debug_assert!s below are a
        // genuine cross-check on this function's field-width bookkeeping
        // (e.g. a future edit that adds/removes a field without updating
        // its width) rather than a tautology against the same values that
        // produced the output.
        let mut total_bits: u32 = 0;
        macro_rules! wb {
            ($value:expr, $n:expr) => {{
                let n: u8 = $n;
                writer.write_bits($value, n);
                total_bits += u32::from(n);
            }};
        }

        // Frame-level fields
        wb!(u32::from(self.main_data_begin), 9);
        wb!(0, if stereo { 3 } else { 5 }); // private_bits

        let channels = if stereo { 2 } else { 1 };
        for ch in 0..channels {
            for i in 0..4 {
                wb!(u32::from(self.scfsi[ch][i]), 1);
            }
        }

        // Per-granule, per-channel: exactly 59 bits each (MPEG-1).
        for gr in 0..2 {
            for ch in 0..channels {
                let gi = &self.granules[gr][ch];
                let ws = gi.block_type != BlockType::Long;
                let granule_start_bits = total_bits;

                // Fixed part (33 bits)
                wb!(u32::from(gi.part2_3_length), 12);
                wb!(u32::from(gi.huffman.big_values), 9);
                wb!(u32::from(gi.quant.global_gain), 8);
                wb!(u32::from(gi.scalefac_compress), 4);

                // Conditional part (23 bits inc. flag)
                wb!(u32::from(ws), 1);

                if ws {
                    // flag=1: block_type(2) + mixed_block_flag(1)
                    //       + table_select×2(10) + subblock_gain×3(9) = 22
                    wb!(block_type_bits(gi.block_type), 2);
                    wb!(u32::from(gi.mixed_block_flag), 1);
                    wb!(u32::from(gi.huffman.table_select[0]), 5);
                    wb!(u32::from(gi.huffman.table_select[1]), 5);
                    let sbg = gi.quant.subblock_gain.unwrap_or([0u8; 3]);
                    for g in sbg {
                        wb!(u32::from(g), 3);
                    }
                } else {
                    // flag=0: table_select×3(15) + region0_count(4)
                    //       + region1_count(3) = 22
                    wb!(u32::from(gi.huffman.table_select[0]), 5);
                    wb!(u32::from(gi.huffman.table_select[1]), 5);
                    wb!(u32::from(gi.huffman.table_select[2]), 5);
                    wb!(u32::from(gi.huffman.region0_count), 4);
                    wb!(u32::from(gi.huffman.region1_count), 3);
                }

                // Tail (3 bits, always present)
                wb!(u32::from(gi.quant.preflag), 1);
                wb!(u32::from(gi.quant.scalefac_scale), 1);
                wb!(u32::from(gi.huffman.count1table_select), 1);

                debug_assert_eq!(
                    total_bits - granule_start_bits,
                    59,
                    "granule {gr}/channel {ch}: side-info block must be \
                     exactly 59 bits per ISO/IEC 11172-3 Annex B (MPEG-1 \
                     only -- LSF's 63-bit layout isn't implemented; see \
                     `Encoder::new`'s `UnsupportedSampleRate` rejection)"
                );
            }
        }

        let expected_total = if stereo { 256 } else { 136 };
        debug_assert_eq!(
            total_bits,
            expected_total,
            "side info total ({total_bits} bits) doesn't match the \
             ISO/IEC 11172-3 Annex B invariant for {} channel mode: \
             {expected_total} bits ({} bytes)",
            if stereo { "stereo" } else { "mono" },
            expected_total / 8,
        );
    }
}

fn block_type_bits(bt: BlockType) -> u32 {
    match bt {
        BlockType::Long => 0,
        BlockType::Start => 1,
        BlockType::Short => 2,
        BlockType::Stop => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Pre-existing no_std-test gap — see the identical note in
    // bitstream/scalefactor_encode.rs's test module.
    use crate::bitstream::writer::BitWriter;
    use crate::huffman::encode::HuffmanSideInfo;
    use crate::mdct::BlockType;
    use crate::quantize::{QuantizationResult, ScaleFactors};
    use crate::types::ChannelMode;
    use alloc::vec::Vec;

    struct BitReader<'a> {
        data: &'a [u8],
        pos: usize,
    }
    impl<'a> BitReader<'a> {
        fn read_bits(&mut self, n: u8) -> u32 {
            let mut v = 0u32;
            for _ in 0..n {
                if self.pos / 8 >= self.data.len() {
                    break;
                }
                let byte = self.data[self.pos / 8];
                let bit = (byte >> (7 - self.pos % 8)) & 1;
                v = (v << 1) | u32::from(bit);
                self.pos += 1;
            }
            v
        }
    }

    fn make_test_granule(
        part2_3_length: u16,
        big_values: u16,
        global_gain: u8,
        table_select: [u8; 3],
        region0_count: u8,
        region1_count: u8,
        block_type: BlockType,
    ) -> GranuleSideInfo {
        GranuleSideInfo {
            part2_3_length,
            block_type,
            mixed_block_flag: false,
            scalefac_compress: 0,
            quant: QuantizationResult {
                ix: [0; 576],
                sign: [false; 576],
                scalefac: ScaleFactors { values: [0; 39] },
                global_gain,
                scalefac_scale: false,
                preflag: false,
                subblock_gain: None,
                converged: true,
            },
            huffman: HuffmanSideInfo {
                big_values,
                region0_count,
                region1_count,
                table_select,
                count1table_select: false,
            },
        }
    }

    #[test]
    fn side_info_stereo_round_trip() {
        let si = SideInfo {
            main_data_begin: 0x1FF,
            scfsi: [[true, false, true, false], [false, true, false, true]],
            granules: [
                [
                    make_test_granule(0x500, 0x100, 0x80, [1, 2, 3], 5, 6, BlockType::Long),
                    make_test_granule(0x300, 0x080, 0x60, [4, 5, 6], 7, 6, BlockType::Long),
                ],
                [
                    make_test_granule(0x200, 0x040, 0x40, [7, 8, 9], 3, 4, BlockType::Long),
                    make_test_granule(0x100, 0x020, 0x20, [10, 11, 12], 1, 2, BlockType::Long),
                ],
            ],
        };

        let mut buf = Vec::new();
        {
            let mut writer = BitWriter::new(&mut buf);
            si.write(&mut writer, ChannelMode::Stereo);
            writer.flush();
        }

        // 9 + 3 + 8 + 4*59 = 256 bits = 32 bytes
        assert_eq!(buf.len(), 32);

        let mut r = BitReader { data: &buf, pos: 0 };

        assert_eq!(r.read_bits(9), 0x1FF); // main_data_begin
        assert_eq!(r.read_bits(3), 0); // private_bits (3 for stereo)
                                       // scfsi[0]
        assert_eq!(r.read_bits(1), 1);
        assert_eq!(r.read_bits(1), 0);
        assert_eq!(r.read_bits(1), 1);
        assert_eq!(r.read_bits(1), 0);
        // scfsi[1]
        assert_eq!(r.read_bits(1), 0);
        assert_eq!(r.read_bits(1), 1);
        assert_eq!(r.read_bits(1), 0);
        assert_eq!(r.read_bits(1), 1);

        // gr0/ch0 — flag=0 path
        assert_eq!(r.read_bits(12), 0x500);
        assert_eq!(r.read_bits(9), 0x100);
        assert_eq!(r.read_bits(8), 0x80);
        assert_eq!(r.read_bits(4), 0); // scalefac_compress
        assert_eq!(r.read_bits(1), 0); // window_switching_flag
        assert_eq!(r.read_bits(5), 1); // table_select[0]
        assert_eq!(r.read_bits(5), 2); // table_select[1]
        assert_eq!(r.read_bits(5), 3); // table_select[2]
        assert_eq!(r.read_bits(4), 5); // region0_count
        assert_eq!(r.read_bits(3), 6); // region1_count
        assert_eq!(r.read_bits(1), 0); // preflag
        assert_eq!(r.read_bits(1), 0); // scalefac_scale
        assert_eq!(r.read_bits(1), 0); // count1table_select

        // gr0/ch1 — flag=0 path
        assert_eq!(r.read_bits(12), 0x300);
        assert_eq!(r.read_bits(9), 0x080);
        assert_eq!(r.read_bits(8), 0x60);
        assert_eq!(r.read_bits(4), 0);
        assert_eq!(r.read_bits(1), 0);
        assert_eq!(r.read_bits(5), 4);
        assert_eq!(r.read_bits(5), 5);
        assert_eq!(r.read_bits(5), 6);
        assert_eq!(r.read_bits(4), 7);
        assert_eq!(r.read_bits(3), 6);
        assert_eq!(r.read_bits(1), 0);
        assert_eq!(r.read_bits(1), 0);
        assert_eq!(r.read_bits(1), 0);
    }

    #[test]
    fn side_info_mono_round_trip() {
        let si = SideInfo {
            main_data_begin: 0,
            scfsi: [[false; 4]; 2],
            granules: [
                [
                    make_test_granule(100, 50, 100, [1, 2, 3], 5, 6, BlockType::Long),
                    make_test_granule(0, 0, 0, [0, 0, 0], 0, 0, BlockType::Long),
                ],
                [
                    make_test_granule(200, 100, 200, [4, 5, 6], 7, 6, BlockType::Long),
                    make_test_granule(0, 0, 0, [0, 0, 0], 0, 0, BlockType::Long),
                ],
            ],
        };

        let mut buf = Vec::new();
        {
            let mut writer = BitWriter::new(&mut buf);
            si.write(&mut writer, ChannelMode::Mono);
            writer.flush();
        }

        // 9 + 5 + 4 + 2*59 = 136 bits = 17 bytes
        assert_eq!(buf.len(), 17);

        let mut r = BitReader { data: &buf, pos: 0 };

        assert_eq!(r.read_bits(9), 0); // main_data_begin
        assert_eq!(r.read_bits(5), 0); // private_bits (5 for mono)
        assert_eq!(r.read_bits(4), 0); // scfsi[0]

        // gr0/ch0
        assert_eq!(r.read_bits(12), 100);
        assert_eq!(r.read_bits(9), 50);
        assert_eq!(r.read_bits(8), 100);
        assert_eq!(r.read_bits(4), 0);
        assert_eq!(r.read_bits(1), 0);
        assert_eq!(r.read_bits(5), 1);
        assert_eq!(r.read_bits(5), 2);
        assert_eq!(r.read_bits(5), 3);
        assert_eq!(r.read_bits(4), 5);
        assert_eq!(r.read_bits(3), 6);
        assert_eq!(r.read_bits(1), 0);
        assert_eq!(r.read_bits(1), 0);
        assert_eq!(r.read_bits(1), 0);

        // gr1/ch0
        assert_eq!(r.read_bits(12), 200);
        assert_eq!(r.read_bits(9), 100);
        assert_eq!(r.read_bits(8), 200);
        assert_eq!(r.read_bits(4), 0);
        assert_eq!(r.read_bits(1), 0);
        assert_eq!(r.read_bits(5), 4);
        assert_eq!(r.read_bits(5), 5);
        assert_eq!(r.read_bits(5), 6);
        assert_eq!(r.read_bits(4), 7);
        assert_eq!(r.read_bits(3), 6);
        assert_eq!(r.read_bits(1), 0);
        assert_eq!(r.read_bits(1), 0);
        assert_eq!(r.read_bits(1), 0);
    }

    #[test]
    fn side_info_window_switching() {
        let si = SideInfo {
            main_data_begin: 0,
            scfsi: [[false; 4]; 2],
            granules: [
                [
                    make_test_granule(0x100, 0x050, 0x60, [1, 2, 0], 5, 6, BlockType::Short),
                    make_test_granule(0, 0, 0, [0, 0, 0], 0, 0, BlockType::Long),
                ],
                [
                    make_test_granule(0, 0, 0, [0, 0, 0], 0, 0, BlockType::Long),
                    make_test_granule(0, 0, 0, [0, 0, 0], 0, 0, BlockType::Long),
                ],
            ],
        };

        let mut buf = Vec::new();
        {
            let mut writer = BitWriter::new(&mut buf);
            si.write(&mut writer, ChannelMode::Mono);
            writer.flush();
        }

        assert_eq!(buf.len(), 17);

        let mut r = BitReader { data: &buf, pos: 0 };
        r.read_bits(9); // main_data_begin
        r.read_bits(5); // private_bits
        r.read_bits(4); // scfsi

        // gr0/ch0 — flag=1 path
        assert_eq!(r.read_bits(12), 0x100);
        assert_eq!(r.read_bits(9), 0x050);
        assert_eq!(r.read_bits(8), 0x60);
        assert_eq!(r.read_bits(4), 0); // scalefac_compress
        assert_eq!(r.read_bits(1), 1); // window_switching_flag = 1
        assert_eq!(r.read_bits(2), 2); // block_type = Short (10)
        assert_eq!(r.read_bits(1), 0); // mixed_block_flag
        assert_eq!(r.read_bits(5), 1); // table_select[0]
        assert_eq!(r.read_bits(5), 2); // table_select[1]
        assert_eq!(r.read_bits(3), 0); // subblock_gain[0]
        assert_eq!(r.read_bits(3), 0); // subblock_gain[1]
        assert_eq!(r.read_bits(3), 0); // subblock_gain[2]
        assert_eq!(r.read_bits(1), 0); // preflag
        assert_eq!(r.read_bits(1), 0); // scalefac_scale
        assert_eq!(r.read_bits(1), 0); // count1table_select
    }
}
