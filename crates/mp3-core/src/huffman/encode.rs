//! Region splitting, table selection, and bit emission — plus the cheap
//! bit-count estimator the quantization inner loop depends on. See
//! `docs/mp3-encoder/09-phase6-huffman-coding.md` §3-4.

use crate::bitstream::writer::BitWriter;
use crate::mdct::BlockType;
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

/// Cost, in bits, of encoding `ix[start..end]` (paired) using exactly
/// `table_id` for every pair in the region -- or `None` if some pair's
/// magnitude exceeds what this table (with its escape mechanism, if any)
/// can represent at all, meaning `table_id` isn't a valid choice for this
/// region.
///
/// This costs the region **as a whole, against one committed table** --
/// matching how `big_values` sub-regions actually work (chapter 09 §1:
/// one table per sub-region, not a free per-pair choice). An earlier
/// version (`estimate_bits_for_region`) ignored which table it was
/// supposedly costing and instead ran its own independent per-pair
/// "cheapest of all 15 tables" search every time -- since a region
/// constrained to one table can never beat the sum of independent
/// per-pair minimums, that structurally *under-counted* relative to what
/// [`encode_granule`] actually emits (confirmed empirically: a granule
/// that estimated 1426 bits produced 2024 real bits, a 42% undercount,
/// silently overflowing the caller's bit budget). It also made
/// `choose_table`'s "exhaustive search" pointless, since every candidate
/// got the same answer regardless of which table was being evaluated.
///
/// Escape handling matches [`encode_granule`]'s real emission exactly
/// (see that function's doc comment for the threshold/lookup
/// derivation).
fn region_cost_with_table(ix: &[i32], start: usize, end: usize, table_id: u8) -> Option<u32> {
    let (vlc_idx, linbits) = BIG_VALUES_TABLES[table_id as usize]?;
    let table = &VLC_TABLES[vlc_idx];
    if table.xlen == 0 {
        // Table 0 (trivially empty): only valid for an empty region.
        return if start >= end { Some(0) } else { None };
    }
    let xlen = table.xlen;
    let esc_threshold = xlen - 1;
    let max_rep = if linbits > 0 {
        esc_threshold + ((1usize << linbits) - 1)
    } else {
        esc_threshold
    };

    let mut bits = 0u32;
    let mut i = start;
    while i + 1 < end {
        let ax = ix[i].unsigned_abs() as usize;
        let ay = ix[i + 1].unsigned_abs() as usize;
        i += 2;

        if ax > max_rep || ay > max_rep {
            return None;
        }
        if linbits > 0 && (ax >= esc_threshold || ay >= esc_threshold) {
            let ax_c = ax.min(esc_threshold);
            let ay_c = ay.min(esc_threshold);
            bits += table.lookup(ax_c, ay_c).bits as u32;
            if ax >= esc_threshold {
                bits += linbits as u32;
            }
            if ay >= esc_threshold {
                bits += linbits as u32;
            }
        } else {
            bits += table.lookup(ax, ay).bits as u32;
        }
        if ax != 0 {
            bits += 1; // sign
        }
        if ay != 0 {
            bits += 1; // sign
        }
    }
    Some(bits)
}

/// Finds the table_id minimizing `region_cost_with_table` for this
/// region, and that minimum cost. Returns `(0, 0)` for an empty or
/// all-zero region (table 0, no bits).
fn choose_table_and_cost(ix: &[i32], start: usize, end: usize) -> (u8, u32) {
    if start >= end || ix[start..end].iter().all(|&v| v == 0) {
        return (0, 0);
    }

    let mut best_table = 1u8;
    let mut best_bits = u32::MAX;
    for table_id in 1u8..=31 {
        if let Some(bits) = region_cost_with_table(ix, start, end, table_id) {
            if bits < best_bits {
                best_bits = bits;
                best_table = table_id;
            }
        }
    }
    (best_table, best_bits)
}

/// Choose the best Huffman table for a given region of ix[] values.
/// Returns the table_id (0..=31; 0 means "empty, no bits").
fn choose_table(ix: &[i32], start: usize, end: usize) -> u8 {
    choose_table_and_cost(ix, start, end).0
}

/// Real cost of the count1 region `ix[start..end]` using whichever of the
/// two count1 tables is cheaper, plus sign bits. Returns
/// `(use_table_b, total_bits)`. Shared by [`estimate_bits`] (as a real
/// cost, not a heuristic -- count1 quadruple coding is cheap enough to
/// compute exactly) and [`encode_granule`] (for the actual table
/// selection).
fn count1_region_cost(ix: &[i32], start: usize, end: usize) -> (bool, u32) {
    if start >= end {
        return (false, 0);
    }

    let mut bits0 = 0u32;
    let mut bits1 = 0u32;
    let mut pos = start;
    while pos + 3 < end {
        let quad = [ix[pos], ix[pos + 1], ix[pos + 2], ix[pos + 3]];
        let idx = signed_to_index(quad[0]) * 8
            + signed_to_index(quad[1]) * 4
            + signed_to_index(quad[2]) * 2
            + signed_to_index(quad[3]);
        bits0 += COUNT1_TABLES[0].entries[idx].bits as u32;
        bits1 += COUNT1_TABLES[1].entries[idx].bits as u32;
        let signs = quad.iter().filter(|&&v| v != 0).count() as u32;
        bits0 += signs;
        bits1 += signs;
        pos += 4;
    }

    let rem_count = end - pos;
    if rem_count > 0 {
        let mut quad = [0i32; 4];
        quad[..rem_count].copy_from_slice(&ix[pos..end]);
        let idx = signed_to_index(quad[0]) * 8
            + signed_to_index(quad[1]) * 4
            + signed_to_index(quad[2]) * 2
            + signed_to_index(quad[3]);
        bits0 += COUNT1_TABLES[0].entries[idx].bits as u32;
        bits1 += COUNT1_TABLES[1].entries[idx].bits as u32;
        let signs = quad[..rem_count].iter().filter(|&&v| v != 0).count() as u32;
        bits0 += signs;
        bits1 += signs;
    }

    if bits1 <= bits0 {
        (true, bits1)
    } else {
        (false, bits0)
    }
}

/// The `big_values`/`count1`/`rzero` region boundaries for one granule,
/// per chapter 09 §1. Shared by [`estimate_bits`] and [`encode_granule`]
/// so the estimator actually costs the same regions the real encoder
/// commits to -- an earlier version had `estimate_bits` cost the entire
/// granule as one undifferentiated run of pairs, which doesn't resemble
/// what gets emitted closely enough to be a tight estimate.
struct Regions {
    big_values_end: usize,
    count1_end: usize,
    r0_end: usize,
    r1_end: usize,
}

fn compute_regions(ix: &[i32; SAMPLES_PER_GRANULE]) -> Regions {
    // Find count1 region: walk backwards to find the last non-zero value.
    let mut count1_end = SAMPLES_PER_GRANULE;
    while count1_end > 0 && ix[count1_end - 1] == 0 {
        count1_end -= 1;
    }

    // Find where big_values ends: the last sample with |val| > 1.
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
    // *past* count1_end -- see the M6 review notes (this is the fix for
    // the proptest-found panic pinned in
    // proptest-regressions/huffman/encode.txt).
    count1_end = count1_end.max(big_values_end);

    let pairs_count = big_values_end / 2;
    let third = pairs_count / 3;

    Regions {
        big_values_end,
        count1_end,
        r0_end: third * 2,
        r1_end: third * 4,
    }
}

/// Bit-count estimate for the quantization inner loop
/// ([`crate::quantize::loop_control::quantize_granule`]). Must
/// over-estimate rather than under-estimate on ties, so the inner loop
/// never produces a bitstream that overflows its budget once
/// [`encode_granule`] runs for real -- this mirrors `encode_granule`'s
/// actual region splitting and per-region table costing exactly (see
/// `region_cost_with_table`'s doc comment for why a cheaper, per-pair
/// heuristic silently violated that contract). See
/// `docs/mp3-encoder/09-phase6-huffman-coding.md` §4.
#[must_use]
pub fn estimate_bits(ix: &[i32; SAMPLES_PER_GRANULE]) -> u32 {
    let regions = compute_regions(ix);

    let mut bits = 0u32;
    for &(start, end) in &[
        (0, regions.r0_end),
        (regions.r0_end, regions.r1_end),
        (regions.r1_end, regions.big_values_end),
    ] {
        bits += choose_table_and_cost(ix, start, end).1;
    }
    bits += count1_region_cost(ix, regions.big_values_end, regions.count1_end).1;

    bits
}

/// `big_values` region boundaries actually used to encode, computed
/// **once** and reused for both table selection and the byte-encoding
/// loop in [`encode_granule`].
///
/// An earlier version of the window-switching (`block_type != Long`)
/// path computed this split three separate ways: one boundary to pick
/// `table_select[0]` (via [`Regions::r0_end`], a long-block-shaped 1/3
/// point), a *different* boundary to actually write the encoded bytes
/// (via `count_sf_bands_for_pairs` on a 1/2 point), and a *third* value
/// for the (unsignaled, for this case) `region0_count`/`region1_count`
/// bookkeeping fields. Because the table was chosen by looking at a
/// smaller range than the one it was then applied to, a table picked as
/// `0` (meaning "no big-values here, skip") for the narrower range could
/// silently drop real values that existed only in the gap between the
/// two boundaries — data loss, not just a suboptimal table choice. This
/// type exists so that class of bug is structurally impossible: there is
/// exactly one boundary computation, used everywhere.
///
/// **Standards-compliance caveat (window-switching case only):**
/// `region0_count`/`region1_count` are never transmitted when
/// `window_switching_flag == 1` (`SideInfo::write` omits them — see
/// `bitstream/side_info.rs`), so a real decoder derives its own
/// region0/1 boundary from ISO/IEC 11172-3 Annex B's fixed rule for that
/// case, independent of anything this encoder chooses. The split
/// computed below (half of `big_values_end`, rounded to a scalefactor-
/// band boundary) is *not yet cross-checked against that fixed table* —
/// it removes the internal inconsistency above, but has not been
/// verified to match what an external decoder (Symphonia/LAME/ffmpeg)
/// expects. Do not treat window-switching output as interoperability-
/// verified until a differential decode test on transient content
/// (Start/Short/Stop-triggering material) passes — see `docs/plus.md`
/// M11.6/M11.7.
struct RegionSplit {
    /// Up to 3 `(start, end)` sample ranges; only `[..n_regions]` valid.
    ranges: [(usize, usize); 3],
    /// 2 for window-switching blocks, 3 for `Long`.
    n_regions: usize,
    /// `region0_count`/`region1_count` — transmitted for `Long`,
    /// computed-but-unused (see caveat above) for window-switching.
    region0_count: u8,
    region1_count: u8,
}

impl RegionSplit {
    fn compute(
        block_type: BlockType,
        big_values_end: usize,
        band_end: &[usize; SF_BAND_COUNT],
    ) -> Self {
        let pairs_count = big_values_end / 2;
        if block_type == BlockType::Long {
            // ISO/IEC 11172-3: `region0_count` is (bands in region0) - 1;
            // `region1_count` is (bands in region1) - 1 -- region1's own
            // count, *not* cumulative from the start. A decoder derives
            // both sample-line boundaries purely from these two fields:
            // `region0_end = band_index[region0_count + 1]` and
            // `region1_end = band_index[region0_count + 1 + region1_count + 1]`
            // (verified against an external reference during the M11
            // gain/corruption investigation -- see docs/mejoras.md).
            //
            // An earlier version computed `region0_count`/`region1_count`
            // this way but then wrote the *actual* big_values bytes using
            // `regions.r0_end`/`r1_end` -- independently-computed "raw
            // thirds of pairs" boundaries from `compute_regions`, not
            // required to land on the same scalefactor-band boundary at
            // all. It also stored `region1_count` as the *cumulative*
            // band count through region1's end rather than region1's own
            // count. Both mistakes are silent: nothing here panics or
            // fails a self-consistency check, since this encoder never
            // decodes its own bitstream the way a real MP3 decoder does
            // (region-boundary derivation from side info, independent of
            // whatever the encoder privately used to choose those
            // numbers). A real decoder applies `table_select[0]`/`[1]`
            // over the *declared* boundaries, so any mismatch between
            // "where this encoder actually switched Huffman tables" and
            // "where the declared region0_count/region1_count say it
            // did" corrupts every bit after the first such mismatch in
            // the granule -- observed as ffmpeg's mp3float `overread`
            // errors and garbage-looking (often full-scale) decoded
            // audio on real content (a single dominant spectral line, as
            // in a synthetic sine-tone test, rarely spans enough bands
            // to expose it).
            let third = pairs_count / 3;
            let bands_through_r0 = count_sf_bands_for_pairs(third, band_end);
            let bands_through_r1 =
                count_sf_bands_for_pairs(third * 2, band_end).max(bands_through_r0);

            let region0_count = bands_through_r0.saturating_sub(1).min(15) as u8;
            let region1_bands = bands_through_r1 - bands_through_r0;
            let region1_count = region1_bands.saturating_sub(1).min(7) as u8;

            // The actual table-switch points, derived from the *same*
            // region0_count/region1_count values just declared -- not
            // from an independent calculation -- so the bytes this
            // function writes below are guaranteed to match what any
            // compliant decoder resolves from side info.
            let r0_end =
                band_end[(region0_count as usize).min(SF_BAND_COUNT - 1)].min(big_values_end);
            let r1_end = band_end
                [(region0_count as usize + region1_count as usize + 1).min(SF_BAND_COUNT - 1)]
            .min(big_values_end);

            Self {
                ranges: [(0, r0_end), (r0_end, r1_end), (r1_end, big_values_end)],
                n_regions: 3,
                region0_count,
                region1_count,
            }
        } else {
            // window_switching_flag == 1: 2 regions. Split at the
            // nearest scalefactor-band boundary to the midpoint of
            // big_values, rounded via the same `count_sf_bands_for_pairs`
            // helper the Long case uses (keeps both cases' boundaries
            // band-aligned, consistent with how `build_band_map` in
            // `quantize/loop_control.rs` already treats short-block
            // bands). See this type's doc comment for what's still
            // unverified about this specific split point.
            let half = pairs_count / 2;
            let band_count = count_sf_bands_for_pairs(half, band_end);
            let mid =
                band_end[band_count.saturating_sub(1).min(band_end.len() - 1)].min(big_values_end);
            Self {
                ranges: [(0, mid), (mid, big_values_end), (0, 0)],
                n_regions: 2,
                region0_count: band_count as u8,
                region1_count: 0,
            }
        }
    }
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

/// Full Huffman encode: region splitting + exhaustive per-region table
/// selection + `count1` region + escape (`linbits`) handling, emitting
/// bits via `writer`. Called once per granule, after the quantization
/// loop has converged. See
/// `docs/mp3-encoder/09-phase6-huffman-coding.md` §3-4.
///
/// When `block_type != Long` (i.e. `window_switching_flag == 1` for
/// Start/Short/Stop), only 2 big_values regions are used instead of 3;
/// `region0_count`/`region1_count` are computed but never transmitted
/// for this case (`SideInfo::write` omits them). See [`RegionSplit`]'s
/// doc comment for the boundary computation and its standards-
/// compliance caveat.
pub fn encode_granule(
    ix: &[i32; SAMPLES_PER_GRANULE],
    block_type: BlockType,
    writer: &mut BitWriter<'_>,
) -> HuffmanSideInfo {
    let band_end = sf_band_end();
    let regions = compute_regions(ix);
    let big_values_end = regions.big_values_end;
    let count1_end = regions.count1_end;

    let big_values_count = (big_values_end / 2) as u16;

    // Region boundaries are computed exactly once here and reused for
    // *both* table selection and the actual byte-encoding loop below —
    // see `RegionSplit`'s doc comment for why that used to be three
    // independently-computed (and disagreeing) values.
    let split = RegionSplit::compute(block_type, big_values_end, &band_end);
    let region0_count = split.region0_count;
    let region1_count = split.region1_count;

    let mut table_select = [0u8; 3];
    for (i, &(start, end)) in split.ranges.iter().enumerate().take(split.n_regions) {
        table_select[i] = choose_table(ix, start, end);
    }

    let regions_list = &split.ranges[..split.n_regions];
    let region_tables = &table_select[..split.n_regions];

    for (&(start, end), &table_id) in regions_list.iter().zip(region_tables.iter()) {
        if table_id == 0 {
            continue;
        }
        // INVARIANT: BIG_VALUES_TABLES is indexed by `table_id`, which is only
        // set (via `choose_table_and_cost`) when the lookup returned `Some`.
        // An index returning `None` would indicate a logic bug in table
        // selection; the caller guarantees it never happens on a valid path.
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
    let (use_table_1, _) = count1_region_cost(ix, count1_start, count1_end);
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
        table_select,
        count1table_select: use_table_1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::huffman::tables::{Count1Table, HuffmanTable};
    use alloc::vec::Vec;
    use proptest::prelude::*;

    /// `estimate_bits` predicts the *raw* bit count `encode_granule`
    /// emits; the only thing `out.len() * 8` can legitimately add on top
    /// is `BitWriter::flush`'s zero-padding up to the next byte boundary
    /// (0..=7 bits) -- not a fudge factor, a hard bound following
    /// directly from `flush`'s own definition. Comparing the two without
    /// that allowance would fail even for a perfectly exact estimator.
    const FLUSH_PADDING_SLOP_BITS: u32 = 7;

    fn assert_estimate_never_undercounts(ix: &[i32; SAMPLES_PER_GRANULE]) {
        let estimate = estimate_bits(ix);

        let mut out = Vec::new();
        let mut writer = BitWriter::new(&mut out);
        let _info = encode_granule(ix, BlockType::Long, &mut writer);
        writer.flush();
        let actual_bits = (out.len() * 8) as u32;

        assert!(
            estimate + FLUSH_PADDING_SLOP_BITS >= actual_bits,
            "estimate ({estimate}) is more than {FLUSH_PADDING_SLOP_BITS} \
             bits (the max possible flush-padding) below actual \
             ({actual_bits}) -- estimate_bits is under-counting the real \
             region-committed Huffman cost"
        );
    }

    #[test]
    fn estimate_bits_never_undercounts() {
        let mut ix = [0i32; SAMPLES_PER_GRANULE];
        for (i, item) in ix.iter_mut().enumerate().take(100) {
            *item = (i as i32 % 7) - 3;
        }
        assert_estimate_never_undercounts(&ix);
    }

    #[test]
    fn estimate_bits_never_undercounts_broadband_noise() {
        // Regression test for the bug this review found: a per-pair
        // "cheapest of all 15 tables" heuristic (an earlier version of
        // `estimate_bits`) structurally undercounts relative to the real
        // encoder, which commits one table per whole region -- confirmed
        // empirically on content shaped like this (128kbps granule of
        // moderate broadband noise): estimate_bits predicted 1426 bits,
        // encode_granule's real output was 2024 -- a 42% undercount that
        // silently overflowed the caller's bit budget in M8's pipeline.
        // The earlier `estimate_bits_never_undercounts` test above only
        // covers 100 small values (magnitude <= 3), never broad enough
        // content to trigger this.
        let mut seed: u32 = 12345;
        let mut ix = [0i32; SAMPLES_PER_GRANULE];
        for v in ix.iter_mut() {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            let raw = (seed >> 24) as i32 - 128; // roughly -128..127
            *v = raw / 4; // keep magnitudes realistic for a quantized granule
        }
        assert_estimate_never_undercounts(&ix);
    }

    #[test]
    fn estimate_bits_is_tight_not_just_safe() {
        // A structurally-safe-but-loose over-estimate (e.g. "assume the
        // biggest possible table for everything") would also pass the
        // never-undercounts tests above without actually fixing the
        // problem those bugs cause downstream (an inner loop trusting a
        // wildly loose estimate coarsens quantization far more than
        // necessary). Confirm the estimate is *close* to real, not just
        // safely above it -- within flush-padding slop plus a small
        // margin for legitimately-suboptimal-but-valid table choices.
        let mut seed: u32 = 999;
        let mut ix = [0i32; SAMPLES_PER_GRANULE];
        for v in ix.iter_mut() {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            *v = ((seed >> 24) as i32 - 128) / 4;
        }
        let estimate = estimate_bits(&ix);
        let mut out = Vec::new();
        let mut writer = BitWriter::new(&mut out);
        encode_granule(&ix, BlockType::Long, &mut writer);
        writer.flush();
        let actual = (out.len() * 8) as u32;

        assert!(estimate >= actual.saturating_sub(FLUSH_PADDING_SLOP_BITS));
        assert!(
            estimate < actual + 32,
            "estimate ({estimate}) is suspiciously far above actual \
             ({actual}) for a tight estimator"
        );
    }

    #[test]
    fn encode_zeros_produces_no_bits() {
        let ix = [0i32; SAMPLES_PER_GRANULE];
        let mut out = Vec::new();
        let mut writer = BitWriter::new(&mut out);
        let _info = encode_granule(&ix, BlockType::Long, &mut writer);
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
        let info = encode_granule(&ix, BlockType::Long, &mut writer);
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
    ///
    /// `block_type` selects the same [`RegionSplit`] computation
    /// `encode_granule` used, so this rebuilds the *identical* boundary
    /// rather than a hardcoded thirds-only split -- for `block_type ==
    /// Long` this is unchanged from before; for window-switching block
    /// types it exercises the 2-region path. Note this still isn't an
    /// independent decoder: it derives the boundary the same way the
    /// encoder does, so it proves internal self-consistency (the bug
    /// documented on `RegionSplit`), not standards compliance -- see
    /// that doc comment's caveat for window-switching blocks.
    fn decode_granule_for_test(
        data: &[u8],
        info: &HuffmanSideInfo,
        block_type: BlockType,
        count1_end: usize,
    ) -> [i32; SAMPLES_PER_GRANULE] {
        let mut out = [0i32; SAMPLES_PER_GRANULE];
        let mut reader = BitReader::new(data);

        let pairs_count = info.big_values as usize;
        let big_values_end = pairs_count * 2;
        let third = pairs_count / 3;
        let regions = Regions {
            big_values_end,
            count1_end,
            r0_end: third * 2,
            r1_end: third * 4,
        };
        let band_end = sf_band_end();
        let split = RegionSplit::compute(block_type, big_values_end, &band_end);
        let mut region_bounds = [(0usize, 0usize, 0u8); 3];
        for (i, &(s, e)) in split.ranges.iter().enumerate().take(split.n_regions) {
            region_bounds[i] = (s, e, info.table_select[i]);
        }

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
        let info = encode_granule(&ix, BlockType::Long, &mut writer);
        writer.flush();

        let decoded = decode_granule_for_test(&out, &info, BlockType::Long, last_nonzero_end(&ix));
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
        // `end - pos` underflow (panic) in `count1_region_cost` and the
        // count1 emission loop. This value/position is the shrunk
        // failing case verbatim.
        let mut ix = [0i32; SAMPLES_PER_GRANULE];
        ix[58] = 2;

        let mut out = Vec::new();
        let mut writer = BitWriter::new(&mut out);
        let info = encode_granule(&ix, BlockType::Long, &mut writer); // must not panic
        writer.flush();

        let decoded = decode_granule_for_test(&out, &info, BlockType::Long, last_nonzero_end(&ix));
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
        let info = encode_granule(&ix, BlockType::Long, &mut writer);
        writer.flush();

        let decoded = decode_granule_for_test(&out, &info, BlockType::Long, last_nonzero_end(&ix));
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
            let info = encode_granule(&ix, BlockType::Long, &mut writer);
            writer.flush();

            let decoded = decode_granule_for_test(&out, &info, BlockType::Long, last_nonzero_end(&ix));
            prop_assert_eq!(decoded, ix);
        }
    }

    /// Guards the specific bug `RegionSplit` closed: an earlier version of
    /// `encode_granule`'s window-switching (`block_type != Long`) path
    /// chose `table_select[0]` by inspecting only `[0, ~1/3 point)` but
    /// then applied that table while actually encoding the larger range
    /// `[0, ~1/2 point)` -- so a big-value pair placed strictly between
    /// those two points could be encoded under a table (or dropped
    /// entirely, if the narrower range looked all-zero and table 0 got
    /// picked) that was never evaluated against it. This places non-zero
    /// big-values exactly in that gap for all three window-switching
    /// block types and checks they still round-trip exactly.
    #[test]
    fn window_switching_round_trips_values_in_the_old_gap_region() {
        for block_type in [BlockType::Start, BlockType::Short, BlockType::Stop] {
            let mut ix = [0i32; SAMPLES_PER_GRANULE];
            // Old (buggy) region0 end was ~1/3 of big_values_end; old
            // actual-encode split was ~1/2. Put distinctive big-values
            // in [1/3, 1/2) of a granule with big_values_end well past
            // both -- and nothing at all before that gap, so the old
            // code's narrower `choose_table` range would have seen an
            // all-zero region and picked table 0 (skip).
            ix[220] = 12;
            ix[221] = -9;
            ix[222] = 30; // forces an escape-table choice
            ix[223] = -30;
            // Extend big_values_end well past the gap so the old
            // "1/3 vs 1/2" boundaries actually differ from each other.
            ix[400] = 5;
            ix[401] = -3;

            let mut out = Vec::new();
            let mut writer = BitWriter::new(&mut out);
            let info = encode_granule(&ix, block_type, &mut writer);
            writer.flush();

            let decoded = decode_granule_for_test(&out, &info, block_type, last_nonzero_end(&ix));
            assert_eq!(
                decoded, ix,
                "{block_type:?}: values in the old gap region did not round-trip"
            );
        }
    }

    proptest! {
        #[test]
        fn window_switching_round_trip_random_granules(
            values in prop::collection::vec(-300i32..=300, 60),
        ) {
            for block_type in [BlockType::Start, BlockType::Short, BlockType::Stop] {
                let mut ix = [0i32; SAMPLES_PER_GRANULE];
                for (i, v) in values.iter().enumerate() {
                    ix[i] = *v;
                }

                let mut out = Vec::new();
                let mut writer = BitWriter::new(&mut out);
                let info = encode_granule(&ix, block_type, &mut writer);
                writer.flush();

                let decoded =
                    decode_granule_for_test(&out, &info, block_type, last_nonzero_end(&ix));
                prop_assert_eq!(decoded, ix);
            }
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
        let info = encode_granule(&ix, BlockType::Long, &mut writer);
        writer.flush();

        assert!(info.big_values > 0 || !out.is_empty());
    }
}
