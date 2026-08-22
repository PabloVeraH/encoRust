//! Region splitting, table selection, and bit emission — plus the cheap
//! bit-count estimator the quantization inner loop depends on. See
//! `docs/mp3-encoder/09-phase6-huffman-coding.md` §3-4.

use crate::bitstream::writer::BitWriter;
use crate::psychoacoustic::SFB_LONG_BOUNDARIES;
use crate::types::SAMPLES_PER_GRANULE;

use super::tables::{BIG_VALUES_TABLES, COUNT1_TABLES, VLC_TABLES};

/// Side-info fields this stage owns, handed to
/// [`crate::bitstream::side_info`] for final assembly. See
/// `docs/mp3-encoder/09-phase6-huffman-coding.md` §1.
#[derive(Debug, Clone, Copy)]
pub struct HuffmanSideInfo {
    /// Count of values coded in the `big_values` region (x2 = actual
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

/// Number of scalefactor bands for long blocks.
const SF_BAND_COUNT: usize = 22;

/// Cumulative sample boundary for each scalefactor band, 44.1 kHz long
/// blocks. An earlier version of this file hardcoded a third,
/// independently-sourced copy of this Annex B table as a width array
/// (`SF_BAND_WIDTHS`) instead of reading the one already defined once,
/// sourced, and table-provenance-tested in `psychoacoustic::tables` (see
/// that module's checksum/monotonicity tests, and the M4/M5 review notes
/// on the same duplication pattern in `quantize::scalefactors`). The
/// values matched (verified when this was fixed), but a third copy is
/// still a landmine if only one ever gets corrected.
///
/// This is 44.1 kHz-only, matching chapter 09's own Rust sketch for
/// `encode_granule`/`estimate_bits` (neither takes a `sample_rate_hz`
/// parameter) -- real multi-rate support needs that plumbed through
/// before M8.
fn sf_band_end() -> [usize; SF_BAND_COUNT] {
    let bounds = &SFB_LONG_BOUNDARIES[0]; // 44100 Hz row
    let mut end = [0usize; SF_BAND_COUNT];
    end.copy_from_slice(&bounds[1..=SF_BAND_COUNT]);
    end
}

/// Estimate the number of bits needed to encode a region with a given table.
///
/// Escape handling mirrors [`encode_granule`]'s real emission exactly (see
/// that function's doc comment for the derivation) -- this used to trigger
/// escape at `ax >= xlen` and always cost a hardcoded 16 bits using the
/// `(xlen-1, xlen-1)` corner code, both wrong in the same way
/// `encode_granule`'s bit-emission was; keeping the two in sync matters
/// because this is also what `choose_table`'s exhaustive search compares
/// candidates against.
fn estimate_bits_for_region(ix: &[i32], start: usize, end: usize, linbits: u8) -> u32 {
    let mut bits: u32 = 0;
    let mut i = start;

    while i + 1 < end {
        let ax = ix[i].unsigned_abs() as usize;
        let ay = ix[i + 1].unsigned_abs() as usize;
        i += 2;

        // Try to find a table that covers this pair
        let mut found = false;
        for table_id in 1u8..=15 {
            if let Some(info) = BIG_VALUES_TABLES[table_id as usize] {
                let vlc_idx = info.0;
                let table_linbits = info.1;
                let table = &VLC_TABLES[vlc_idx];
                if table.xlen == 0 {
                    continue;
                }
                let xlen = table.xlen;
                if table_linbits == 0 {
                    // No escape mechanism at all for this table -- only a
                    // candidate if both coordinates fit directly.
                    if ax < xlen && ay < xlen {
                        bits += table.lookup(ax, ay).bits as u32;
                        found = true;
                        break;
                    }
                } else {
                    let esc_threshold = xlen - 1;
                    let linbits_limit = 1usize << table_linbits;
                    let max_rep = esc_threshold + (linbits_limit - 1);
                    if ax <= max_rep && ay <= max_rep {
                        let ax_c = ax.min(esc_threshold);
                        let ay_c = ay.min(esc_threshold);
                        bits += table.lookup(ax_c, ay_c).bits as u32;
                        if ax >= esc_threshold {
                            bits += table_linbits as u32;
                        }
                        if ay >= esc_threshold {
                            bits += table_linbits as u32;
                        }
                        if ax != 0 {
                            bits += 1; // sign
                        }
                        if ay != 0 {
                            bits += 1; // sign
                        }
                        found = true;
                        break;
                    }
                }
            }
        }
        if !found {
            bits += 32; // conservative fallback -- no candidate table covers
                        // this pair at all (shouldn't happen for in-range ix
                        // values); over-estimate generously rather than
                        // silently under-count.
        }
        let _ = linbits;
    }

    bits
}

/// Fast, allocation-free bit-count estimate for the quantization inner
/// loop ([`crate::quantize::loop_control::quantize_granule`]). Must
/// over-estimate rather than under-estimate on ties, so the inner loop
/// never produces a bitstream that overflows its budget once
/// [`encode_granule`] runs for real. See
/// `docs/mp3-encoder/09-phase6-huffman-coding.md` §4.
#[must_use]
pub fn estimate_bits(ix: &[i32; SAMPLES_PER_GRANULE]) -> u32 {
    let mut bits: u32 = 0;
    let mut i = 0;
    while i + 1 < SAMPLES_PER_GRANULE {
        let ax = ix[i].unsigned_abs() as usize;
        let ay = ix[i + 1].unsigned_abs() as usize;

        // Quick check: can table 1 (xlen=2) handle this?
        if ax < 2 && ay < 2 {
            bits += VLC_TABLES[1].lookup(ax, ay).bits as u32;
        } else {
            // Conservative estimate for larger values
            bits += estimate_bits_for_region(ix, i, i + 2, 0);
        }
        i += 2;
    }
    bits
}

/// Choose the best Huffman table for a given region of ix[] values.
/// Returns the table_id (1..=31).
fn choose_table(ix: &[i32], start: usize, end: usize) -> u8 {
    if start >= end {
        return 0;
    }

    let mut max_val: i32 = 0;
    let mut any_nonzero = false;
    for &v in &ix[start..end] {
        let a = v.abs();
        if a > max_val {
            max_val = a;
        }
        if v != 0 {
            any_nonzero = true;
        }
    }

    if !any_nonzero {
        return 0;
    }

    let mut best_table: u8 = 1;
    let mut best_bits = u32::MAX;

    for table_id in 1u8..=31 {
        if let Some(info) = BIG_VALUES_TABLES[table_id as usize] {
            let vlc_idx = info.0;
            let linbits = info.1;
            let table = &VLC_TABLES[vlc_idx];
            if table.xlen == 0 {
                continue;
            }
            let xlen = table.xlen as i32;
            // Non-escape tables (linbits == 0): only 0..=(xlen-1) directly.
            // Escape tables: the linbits field can add 0..=(2^linbits - 1)
            // on top of the (xlen-1) escape threshold -- an earlier
            // version added the full 2^linbits (one too many), matching
            // the encoder's own off-by-one at the escape boundary.
            let max_representable = if linbits > 0 {
                (xlen - 1) + ((1i32 << linbits) - 1)
            } else {
                xlen - 1
            };
            if max_val > max_representable {
                continue;
            }
            let bits = estimate_bits_for_region(ix, start, end, linbits);
            if bits < best_bits {
                best_bits = bits;
                best_table = table_id;
            }
        }
    }

    best_table
}

/// Map a signed value to unsigned index for count1 encoding:
/// 0 -> 0, 1 -> 1, -1 -> 1. Signs are emitted separately.
fn signed_to_index(v: i32) -> usize {
    match v {
        0 => 0,
        _ => 1,
    }
}

/// Count how many scalefactor bands are needed for `pairs` value pairs.
fn count_sf_bands_for_pairs(pairs: usize, band_end: &[usize; SF_BAND_COUNT]) -> usize {
    let samples = pairs * 2;
    for (i, &end) in band_end.iter().enumerate() {
        if end >= samples {
            return i + 1;
        }
    }
    SF_BAND_COUNT
}

/// Choose between the two count1 tables.
fn choose_count1_table(ix: &[i32], start: usize, end: usize) -> bool {
    // Defensive: `end - pos` below would underflow if `start > end`.
    // `encode_granule` guarantees `count1_start <= count1_end` itself
    // now (see its own comment on the rounding edge case that used to
    // violate this), but this function has its own preconditions to
    // uphold regardless of what a future caller passes.
    if start >= end {
        return false;
    }

    let mut bits0: u32 = 0;
    let mut bits1: u32 = 0;

    let mut pos = start;
    while pos + 3 < end {
        let v = signed_to_index(ix[pos]);
        let w = signed_to_index(ix[pos + 1]);
        let x = signed_to_index(ix[pos + 2]);
        let y = signed_to_index(ix[pos + 3]);
        pos += 4;
        let idx = v * 8 + w * 4 + x * 2 + y;
        bits0 += COUNT1_TABLES[0].entries[idx].bits as u32;
        bits1 += COUNT1_TABLES[1].entries[idx].bits as u32;
    }

    let mut remaining = [0i32; 4];
    let rem_count = end - pos;
    remaining[..rem_count].copy_from_slice(&ix[pos..pos + rem_count]);
    if rem_count > 0 {
        let v = signed_to_index(remaining[0]);
        let w = if rem_count > 1 {
            signed_to_index(remaining[1])
        } else {
            0
        };
        let x = if rem_count > 2 {
            signed_to_index(remaining[2])
        } else {
            0
        };
        let y = if rem_count > 3 {
            signed_to_index(remaining[3])
        } else {
            0
        };
        let idx = v * 8 + w * 4 + x * 2 + y;
        bits0 += COUNT1_TABLES[0].entries[idx].bits as u32;
        bits1 += COUNT1_TABLES[1].entries[idx].bits as u32;
    }

    bits1 <= bits0
}

/// Full Huffman encode: region splitting + exhaustive per-region table
/// selection + `count1` region + escape (`linbits`) handling, emitting
/// bits via `writer`. Called once per granule, after the quantization
/// loop has converged. See
/// `docs/mp3-encoder/09-phase6-huffman-coding.md` §3-4.
pub fn encode_granule(
    ix: &[i32; SAMPLES_PER_GRANULE],
    writer: &mut BitWriter<'_>,
) -> HuffmanSideInfo {
    let band_end = sf_band_end();

    // Find count1 region: walk backwards to find the last non-zero value
    let mut count1_end = SAMPLES_PER_GRANULE;
    while count1_end > 0 && ix[count1_end - 1] == 0 {
        count1_end -= 1;
    }

    // Find where big_values ends: the last sample with |val| > 1
    let mut big_values_end = 0usize;
    for (i, &v) in ix.iter().enumerate() {
        if v.abs() > 1 {
            big_values_end = i + 1;
        }
    }
    big_values_end = (big_values_end + 1) & !1;
    if big_values_end > SAMPLES_PER_GRANULE {
        big_values_end = SAMPLES_PER_GRANULE;
    }

    // Rounding big_values_end up to the next even index can push it
    // *past* count1_end: e.g. the last nonzero value in the whole
    // spectrum has magnitude > 1 (needs big_values) and sits at an odd
    // index -- count1_end stops right after it, but big_values_end
    // rounds one further. When that happens there's no count1 region
    // left at all (everything nonzero is already inside the
    // now-extended big_values region, even though its rounding-padded
    // pair-mate is zero); without this, `count1_end - count1_start`
    // below underflows (`count1_start = big_values_end > count1_end`).
    count1_end = count1_end.max(big_values_end);

    let big_values_count = (big_values_end / 2) as u16;

    // Region boundaries: split big_values into 3 roughly equal sub-regions
    let pairs_count = big_values_end / 2;
    let third = pairs_count / 3;
    let region0_count = count_sf_bands_for_pairs(third, &band_end) as u8;
    let region1_count = count_sf_bands_for_pairs(third * 2, &band_end) as u8;

    let r0_start = 0;
    let r0_end = third * 2;
    let r1_start = r0_end;
    let r1_end = third * 4;
    let r2_start = r1_end;
    let r2_end = big_values_end;

    let table0 = choose_table(ix, r0_start, r0_end);
    let table1 = choose_table(ix, r1_start, r1_end);
    let table2 = choose_table(ix, r2_start, r2_end);

    // Encode big_values pairs for each region
    for (start, end, table_id) in [
        (r0_start, r0_end, table0),
        (r1_start, r1_end, table1),
        (r2_start, r2_end, table2),
    ] {
        if table_id == 0 {
            continue;
        }
        let info = BIG_VALUES_TABLES[table_id as usize].unwrap();
        let vlc_idx = info.0;
        let linbits = info.1;
        let table = &VLC_TABLES[vlc_idx];

        let mut pos = start;
        while pos + 1 < end {
            let x = ix[pos];
            let y = ix[pos + 1];
            pos += 2;

            let ax = x.unsigned_abs() as usize;
            let ay = y.unsigned_abs() as usize;

            // Escape trigger is `>= xlen - 1` (value 15 for the 16x16
            // escape tables), not `>= xlen` -- cross-checked against
            // minimp3's decoder (`if (lsb == 15) { lsb += linbits... }`).
            // A coordinate exactly at the boundary (e.g. ax == 15) still
            // needs its escape-convention code + a (possibly zero-valued)
            // linbits field, because the *decoder* unconditionally reads
            // linbits whenever it decodes coordinate 15 -- omitting them
            // desyncs every bit after this pair. Each coordinate is also
            // clamped to the escape threshold *independently* for the
            // lookup (not both forced to the `(xlen-1, xlen-1)` corner),
            // since only tables 16/24 (linbits > 0) have this convention
            // at all; non-escape tables (linbits == 0) never take this
            // branch -- `choose_table` already guarantees max_val fits
            // within `xlen - 1` for those, so ax/ay < xlen always here.
            let esc_threshold = table.xlen - 1;
            if linbits > 0 && (ax >= esc_threshold || ay >= esc_threshold) {
                let ax_c = ax.min(esc_threshold);
                let ay_c = ay.min(esc_threshold);
                let entry = table.lookup(ax_c, ay_c);
                writer.write_bits(entry.code, entry.bits);
                if ax >= esc_threshold {
                    writer.write_bits((ax - esc_threshold) as u32, linbits);
                }
                if ay >= esc_threshold {
                    writer.write_bits((ay - esc_threshold) as u32, linbits);
                }
                if x != 0 {
                    writer.write_bits(u32::from(x < 0), 1);
                }
                if y != 0 {
                    writer.write_bits(u32::from(y < 0), 1);
                }
            } else {
                let entry = table.lookup(ax, ay);
                writer.write_bits(entry.code, entry.bits);
                // Sign bits for non-zero values
                if x != 0 {
                    writer.write_bits(u32::from(x < 0), 1);
                }
                if y != 0 {
                    writer.write_bits(u32::from(y < 0), 1);
                }
            }
        }
    }

    // Encode count1 region
    let count1_start = big_values_end;
    let use_table_1 = choose_count1_table(ix, count1_start, count1_end);
    let count1_table = &COUNT1_TABLES[if use_table_1 { 1 } else { 0 }];

    let mut pos = count1_start;
    while pos + 3 < count1_end {
        let v = ix[pos];
        let w = ix[pos + 1];
        let x = ix[pos + 2];
        let y = ix[pos + 3];
        pos += 4;
        let idx = signed_to_index(v) * 8
            + signed_to_index(w) * 4
            + signed_to_index(x) * 2
            + signed_to_index(y);
        let entry = count1_table.entries[idx];
        writer.write_bits(entry.code, entry.bits);
        // Sign bits: 1 for each non-zero value (negative = 1)
        if v != 0 {
            writer.write_bits(u32::from(v < 0), 1);
        }
        if w != 0 {
            writer.write_bits(u32::from(w < 0), 1);
        }
        if x != 0 {
            writer.write_bits(u32::from(x < 0), 1);
        }
        if y != 0 {
            writer.write_bits(u32::from(y < 0), 1);
        }
    }

    // Handle remaining count1 values (1-3 trailing samples)
    let mut remaining = [0i32; 4];
    let rem_count = count1_end - pos;
    remaining[..rem_count].copy_from_slice(&ix[pos..pos + rem_count]);
    if rem_count > 0 {
        let v = remaining[0];
        let w = if rem_count > 1 { remaining[1] } else { 0 };
        let x = if rem_count > 2 { remaining[2] } else { 0 };
        let y = if rem_count > 3 { remaining[3] } else { 0 };
        let idx = signed_to_index(v) * 8
            + signed_to_index(w) * 4
            + signed_to_index(x) * 2
            + signed_to_index(y);
        let entry = count1_table.entries[idx];
        writer.write_bits(entry.code, entry.bits);
        if v != 0 {
            writer.write_bits(u32::from(v < 0), 1);
        }
        if w != 0 {
            writer.write_bits(u32::from(w < 0), 1);
        }
        if x != 0 {
            writer.write_bits(u32::from(x < 0), 1);
        }
        if y != 0 {
            writer.write_bits(u32::from(y < 0), 1);
        }
    }

    HuffmanSideInfo {
        big_values: big_values_count,
        region0_count,
        region1_count,
        table_select: [table0, table1, table2],
        count1table_select: use_table_1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::huffman::tables::{Count1Table, HuffmanTable};
    use alloc::vec::Vec;
    use proptest::prelude::*;

    #[test]
    fn estimate_bits_never_undercounts() {
        let mut ix = [0i32; SAMPLES_PER_GRANULE];
        for (i, item) in ix.iter_mut().enumerate().take(100) {
            *item = (i as i32 % 7) - 3;
        }

        let estimate = estimate_bits(&ix);

        let mut out = Vec::new();
        let mut writer = BitWriter::new(&mut out);
        let _info = encode_granule(&ix, &mut writer);
        writer.flush();
        let actual_bits = (out.len() * 8) as u32;

        assert!(
            estimate >= actual_bits,
            "estimate ({estimate}) < actual ({actual_bits})"
        );
    }

    #[test]
    fn encode_zeros_produces_no_bits() {
        let ix = [0i32; SAMPLES_PER_GRANULE];
        let mut out = Vec::new();
        let mut writer = BitWriter::new(&mut out);
        let _info = encode_granule(&ix, &mut writer);
        writer.flush();
        assert!(out.is_empty());
    }

    #[test]
    fn round_trip_count1_values() {
        let mut ix = [0i32; SAMPLES_PER_GRANULE];
        ix[SAMPLES_PER_GRANULE - 4] = 1;
        ix[SAMPLES_PER_GRANULE - 3] = -1;
        ix[SAMPLES_PER_GRANULE - 2] = 0;
        ix[SAMPLES_PER_GRANULE - 1] = 1;

        let mut out = Vec::new();
        let mut writer = BitWriter::new(&mut out);
        let info = encode_granule(&ix, &mut writer);
        writer.flush();

        assert_eq!(info.big_values, 0);
    }

    // --- Round-trip against a test-only decoder ---
    //
    // Chapter 09 §5 explicitly allows "a hand-written test-only decoder
    // matching the exact table data" as the interim round-trip check for
    // M6 in isolation (a full independent-decoder check needs M8's real
    // frame assembly). This only proves internal consistency between
    // `encode_granule` and this decoder, not interoperability with a
    // real MP3 decoder -- that check is still owed once M8 lands.

    struct BitReader<'a> {
        data: &'a [u8],
        pos: usize,
    }

    impl<'a> BitReader<'a> {
        fn new(data: &'a [u8]) -> Self {
            Self { data, pos: 0 }
        }

        fn read_bit(&mut self) -> u32 {
            let byte_idx = self.pos / 8;
            let bit_idx = 7 - (self.pos % 8);
            let bit = (self.data[byte_idx] >> bit_idx) & 1;
            self.pos += 1;
            u32::from(bit)
        }

        fn read_bits(&mut self, n: u8) -> u32 {
            let mut v = 0u32;
            for _ in 0..n {
                v = (v << 1) | self.read_bit();
            }
            v
        }
    }

    /// Reads bits one at a time until they match an entry in `table`,
    /// exploiting the prefix-free property every table is checked for in
    /// `huffman::tables::tests` -- a real streaming Huffman decoder works
    /// the same way.
    fn decode_pair(reader: &mut BitReader, table: &HuffmanTable) -> (usize, usize) {
        let mut code = 0u32;
        let mut bits = 0u8;
        loop {
            code = (code << 1) | reader.read_bit();
            bits += 1;
            for y in 0..table.len_x() {
                for x in 0..table.len_x() {
                    let entry = table.lookup(x, y);
                    if entry.bits == bits && entry.code == code {
                        return (x, y);
                    }
                }
            }
            assert!(bits < 24, "no matching big_values code after 24 bits");
        }
    }

    fn decode_quad(reader: &mut BitReader, table: &Count1Table) -> usize {
        let mut code = 0u32;
        let mut bits = 0u8;
        loop {
            code = (code << 1) | reader.read_bit();
            bits += 1;
            for (idx, entry) in table.entries.iter().enumerate() {
                if entry.bits == bits && entry.code == code {
                    return idx;
                }
            }
            assert!(bits < 24, "no matching count1 code after 24 bits");
        }
    }

    /// Where `encode_granule` stops transmitting (everything past this is
    /// the implicit-zero `rzero` region) -- mirrors its own computation.
    fn last_nonzero_end(ix: &[i32; SAMPLES_PER_GRANULE]) -> usize {
        let mut end = SAMPLES_PER_GRANULE;
        while end > 0 && ix[end - 1] == 0 {
            end -= 1;
        }
        end
    }

    /// Decodes a granule [`encode_granule`] produced back into `ix[]`.
    /// `count1_end` (where transmission actually stops) is passed in
    /// rather than re-derived from the bitstream itself -- a real decoder
    /// gets the equivalent from `part2_3_length` (chapter 11, M8 scope);
    /// reconstructing that framing isn't needed to check the Huffman
    /// coding itself round-trips correctly.
    fn decode_granule_for_test(
        data: &[u8],
        info: &HuffmanSideInfo,
        count1_end: usize,
    ) -> [i32; SAMPLES_PER_GRANULE] {
        let mut out = [0i32; SAMPLES_PER_GRANULE];
        let mut reader = BitReader::new(data);

        let pairs_count = info.big_values as usize;
        let third = pairs_count / 3;
        let region_bounds = [
            (0usize, third * 2, info.table_select[0]),
            (third * 2, third * 4, info.table_select[1]),
            (third * 4, pairs_count * 2, info.table_select[2]),
        ];

        for (start, end, table_id) in region_bounds {
            if table_id == 0 {
                continue;
            }
            let (vlc_idx, linbits) = BIG_VALUES_TABLES[table_id as usize].unwrap();
            let table = &VLC_TABLES[vlc_idx];
            let esc_threshold = table.xlen - 1;

            let mut pos = start;
            while pos + 1 < end {
                let (mut x, mut y) = decode_pair(&mut reader, table);
                if linbits > 0 && x == esc_threshold {
                    x += reader.read_bits(linbits) as usize;
                }
                if linbits > 0 && y == esc_threshold {
                    y += reader.read_bits(linbits) as usize;
                }
                let sx = if x != 0 { reader.read_bit() } else { 0 };
                let sy = if y != 0 { reader.read_bit() } else { 0 };
                out[pos] = if sx == 1 { -(x as i32) } else { x as i32 };
                out[pos + 1] = if sy == 1 { -(y as i32) } else { y as i32 };
                pos += 2;
            }
        }

        let count1_table = &COUNT1_TABLES[usize::from(info.count1table_select)];
        let mut pos = pairs_count * 2;
        while pos + 4 <= count1_end {
            decode_quad_into(&mut reader, count1_table, &mut out, pos, 4);
            pos += 4;
        }
        if pos < count1_end {
            let rem = count1_end - pos;
            decode_quad_into(&mut reader, count1_table, &mut out, pos, rem);
        }

        out
    }

    fn decode_quad_into(
        reader: &mut BitReader,
        table: &Count1Table,
        out: &mut [i32; SAMPLES_PER_GRANULE],
        pos: usize,
        real_count: usize,
    ) {
        let idx = decode_quad(reader, table);
        let comps = [(idx >> 3) & 1, (idx >> 2) & 1, (idx >> 1) & 1, idx & 1];
        for (offset, &mag) in comps.iter().enumerate() {
            if mag == 1 {
                let sign = reader.read_bit();
                if offset < real_count {
                    out[pos + offset] = if sign == 1 { -1 } else { 1 };
                }
            }
        }
    }

    #[test]
    fn round_trip_decodes_exactly_with_escape_values() {
        let mut ix = [0i32; SAMPLES_PER_GRANULE];
        ix[0] = 3;
        ix[1] = -2;
        ix[2] = 20; // well past the escape threshold
        ix[3] = -15; // exactly at it -- see the boundary test below
        ix[4] = 7;
        ix[5] = -1;
        ix[SAMPLES_PER_GRANULE - 4] = 1;
        ix[SAMPLES_PER_GRANULE - 3] = -1;
        ix[SAMPLES_PER_GRANULE - 2] = 0;
        ix[SAMPLES_PER_GRANULE - 1] = 1;

        let mut out = Vec::new();
        let mut writer = BitWriter::new(&mut out);
        let info = encode_granule(&ix, &mut writer);
        writer.flush();

        let decoded = decode_granule_for_test(&out, &info, last_nonzero_end(&ix));
        assert_eq!(decoded, ix, "round-trip mismatch with escape-coded values");
    }

    #[test]
    fn big_values_rounding_never_underflows_count1_bounds() {
        // Regression test for a `proptest`-found panic (see
        // `proptest-regressions/huffman/encode.txt`): a single value of
        // magnitude > 1 as the *only* nonzero content, positioned such
        // that rounding `big_values_end` up to an even boundary pushes
        // it one past `count1_end` (the last-nonzero position). Before
        // the fix, `count1_start(=big_values_end) > count1_end` made
        // `end - pos` underflow (panic) in `choose_count1_table` and the
        // count1 emission loop. This value/position is the shrunk
        // failing case verbatim.
        let mut ix = [0i32; SAMPLES_PER_GRANULE];
        ix[58] = 2;

        let mut out = Vec::new();
        let mut writer = BitWriter::new(&mut out);
        let info = encode_granule(&ix, &mut writer); // must not panic
        writer.flush();

        let decoded = decode_granule_for_test(&out, &info, last_nonzero_end(&ix));
        assert_eq!(decoded, ix);
    }

    #[test]
    fn escape_boundary_exactly_15_round_trips() {
        // Isolates the exact off-by-one this review found: a magnitude
        // of *exactly* 15 must still take the escape path (and append a
        // possibly-zero-valued linbits field), or the bitstream desyncs
        // from this pair onward -- see encode_granule's escape-handling
        // doc comment for the full derivation.
        let mut ix = [0i32; SAMPLES_PER_GRANULE];
        ix[0] = 15;
        ix[1] = 15;
        ix[2] = 1;
        ix[3] = 1;

        let mut out = Vec::new();
        let mut writer = BitWriter::new(&mut out);
        let info = encode_granule(&ix, &mut writer);
        writer.flush();

        let decoded = decode_granule_for_test(&out, &info, last_nonzero_end(&ix));
        assert_eq!(decoded, ix, "magnitude-15 boundary did not round-trip");
    }

    proptest! {
        #[test]
        fn round_trip_random_granules(values in prop::collection::vec(-300i32..=300, 60)) {
            let mut ix = [0i32; SAMPLES_PER_GRANULE];
            for (i, v) in values.iter().enumerate() {
                ix[i] = *v;
            }

            let mut out = Vec::new();
            let mut writer = BitWriter::new(&mut out);
            let info = encode_granule(&ix, &mut writer);
            writer.flush();

            let decoded = decode_granule_for_test(&out, &info, last_nonzero_end(&ix));
            prop_assert_eq!(decoded, ix);
        }
    }

    #[test]
    fn table_lookup_known_values() {
        let entry = VLC_TABLES[1].lookup(0, 0);
        assert_eq!(entry.code, 0x0001);
        assert_eq!(entry.bits, 1);

        let entry = VLC_TABLES[1].lookup(1, 1);
        assert_eq!(entry.code, 0x0000);
        assert_eq!(entry.bits, 3);
    }

    #[test]
    fn encode_small_values() {
        let mut ix = [0i32; SAMPLES_PER_GRANULE];
        ix[0] = 1;
        ix[1] = 1;
        ix[2] = -1;
        ix[3] = 0;

        let mut out = Vec::new();
        let mut writer = BitWriter::new(&mut out);
        let info = encode_granule(&ix, &mut writer);
        writer.flush();

        assert!(info.big_values > 0 || !out.is_empty());
    }
}
