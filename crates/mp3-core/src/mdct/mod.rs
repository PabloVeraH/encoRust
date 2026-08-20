//! Per-subband MDCT, window shapes, and the anti-aliasing butterfly — the
//! second stage of Layer III's hybrid filterbank. See
//! `docs/mp3-encoder/06-phase3-mdct-and-windowing.md`.

use crate::types::SUBBANDS;

/// Which window shape (and therefore MDCT size) a granule — or, for
/// mixed blocks, a subband within a granule — uses. See
/// `docs/mp3-encoder/06-phase3-mdct-and-windowing.md` §1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockType {
    /// Stationary signal, the normal case. 36 samples in, 18 lines out.
    Long,
    /// Transition into a run of short blocks (attack incoming).
    Start,
    /// Transient signal — finer time resolution. 12 samples in, 6 lines
    /// out, ×3 short windows per granule.
    Short,
    /// Transition back to long blocks from a run of short blocks.
    Stop,
}

/// Applies the appropriate window (per [`BlockType`]) then the MDCT, for
/// one subband's long/start/stop-type block.
///
/// `prev_tail` is this subband's last-18-samples carryover from the
/// previous granule — the MDCT's 50% overlap is what buys Layer III its
/// extra frequency resolution beyond the 32-band filterbank alone. See
/// `docs/mp3-encoder/06-phase3-mdct-and-windowing.md` §2.
///
/// # Panics
///
/// Always, in this scaffold. Implement per §2-3 of that chapter — and
/// verify the exact MDCT formula (sign/phase/normalization convention)
/// against Annex B or a second source via the perfect-reconstruction test
/// specified in that chapter's Definition of Done **before** trusting
/// this function; MDCT convention mismatches are subtle and this is
/// exactly the failure mode that test exists to catch.
pub fn transform_long(
    input: &[f32; 18],
    prev_tail: &[f32; 18],
    block_type: BlockType,
) -> [f32; 18] {
    let _ = (input, prev_tail, block_type);
    todo!("M3: window then MDCT sum — see 06-phase3 §2-3")
}

/// Applies the short window then the MDCT, independently, to each of the
/// granule's 3 short windows. See
/// `docs/mp3-encoder/06-phase3-mdct-and-windowing.md` §2-3.
///
/// # Panics
///
/// Always, in this scaffold.
pub fn transform_short(windows: &[[f32; 12]; 3]) -> [[f32; 6]; 3] {
    let _ = windows;
    todo!("M3: window then MDCT per short window — see 06-phase3 §2-3")
}

/// Applies the 8-point anti-aliasing butterfly across all 31
/// adjacent-subband boundaries of a fully-transformed granule, correcting
/// for frequency-domain aliasing inherent in the 32-band polyphase split.
/// Mutates in place. See
/// `docs/mp3-encoder/06-phase3-mdct-and-windowing.md` §4.
///
/// # Panics
///
/// Always, in this scaffold. Before trusting the `cs`/`ca` constants used
/// internally, verify `cs[i]^2 + ca[i]^2 ≈ 1` for all 8 pairs (the
/// butterfly is a rotation, so this must hold regardless of source) — see
/// `docs/mp3-encoder/13-testing-and-validation.md` §Table provenance.
pub fn antialias_butterfly(
    spectrum: &mut [[f32; 18]; SUBBANDS],
    block_types: &[BlockType; SUBBANDS],
) {
    let _ = (spectrum, block_types);
    todo!("M3: 8-point butterfly per adjacent subband pair — see 06-phase3 §4")
}

#[cfg(test)]
mod tests {
    // TODO(M3): perfect-reconstruction test — encode PCM through the
    // filterbank + MDCT, invert with a test-only inverse MDCT +
    // overlap-add + inverse filterbank, assert the result matches the
    // (delayed) input within a documented tolerance. This is the
    // load-bearing correctness gate for this module — see
    // docs/mp3-encoder/06-phase3-mdct-and-windowing.md §6.
}
