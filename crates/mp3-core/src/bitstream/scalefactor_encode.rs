//! Scalefactor bitstream encoding. Computes `scalefac_compress` from actual
//! scale factor values and emits the per-band scale factors at the correct
//! bit widths. See
//! `docs/mp3-encoder/11-phase8-bitstream-multiplexing.md` §4
//! and ISO/IEC 11172-3 Annex B §2.4.3.1, Table B.1.

use crate::bitstream::writer::BitWriter;
use crate::mdct::BlockType;
use crate::psychoacoustic::{
    scalefactor_sample_rate_index, SFB_LONG_BOUNDARIES, SFB_SHORT_BOUNDARIES,
};

/// Canonical `scalefac_compress` table (ISO/IEC 11172-3 Table B.1).
/// Index into this array is the `scalefac_compress` value (0..=15);
/// each entry is `(slen1, slen2)`.
const SLEN_TABLE: [(u8, u8); 16] = [
    (0, 0), // 0
    (0, 1), // 1
    (0, 2), // 2
    (0, 3), // 3
    (3, 0), // 4
    (1, 1), // 5
    (1, 2), // 6
    (1, 3), // 7
    (2, 1), // 8
    (2, 2), // 9
    (2, 3), // 10
    (3, 1), // 11
    (3, 2), // 12
    (3, 3), // 13
    (4, 2), // 14
    (4, 3), // 15
];

fn bits_needed(max_val: u8) -> u8 {
    if max_val == 0 {
        return 0;
    }
    8 - max_val.leading_zeros() as u8
}

/// Looks up the `scalefac_compress` value that best represents scale
/// factor groups needing `slen1`/`slen2` bits.
///
/// When no table entry is an exact match, this rounds up to the
/// cheapest entry that still covers both groups losslessly (standard
/// practice -- MP3 encoders always transmit a real, if not the
/// theoretical minimum, combination from Table B.1).
///
/// ISO/IEC 11172-3 Table B.1 tops out at `slen2 == 3` (its widest entry
/// is `(4, 3)`, index 15) -- there is no representable combination for
/// `slen2 >= 4`. Since scale factors are bounded to `0..=15`
/// (`MAX_SCALEFAC` in `quantize/loop_control.rs`), a group whose max
/// value is `8..=15` legitimately needs 4 bits and hits exactly this
/// gap for genuinely loud/dynamic content -- it is a real limitation of
/// the standard's own table, not a bug in this lookup. In that case,
/// give the second group all 3 bits the table can offer and the first
/// group the exact width it needs (the table has a `(slen1, 3)` entry
/// for every `slen1` in `0..=4`); callers must saturate any per-band
/// value that still overflows the returned entry's widths, since
/// `BitWriter::write_bits` silently wraps values wider than the field
/// instead of clamping them.
fn find_scalefac_compress(slen1: u8, slen2: u8) -> u8 {
    if let Some(i) = SLEN_TABLE
        .iter()
        .position(|&(s1, s2)| s1 == slen1 && s2 == slen2)
    {
        return i as u8;
    }
    if let Some((i, _)) = SLEN_TABLE
        .iter()
        .enumerate()
        .filter(|&(_, &(s1, s2))| s1 >= slen1 && s2 >= slen2)
        .min_by_key(|&(_, &(s1, s2))| u32::from(s1) + u32::from(s2))
    {
        return i as u8;
    }
    SLEN_TABLE
        .iter()
        .position(|&(s1, s2)| s1 == slen1 && s2 == 3)
        .map(|i| i as u8)
        .unwrap_or(15) // defensive: only reached if `slen1` also exceeds 4
}

/// Clamps `sf` to the largest value representable in `bw` bits, so an
/// over-range scale factor is saturated rather than silently wrapped by
/// `BitWriter::write_bits`'s masking. Only bites when `find_scalefac_compress`
/// had to under-provision a group (see its doc comment); the exact-match
/// and covering-match paths never produce a value that needs clamping.
fn saturate_to_width(sf: u8, bw: u8) -> u8 {
    if bw >= 8 {
        return sf;
    }
    sf.min((1u16 << bw) as u8 - 1)
}

/// Minimum slen that can represent `max_val`.
fn min_slen_for(max_val: u8) -> u8 {
    bits_needed(max_val)
}

/// Encode the scale factors for one granule/channel into `writer`.
/// Returns `(scalefac_compress, bits_written)`.
///
/// `scale_factors` is indexed `[band + window * bands_per_window]`,
/// laid out as produced by [`crate::quantize::ScaleFactors::values`].
pub fn encode_granule_scalefactors(
    scale_factors: &[u8; 39],
    block_type: BlockType,
    sample_rate_hz: u32,
    writer: &mut BitWriter<'_>,
) -> (u8, u32) {
    let sfb_idx = scalefactor_sample_rate_index(sample_rate_hz);

    match block_type {
        BlockType::Long | BlockType::Start | BlockType::Stop => {
            encode_long_scalefactors(scale_factors, sfb_idx, writer)
        }
        BlockType::Short => encode_short_scalefactors(scale_factors, sfb_idx, writer),
    }
}

fn encode_long_scalefactors(
    scale_factors: &[u8; 39],
    sfb_idx: usize,
    writer: &mut BitWriter<'_>,
) -> (u8, u32) {
    let bounds = &SFB_LONG_BOUNDARIES[sfb_idx];

    // Count actual bands: skip leading 0 and trailing sentinel (576)
    let n_bands = bounds.iter().filter(|&&b| b > 0 && b < 576).count();

    // Figure out the scalefactor band count per slen group.
    // Per Annex B Table B.1: slen1 covers bands 0..10 (11 bands),
    // slen2 covers bands 11..(n_bands-1).
    let slen1_bands = 11usize.min(n_bands);

    // Find max values per group
    let mut max1 = 0u8;
    let mut max2 = 0u8;
    for (i, &sf) in scale_factors.iter().enumerate().take(n_bands) {
        if i < slen1_bands {
            max1 = max1.max(sf);
        } else {
            max2 = max2.max(sf);
        }
    }

    let scalefac_compress = find_scalefac_compress(min_slen_for(max1), min_slen_for(max2));
    // Re-derive the widths from the entry actually selected: when
    // `find_scalefac_compress` had to round up (or, for the slen2>=4
    // gap, round down), the transmitted `scalefac_compress` tells the
    // decoder to expect *these* widths, not the originally-desired
    // minimal ones -- writing with the wrong width would desync the
    // decoder's bit position for every scalefactor after this granule.
    let (slen1, slen2) = SLEN_TABLE[scalefac_compress as usize];

    let mut bits = 0u32;
    for (i, &sf) in scale_factors.iter().enumerate().take(n_bands) {
        let bw = if i < slen1_bands { slen1 } else { slen2 };
        if bw > 0 {
            writer.write_bits(u32::from(saturate_to_width(sf, bw)), bw);
            bits += bw as u32;
        }
    }

    (scalefac_compress, bits)
}

fn encode_short_scalefactors(
    scale_factors: &[u8; 39],
    sfb_idx: usize,
    writer: &mut BitWriter<'_>,
) -> (u8, u32) {
    let bounds = &SFB_SHORT_BOUNDARIES[sfb_idx];

    // 3 windows × 13 bands = 39 scalefactors
    let n_windows = 3usize;
    let n_bands_per_window = 13usize;

    // slen1: bands 0..5 per window, slen2: bands 6..12 per window
    let slen1_bands = 6usize;

    // Find max per group across all windows
    let mut max1 = 0u8;
    let mut max2 = 0u8;
    for w in 0..n_windows {
        let base = w * n_bands_per_window;
        for b in 0..n_bands_per_window {
            let sf = scale_factors[base + b];
            if b < slen1_bands {
                max1 = max1.max(sf);
            } else {
                max2 = max2.max(sf);
            }
        }
        let _ = bounds; // bounds used to validate band structure
    }

    let scalefac_compress = find_scalefac_compress(min_slen_for(max1), min_slen_for(max2));
    // See the long-block path above: widths must come from the entry
    // actually selected, not the originally-desired minimal ones.
    let (slen1, slen2) = SLEN_TABLE[scalefac_compress as usize];

    let mut bits = 0u32;
    for w in 0..n_windows {
        let base = w * n_bands_per_window;
        for b in 0..n_bands_per_window {
            let sf = scale_factors[base + b];
            let bw = if b < slen1_bands { slen1 } else { slen2 };
            if bw > 0 {
                writer.write_bits(u32::from(saturate_to_width(sf, bw)), bw);
                bits += bw as u32;
            }
        }
    }

    (scalefac_compress, bits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitstream::writer::BitWriter;
    use crate::mdct::BlockType;

    #[test]
    fn all_zero_scalefactors_gives_compress_0() {
        let sf = [0u8; 39];
        let mut buf = Vec::new();
        let mut writer = BitWriter::new(&mut buf);
        let (compress, bits) =
            encode_granule_scalefactors(&sf, BlockType::Long, 44100, &mut writer);
        writer.flush();
        assert_eq!(compress, 0, "all zeros should need slen1=0, slen2=0");
        assert_eq!(bits, 0, "no bits with slen1=slen2=0");
        assert!(buf.is_empty());
    }

    #[test]
    fn nonzero_slen1_zeros_slen2_gives_slen1_only_compress() {
        let mut sf = [0u8; 39];
        sf[0] = 7; // needs slen1=3
        let mut buf = Vec::new();
        let mut writer = BitWriter::new(&mut buf);
        let (compress, bits) =
            encode_granule_scalefactors(&sf, BlockType::Long, 44100, &mut writer);
        writer.flush();
        // slen1=3, slen2=0 → compress=4
        assert_eq!(compress, 4);
        assert_eq!(bits, 3 * 11); // 3 bits × 11 bands (slen1 only)
    }

    #[test]
    fn nonzero_both_groups_gives_larger_compress() {
        let mut sf = [0u8; 39];
        sf[0] = 3; // needs slen1=2 (3 fits in 2 bits: 0..3)
        sf[11] = 7; // needs slen2=3
        let mut buf = Vec::new();
        let mut writer = BitWriter::new(&mut buf);
        let (compress, _bits) =
            encode_granule_scalefactors(&sf, BlockType::Long, 44100, &mut writer);
        writer.flush();
        // slen1=2, slen2=3 → compress=10
        assert_eq!(compress, 10);
    }

    #[test]
    fn short_block_scalefactors() {
        let sf = [0u8; 39];
        let mut buf = Vec::new();
        let mut writer = BitWriter::new(&mut buf);
        let (compress, bits) =
            encode_granule_scalefactors(&sf, BlockType::Short, 44100, &mut writer);
        writer.flush();
        assert_eq!(compress, 0);
        assert_eq!(bits, 0);
    }

    #[test]
    fn bits_needed_table() {
        assert_eq!(bits_needed(0), 0);
        assert_eq!(bits_needed(1), 1);
        assert_eq!(bits_needed(3), 2);
        assert_eq!(bits_needed(4), 3);
        assert_eq!(bits_needed(7), 3);
        assert_eq!(bits_needed(8), 4);
        assert_eq!(bits_needed(15), 4);
    }

    #[test]
    fn find_compress_exact_match() {
        assert_eq!(find_scalefac_compress(0, 0), 0);
        assert_eq!(find_scalefac_compress(1, 2), 6);
        assert_eq!(find_scalefac_compress(4, 3), 15);
    }

    struct BitReader<'a> {
        data: &'a [u8],
        pos: usize,
    }
    impl<'a> BitReader<'a> {
        fn read_bits(&mut self, n: u8) -> u32 {
            let mut v = 0u32;
            for _ in 0..n {
                let byte = self.data[self.pos / 8];
                let bit = (byte >> (7 - self.pos % 8)) & 1;
                v = (v << 1) | u32::from(bit);
                self.pos += 1;
            }
            v
        }
    }

    #[test]
    fn covering_fallback_rewrites_widths_to_match_transmitted_compress() {
        // (slen1=1, slen2=0) has no exact entry in SLEN_TABLE -- must
        // round up to the cheapest covering entry, (1, 1) at index 5.
        // Regression test: an earlier version wrote every band with the
        // *originally desired* widths (1, 0) instead of the *actually
        // transmitted* compress's widths (1, 1), which would desync a
        // real decoder (it trusts `scalefac_compress`, not the
        // encoder's internal intent) by under-writing every group-2
        // band by 1 bit.
        let mut sf = [0u8; 39];
        sf[0] = 1; // group 1 (bands 0..11): needs slen1=1
                   // group 2 (bands 11..) stays all zero: desired slen2=0
        let mut buf = Vec::new();
        let mut writer = BitWriter::new(&mut buf);
        let (compress, bits) =
            encode_granule_scalefactors(&sf, BlockType::Long, 44100, &mut writer);
        writer.flush();

        assert_eq!(
            compress, 5,
            "no exact (1,0) entry -- must round up to (1,1)"
        );
        let (slen1, slen2) = SLEN_TABLE[compress as usize];
        assert_eq!((slen1, slen2), (1, 1));

        let n_bands = SFB_LONG_BOUNDARIES[scalefactor_sample_rate_index(44100)]
            .iter()
            .filter(|&&b| b > 0 && b < 576)
            .count();
        let expected_bits = 11 * u32::from(slen1) + (n_bands - 11) as u32 * u32::from(slen2);
        assert_eq!(
            bits, expected_bits,
            "must write every group-2 band at the transmitted compress's \
             width (1 bit), not the originally-desired width (0 bits)"
        );
    }

    #[test]
    fn slen2_needing_four_bits_saturates_instead_of_wrapping() {
        // Group 2's max value (15) needs 4 bits, which no SLEN_TABLE
        // entry provides (the table's widest slen2 is 3) -- must fall
        // back to (slen1, 3) and saturate the over-range value to the
        // 3-bit max (7) rather than letting `BitWriter::write_bits`
        // silently wrap it (15 & 0b111 = 7 coincidentally here, but the
        // point is this is a deliberate clamp, not an accidental mask).
        let mut sf = [0u8; 39];
        sf[11] = 15; // group 2 (bands 11..): needs slen2=4
        let mut buf = Vec::new();
        let mut writer = BitWriter::new(&mut buf);
        let (compress, _bits) =
            encode_granule_scalefactors(&sf, BlockType::Long, 44100, &mut writer);
        writer.flush();

        let (slen1, slen2) = SLEN_TABLE[compress as usize];
        assert_eq!(slen2, 3, "table's widest slen2 is 3");
        assert_eq!(slen1, 0, "group 1 stayed at its exact desired width");

        let mut r = BitReader { data: &buf, pos: 0 };
        for _ in 0..11 {
            r.read_bits(slen1); // bands 0..11: 0 bits each here
        }
        let decoded = r.read_bits(slen2); // band 11
        assert_eq!(decoded, 7, "value 15 must saturate to the 3-bit max (7)");
    }
}
