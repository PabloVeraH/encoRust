//! The inner (rate) loop and outer (distortion) loop. See
//! `docs/mp3-encoder/08-phase5-quantization-loop.md` §3 — the
//! algorithmic heart of the encoder, where quality/bitrate trade-offs
//! actually happen.

use crate::mdct::BlockType;
use crate::psychoacoustic::ScalefactorBandSmr;
use crate::psychoacoustic::{
    scalefactor_sample_rate_index, SFB_LONG_BOUNDARIES, SFB_LONG_COUNTS, SFB_SHORT_BOUNDARIES,
    SFB_SHORT_COUNTS,
};
use crate::quantize::scalefactors::ScaleFactors;

// f32::abs() is std-only under this crate's MSRV (1.82) when built
// `--no-default-features` for wasm32 — pre-existing latent breakage,
// invisible until `rust-toolchain.toml` pinned a real 1.82 build (see
// docs/investigation-log.md's review notes). Use libm::fabsf, same as every other
// transcendental function in this crate.
use libm::{fabsf, powf};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum outer-loop (distortion) iterations before accepting best-effort.
const MAX_OUTER_ITERATIONS: usize = 8;

/// `global_gain` transmitted in the bitstream carries a +210 bias relative
/// to the internal `step` (see §2 of the phase-5 chapter).
const GLOBAL_GAIN_BIAS: u8 = 210;

/// Quantizer additive constant (ISO/IEC 11172-3 Annex B).
const QUANT_OFFSET: f32 = 0.0946;

/// Quantizer power: `(...)^0.75` on encode, `(...)^(4/3)` on decode.
const QUANT_POW_ENCODE: f32 = 0.75;
const QUANT_POW_DECODE: f32 = 4.0 / 3.0;

/// Exponent scale: `2^(step/4)` → multiplier = `2^(-step*0.25)`.
const STEP_SCALE: f32 = 0.25;

/// Maximum `global_gain` (8-bit field; 0..255).
const MAX_GLOBAL_GAIN: u8 = 255;

/// Maximum scalefactor value (for scalefac_scale = false, values 0..15).
const MAX_SCALEFAC: u8 = 15;

/// Small floor to prevent log-of-zero / div-by-zero in edge cases.
const EPS: f32 = 1e-30;

/// Largest `ix` magnitude any Huffman big_values table can represent.
/// The widest table (`xlen=16`, i.e. `esc_threshold=15`) paired with the
/// widest escape (`linbits=13`) covers `15 + (2^13 - 1) = 8206` — see
/// `huffman::tables::BIG_VALUES_TABLES`. `quantize_spectrum` saturates to
/// this bound for the same reason `scalefactor_encode::saturate_to_width`
/// clamps scalefactors: producing a value no real Huffman table (with or
/// without its escape mechanism) can encode doesn't fail loudly here --
/// `encode_granule`'s `write_bits` calls would silently truncate the
/// escape field's high bits instead, corrupting that value and
/// desyncing every bit the decoder reads afterward in the granule. This
/// was unreachable while the rate loop only ever searched `step` upward
/// from a fixed 0 (small, bounded `ix`); it became reachable once the
/// search was corrected to cover the *entire* representable `step`
/// range (see `inner_loop`'s doc comment) — confirmed via symphonia
/// reporting `"huffman decode overrun"` on real content before this
/// clamp was added.
const MAX_REPRESENTABLE_IX: i32 = 8206;

// ---------------------------------------------------------------------------
// Quantization result
// ---------------------------------------------------------------------------

/// Everything the quantization loop produces for one granule/channel:
/// the quantized values themselves plus every side-info field this stage
/// owns.
#[derive(Debug, Clone, Copy)]
pub struct QuantizationResult {
    /// Quantized magnitudes (sign is tracked separately).
    pub ix: [i32; 576],
    /// Sign of each spectral line (`true` = negative).
    pub sign: [bool; 576],
    /// Per-band scale factors.
    pub scalefac: ScaleFactors,
    /// `global_gain` side-info field (transmitted value).
    pub global_gain: u8,
    /// `scalefac_scale` side-info field.
    pub scalefac_scale: bool,
    /// `preflag` side-info field.
    pub preflag: bool,
    /// `subblock_gain` per short window — `None` for long/start/stop blocks.
    pub subblock_gain: Option<[u8; 3]>,
    /// Whether the outer (distortion) loop satisfied every band's masking
    /// threshold before `MAX_OUTER_ITERATIONS` ran out. `false` means the
    /// returned result is a best-effort fallback -- chapter 08 §3
    /// explicitly expects this to happen sometimes ("termination is not
    /// guaranteed by the math alone") and asks for it to be visible
    /// rather than silent; this field is that visibility, since a
    /// `no_std`-compatible lib crate has no logging backend to write to.
    pub converged: bool,
}

// ---------------------------------------------------------------------------
// Non-uniform quantizer (§2)
// ---------------------------------------------------------------------------

/// Apply the power-law (companded) quantizer to all 576 spectral lines.
///
/// `ix[i] = nint( (|xr[i]| * 2^(-step_eff/4))^0.75 - 0.0946 )`
/// where `step_eff = step - scalefac_gain[b]`.
fn quantize_spectrum(
    spectrum: &[f32; 576],
    step: f32,
    band_of_line: &[usize; 576],
    scalefac: &[u8; 39],
    scalefac_scale: bool,
    ix_out: &mut [i32; 576],
    sign_out: &mut [bool; 576],
) {
    for i in 0..576 {
        let b = band_of_line[i];
        let sf_gain = scalefac_value(scalefac[b], scalefac_scale) as f32;
        let step_eff = step - sf_gain;
        let scaled = fabsf(spectrum[i]) * powf(2.0, -STEP_SCALE * step_eff);
        let ix_float = powf(scaled, QUANT_POW_ENCODE) - QUANT_OFFSET;
        ix_out[i] = if ix_float > 0.0 {
            ((ix_float + 0.5) as i32).min(MAX_REPRESENTABLE_IX)
        } else {
            0
        };
        sign_out[i] = spectrum[i] < 0.0;
    }
}

/// Map a raw scalefactor integer to its additive contribution to the
/// exponent, in the *same* quarter-step units `step`/`global_gain` use
/// (i.e. the combined exponent is `(step - scalefac_value(...)) / 4`).
///
/// A raw scalefactor step is **not** a quarter-step: it's `2*raw` when
/// `scalefac_scale = false`, `4*raw` when `scalefac_scale = true` (the
/// scale flag doubles scalefactor step size, same as the standard's own
/// `scalefac_multiplier` of 0.5 / 1.0 applied on top of a `1/4`-scaled
/// exponent). Cross-checked against minimp3's dequantizer (`scf_shift =
/// scalefac_scale + 1`, i.e. `iscf << scf_shift`, so `raw << 1` / `raw <<
/// 2`) and the equivalent `scale_factor = 2*scalefac*(1+scalefac_scale)`
/// formulation. An earlier version of this function returned `raw` / `2*
/// raw` — half the correct value in both branches, which would make a
/// real decoder apply half the intended scalefactor gain per band; see
/// the M5 review notes for the derivation.
fn scalefac_value(raw: u8, scale: bool) -> u8 {
    let shift = if scale { 2 } else { 1 };
    raw << shift
}

// ---------------------------------------------------------------------------
// Dequantizer — used internally for distortion estimation in the outer loop,
// and exposed to tests for the round-trip validation.
// ---------------------------------------------------------------------------

/// Inverse of [`quantize_spectrum`] -- the *decoder-side* formula chapter
/// 08 §2 quotes as authoritative: `xr_hat = ix^(4/3) * 2^(step_eff/4)`.
///
/// Deliberately does **not** add `QUANT_OFFSET` here: that constant is an
/// encoder-only rounding-bias trick (it shifts `nint`'s decision boundary
/// to better center each quantization bin before rounding), not part of
/// the mathematical inverse -- chapter 08 §2 states this formula with no
/// additive term, precisely to head off "two incompatible formulations
/// circulate" confusion. Carrying `QUANT_OFFSET` into this decode-side
/// formula (as an earlier version of this function did) makes `ix == 0`
/// dequantize to a nonzero floor (`0.0946^(4/3) ≈ 0.043`) instead of
/// exact silence, and biases the outer loop's distortion estimate upward
/// for every low-energy band -- see the M5 review notes.
fn dequantize_spectrum(
    ix: &[i32; 576],
    step: f32,
    band_of_line: &[usize; 576],
    scalefac: &[u8; 39],
    scalefac_scale: bool,
    sign_in: &[bool; 576],
) -> [f32; 576] {
    let mut out = [0.0f32; 576];
    for i in 0..576 {
        let b = band_of_line[i];
        let sf_gain = scalefac_value(scalefac[b], scalefac_scale) as f32;
        let step_eff = step - sf_gain;
        let mag = powf(ix[i] as f32, QUANT_POW_DECODE) * powf(2.0, STEP_SCALE * step_eff);
        out[i] = if sign_in[i] { -mag } else { mag };
    }
    out
}

// ---------------------------------------------------------------------------
// Bit-count estimator (§3 inner loop)
// ---------------------------------------------------------------------------
//
// The rate loop below costs candidate quantizations via
// `crate::huffman::estimate_bits` -- the *real*, per-region Huffman table
// cost (see that function's doc comment), not an independent, cheaper
// heuristic. An earlier version of this module had its own such
// heuristic here (`1 (sign) + floor(log2(v)) + 1` bits per nonzero
// value, monotonic in `v` and `step` but otherwise unrelated to what any
// real Huffman table actually costs). That was tolerable while the rate
// loop only ever searched a narrow, small-`ix` region of `step`, but
// became a real bug once the search was corrected to cover the entire
// representable `step` range (see `inner_loop`'s doc comment): the crude
// estimate could under-count badly enough that a chosen step's *real*
// Huffman-coded size overflowed the granule's bit budget, silently
// truncating `main_data` for the rest of the frame — confirmed via an
// independent decoder (symphonia) reporting "huffman decode overrun" on
// real content before this switch.

// ---------------------------------------------------------------------------
// Per-band helpers
// ---------------------------------------------------------------------------

/// Build a `band_of_line[576]` mapping each MDCT line to its scalefactor
/// band using the Annex B tables. Long-block layout uses up to 22 bands;
/// short blocks use 13. Returns a zero-filled map for lines not covered
/// by any band.
///
/// # Short-block layout assumption
///
/// `SFB_SHORT_BOUNDARIES` gives boundaries *within one 192-line window*.
/// The flat 576-line-per-granule layout this function assumes is the
/// standard "band-major, window-minor" one real encoders use: band `b`
/// occupies `3 * width(b)` *consecutive* lines in the flat array --
/// window 0's slice, then window 1's, then window 2's -- i.e. flat
/// boundary `3 * bounds[b]`, not `bounds[b]` directly (an earlier version
/// of this function used `bounds[b]` unscaled, which only covered lines
/// 0..192 and left the 2nd/3rd windows' 384 lines silently mapped to
/// band 0). This is the conventional layout (matches LAME/dist10's
/// `3*scalefac_band.s[sfb]`-style indexing), but no assembly function
/// producing the actual flat short-block spectrum exists yet elsewhere in
/// this crate (`mdct::transform_short` only produces one subband's
/// `[[f32; 6]; 3]` at a time) -- **cross-check this against chapter 06's
/// short-block granule assembly once that's implemented**, don't treat it
/// as independently verified.
///
/// Separately: `smr: &ScalefactorBandSmr` passed into
/// [`quantize_granule`] is *always* computed against the long-block 22-
/// band grid (M4's psychoacoustic model doesn't wire up short-block SMR
/// yet -- see `docs/mp3-encoder/07-phase4-psychoacoustic-model.md`'s M4
/// revision log). For `BlockType::Short`, this function's 13-band map is
/// therefore mechanically correct but the outer loop compares it against
/// an SMR profile keyed to a *different* band grid -- not fixable within
/// M5's scope; flagged here so it isn't mistaken for full short-block
/// support.
fn build_band_map(sfb_idx: usize, block_type: BlockType) -> [usize; 576] {
    let mut map = [0usize; 576];

    match block_type {
        BlockType::Long | BlockType::Start | BlockType::Stop => {
            let bounds = &SFB_LONG_BOUNDARIES[sfb_idx];
            let count = SFB_LONG_COUNTS[sfb_idx];
            for b in 0..count {
                let start = bounds[b];
                let end = bounds[b + 1].min(576);
                if end > start {
                    map[start..end].fill(b);
                }
            }
        }
        BlockType::Short => {
            let bounds = &SFB_SHORT_BOUNDARIES[sfb_idx];
            let count = SFB_SHORT_COUNTS[sfb_idx];
            for b in 0..count {
                let start = bounds[b] * 3;
                let end = (bounds[b + 1] * 3).min(576);
                if end > start {
                    map[start..end].fill(b);
                }
            }
        }
    }

    map
}

/// Scalefactor band count for the given block type -- long blocks (and
/// the long-shaped Start/Stop transition blocks) use
/// [`SFB_LONG_COUNTS`], short blocks use [`SFB_SHORT_COUNTS`]. An earlier
/// version of [`quantize_granule`] used [`SFB_LONG_COUNTS`]
/// unconditionally, which combined with [`build_band_map`]'s old bug to
/// leave short blocks doubly wrong (mapping *and* band count both
/// long-shaped).
fn band_count_for(sfb_idx: usize, block_type: BlockType) -> usize {
    match block_type {
        BlockType::Long | BlockType::Start | BlockType::Stop => SFB_LONG_COUNTS[sfb_idx],
        BlockType::Short => SFB_SHORT_COUNTS[sfb_idx],
    }
    .min(22)
}

/// Compute per-band distortion energy (sum of squared errors) and signal
/// energy. Returns `(signal_energy, distortion_energy)` arrays.
fn compute_band_distortion(
    original: &[f32; 576],
    reconstructed: &[f32; 576],
    band_of_line: &[usize; 576],
    num_bands: usize,
) -> ([f32; 22], [f32; 22]) {
    let mut signal = [0.0f32; 22];
    let mut distortion = [0.0f32; 22];

    for i in 0..576 {
        let b = band_of_line[i];
        if b < num_bands {
            let diff = original[i] - reconstructed[i];
            signal[b] += original[i] * original[i];
            distortion[b] += diff * diff;
        }
    }

    (signal, distortion)
}

// ---------------------------------------------------------------------------
// Inner loop ("rate loop")
// ---------------------------------------------------------------------------

struct InnerLoopResult {
    ix: [i32; 576],
    sign: [bool; 576],
    step: f32,
    global_gain: u8,
    converged: bool,
}

/// Smallest representable `step` (`global_gain` field = 0).
const MIN_STEP: i32 = -(GLOBAL_GAIN_BIAS as i32);

/// Largest representable `step` (`global_gain` field = `MAX_GLOBAL_GAIN`).
const MAX_STEP: i32 = MAX_GLOBAL_GAIN as i32 - GLOBAL_GAIN_BIAS as i32;

/// Rate loop: binary-search `step` over the *entire* representable range
/// (`global_gain` 0..=255, i.e. `step` in `MIN_STEP..=MAX_STEP`) for the
/// finest (smallest) step whose estimated bit count still fits
/// `bit_budget`.
///
/// `quantize_spectrum`'s bit cost is monotonically non-increasing in
/// `step` (a coarser step never needs *more* bits to represent the same
/// spectrum — see `quantizer_values_monotonic_with_step`/
/// `estimate_bits_monotonic`), which is exactly the precondition a
/// binary search over a monotonic predicate needs: converges to the
/// optimal boundary in `O(log(MAX_STEP - MIN_STEP))` ≈ 8 evaluations,
/// regardless of the spectrum's absolute magnitude.
///
/// An earlier version searched *linearly upward from `step = 0`* only
/// (never finer, i.e. never negative) — implicitly assuming `spectrum`'s
/// magnitudes already sit in whatever range makes `step = 0` a
/// reasonable starting point. That assumption silently broke for this
/// encoder's own PCM convention: `io::PcmBuffer` normalizes samples to
/// `[-1.0, 1.0]` (not the raw 16-bit-integer range many quantizer
/// constants are implicitly calibrated against), so real spectra are
/// often small enough that `step = 0` already quantizes everything to
/// zero — and since the loop could only ever coarsen from there, no
/// amount of iteration would recover a nonzero result; only per-band
/// scalefactor amplification could (`quantize_granule`'s outer loop),
/// and its own iteration cap isn't remotely enough to cover the gap.
/// Searching the *full* valid range removes the need to guess a correct
/// starting point at all. See `docs/investigation-log.md`'s gain-bug investigation
/// notes for how this was diagnosed (empirically, via ffmpeg/symphonia
/// differential decoding against real recordings, not just this crate's
/// own self-consistent quantize/dequantize round-trip).
fn inner_loop(
    spectrum: &[f32; 576],
    bit_budget: u32,
    band_of_line: &[usize; 576],
    scalefac: &[u8; 39],
    scalefac_scale: bool,
    block_type: BlockType,
) -> InnerLoopResult {
    let mut ix = [0i32; 576];
    let mut sign = [false; 576];

    let quantize_at = |step: i32, ix: &mut [i32; 576], sign: &mut [bool; 576]| {
        quantize_spectrum(
            spectrum,
            step as f32,
            band_of_line,
            scalefac,
            scalefac_scale,
            ix,
            sign,
        );
        crate::huffman::estimate_bits(ix, block_type)
    };

    // If even the coarsest representable step doesn't fit, there is
    // nothing more coarsening can do -- return that best-effort result
    // and report non-convergence (mirrors the previous implementation's
    // step-cap fallback).
    if quantize_at(MAX_STEP, &mut ix, &mut sign) > bit_budget {
        return InnerLoopResult {
            ix,
            sign,
            step: MAX_STEP as f32,
            global_gain: MAX_GLOBAL_GAIN,
            converged: false,
        };
    }

    // Binary search: `hi` always denotes a step known to fit (`MAX_STEP`
    // does, per the check above); narrow until `lo == hi`.
    let mut lo = MIN_STEP;
    let mut hi = MAX_STEP;
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if quantize_at(mid, &mut ix, &mut sign) <= bit_budget {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }

    quantize_at(hi, &mut ix, &mut sign);
    let gg = (hi + GLOBAL_GAIN_BIAS as i32).clamp(0, MAX_GLOBAL_GAIN as i32) as u8;
    InnerLoopResult {
        ix,
        sign,
        step: hi as f32,
        global_gain: gg,
        converged: true,
    }
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Diagnostic-only bypass (docs/investigation-log.md §11, "Where the next session
// should start", step 1) -- isolates whether the ~0 sample-correlation
// fidelity bug lives upstream (filterbank/MDCT/mdct_prev_tail) or
// downstream (this module's inner/outer loop interaction) of
// `quantize_granule`. When armed, `quantize_granule` skips its own
// search entirely and quantizes at a caller-chosen, fixed `step` with
// scalefac forced to all-zero -- no adaptive precision-maximization, no
// SMR-driven amplification. `#[cfg(test)]`-gated: compiled only for
// `mp3-core`'s own unit-test binary, never present in a release build or
// visible to other crates. See `Encoder`'s `diag_bypass_quantizer_pure_tone`
// test for the harness that uses this.
#[cfg(test)]
thread_local! {
    static DEBUG_FIXED_STEP: core::cell::Cell<Option<f32>> = const { core::cell::Cell::new(None) };
}

/// Arms (`Some(step)`) or disarms (`None`) the fixed-step bypass for the
/// calling thread. See the module-level comment above this function.
#[cfg(test)]
pub fn set_debug_fixed_step(step: Option<f32>) {
    DEBUG_FIXED_STEP.with(|cell| cell.set(step));
}

/// Runs the inner (rate) loop nested inside the outer (distortion) loop
/// to quantize one granule's spectrum.
///
/// - `step` convention: internal `step = global_gain - 210` (bias at side-info).
/// - Outer loop amplifies scalefactors for bands where quantization noise
///   exceeds the SMR-derived masking threshold.
/// - Always terminates within `MAX_OUTER_ITERATIONS`; best-effort result
///   returned if full convergence isn't reached.
pub fn quantize_granule(
    spectrum: &[f32; 576],
    smr: &ScalefactorBandSmr,
    bit_budget: u32,
    block_type: BlockType,
    sample_rate_hz: u32,
) -> QuantizationResult {
    let sfb_idx = scalefactor_sample_rate_index(sample_rate_hz);

    #[cfg(test)]
    if let Some(step) = DEBUG_FIXED_STEP.with(core::cell::Cell::get) {
        let band_of_line = build_band_map(sfb_idx, block_type);
        let scalefac = [0u8; 39];
        let mut ix = [0i32; 576];
        let mut sign = [false; 576];
        quantize_spectrum(
            spectrum,
            step,
            &band_of_line,
            &scalefac,
            false,
            &mut ix,
            &mut sign,
        );
        let gg = (step as i32 + GLOBAL_GAIN_BIAS as i32).clamp(0, MAX_GLOBAL_GAIN as i32) as u8;
        return QuantizationResult {
            ix,
            sign,
            scalefac: ScaleFactors { values: scalefac },
            global_gain: gg,
            scalefac_scale: false,
            preflag: false,
            subblock_gain: None,
            converged: true,
        };
    }

    let band_of_line = build_band_map(sfb_idx, block_type);
    let num_bands = band_count_for(sfb_idx, block_type);

    let mut scalefac = [0u8; 39];
    let scalefac_scale = false;

    let mut best_ir: Option<InnerLoopResult> = None;
    // True only if the outer loop exited because no band needed a retry
    // *and* the inner loop that produced this final result itself fit the
    // bit budget, rather than because either loop ran out of iterations.
    let mut converged = false;

    for _outer in 0..MAX_OUTER_ITERATIONS {
        // --- Inner (rate) loop ---
        let ir = inner_loop(
            spectrum,
            bit_budget,
            &band_of_line,
            &scalefac,
            scalefac_scale,
            block_type,
        );
        let inner_step = ir.step;

        // --- Distortion check ---
        let recon = dequantize_spectrum(
            &ir.ix,
            inner_step,
            &band_of_line,
            &scalefac,
            scalefac_scale,
            &ir.sign,
        );
        let (signal_e, distort_e) =
            compute_band_distortion(spectrum, &recon, &band_of_line, num_bands);

        // Bands already saturating `MAX_REPRESENTABLE_IX` (see
        // `quantize_spectrum`'s doc comment) can't be pushed any finer:
        // the *global* step chosen next iteration only ever gets finer
        // when there's bit budget to spare (the binary-search rate loop
        // in `inner_loop` picks the finest step the budget allows), and
        // scalefactor's per-band boost stacks multiplicatively on top of
        // that -- so a further increment here wouldn't reduce this
        // band's real distortion at all, it would just widen the gap
        // between what `ix` is capped at and what the *declared*
        // scalefactor claims, which a real decoder reconstructs from
        // the (unclamped-in-its-math) formula. Concretely: for genuinely
        // tonal content with a generous bit budget, this loop used to
        // keep incrementing such a band every iteration up to the
        // MAX_OUTER_ITERATIONS cap regardless, saturating `ix` there and
        // producing a *wrong*, not just coarser, reconstruction -- and
        // since the granule-by-granule outer loop has no continuity
        // constraint across time, two adjacent granules encoding the
        // *same* stationary tone could saturate by different amounts,
        // producing audible amplitude modulation on content a
        // reference encoder (LAME) reconstructs with ~1.0 sample
        // correlation to the original. See docs/investigation-log.md's
        // gain/corruption investigation notes for how this was
        // diagnosed (differential correlation against LAME's own
        // encode of the same material, not just this crate's internal
        // consistency).
        let mut saturated = [false; 22];
        for (&line_band, &ix) in band_of_line.iter().zip(ir.ix.iter()) {
            if ix >= MAX_REPRESENTABLE_IX && line_band < 22 {
                saturated[line_band] = true;
            }
        }

        let mut retry = false;
        for b in 0..num_bands {
            // Short blocks use their own band grid -- `smr.bands` is
            // keyed to `SFB_LONG_BOUNDARIES`, a different frequency-to-
            // band mapping than the short-block grid `band_of_line`
            // just used to compute `signal_e`/`distort_e` above. See
            // `ScalefactorBandSmr::short_bands`'s doc comment.
            let smr_value = if block_type == BlockType::Short {
                smr.short_bands[b]
            } else {
                smr.bands[b]
            };
            // `allowed` is the band's masking threshold in energy terms
            // (`signal_energy / SMR`, the standard formula) -- not
            // special-cased for `smr_value <= 1.0`. The psychoacoustic
            // model clamps every SMR value to `[1.0, 1e6]`
            // (`model2.rs`'s `part_smr[p] = (...).clamp(1.0, 1e6)`, and
            // both `sfb_smr`/`sfb_smr_short` default-init to `1.0`), so
            // `smr_value` is always >= 1.0 -- no division-by-zero risk,
            // and no case where this formula produces a *negative* or
            // undefined threshold.
            //
            // An earlier version treated `smr_value <= 1.0` (which, given
            // the clamp above, only ever means "exactly 1.0": a band the
            // model found no real masking headroom for, or one that
            // never got touched at all) as "no distortion ceiling at
            // all" (`f32::MAX`). That's the right idea for a band that
            // genuinely stands out from its neighbors, but broadband,
            // flat-spectrum content (e.g. real noise, or noise-like
            // transient material) legitimately produces SMR at-or-near
            // 1.0 across *most or all* bands simultaneously -- no band
            // stands out tonally, so the masking-ratio criterion has
            // nothing to say, and this branch quietly disabled all
            // per-band shaping for exactly that content. Confirmed
            // empirically (docs/investigation-log.md §11, Session 5): on dense
            // broadband noise, `global_gain` stayed pinned to one value
            // for the *entire file* (457 of 460 granules identical) and
            // `scalefac_compress` averaged less than half of LAME's on
            // the same content -- i.e. almost no real per-band
            // differentiation was happening, just one uniform global
            // step repeated unchanged, which two independent decoders
            // (ffmpeg and Symphonia) agreed reconstructs ~3.2% of samples
            // louder than the source's own amplitude. Using the natural
            // formula unconditionally still gives quiet/uniform bands a
            // generous, but *finite*, ceiling (their own signal energy,
            // when `smr_value == 1.0`) instead of an unconditional pass.
            let allowed = (signal_e[b] + EPS) / smr_value;

            if distort_e[b] > allowed && scalefac[b] < MAX_SCALEFAC && !saturated[b] {
                scalefac[b] += 1;
                retry = true;
            }
        }

        let inner_converged = ir.converged;
        best_ir = Some(ir);
        if !retry {
            converged = inner_converged;
            break;
        }
    }

    let ir = best_ir.expect("inner loop always produces a result");

    QuantizationResult {
        ix: ir.ix,
        sign: ir.sign,
        scalefac: ScaleFactors { values: scalefac },
        global_gain: ir.global_gain,
        scalefac_scale,
        preflag: false,
        subblock_gain: None,
        converged,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
// Scoped to `tests` only — see docs/investigation-log.md §7 item 6.
#[allow(clippy::needless_range_loop)]
mod tests {
    use super::*;
    use libm::{sinf as sin, sqrtf as sqrt};

    // --- Test helpers ---

    fn make_tone_spectrum(freq_bin: usize, amplitude: f32) -> [f32; 576] {
        let mut s = [0.0f32; 576];
        if freq_bin < 576 {
            s[freq_bin] = amplitude;
        }
        s
    }

    fn make_multitone() -> [f32; 576] {
        let mut s = [0.0f32; 576];
        for i in 0..576 {
            s[i] = (sin(i as f32 * 0.7) * 0.5 + sin(i as f32 * 2.1) * 0.3) * 10.0;
        }
        s
    }

    fn make_band_map_44100() -> [usize; 576] {
        build_band_map(0, BlockType::Long)
    }

    // --- Band mapping ---

    #[test]
    fn short_block_band_map_covers_all_576_lines() {
        // Every line must land in a real band (0..SFB_SHORT_COUNTS), not
        // silently default to band 0 because it fell outside the ×3
        // window-major boundaries -- see build_band_map's doc comment for
        // the layout this assumes. An earlier version only covered lines
        // 0..192 (the un-scaled `bounds[b]`), leaving the 2nd/3rd windows'
        // 384 lines all defaulted to band 0.
        let map = build_band_map(0, BlockType::Short);
        let count = SFB_SHORT_COUNTS[0];

        // The last band must extend to the end of the array -- if it
        // stopped at line 192 (the un-scaled bug), this fails outright.
        assert_eq!(
            map[575],
            count - 1,
            "line 575 should belong to the last short-block band, got {}",
            map[575]
        );

        // Every band from 0..count should own at least one line -- a
        // sign no band was skipped by the ×3 scaling.
        let mut seen = [false; 13];
        for &b in &map {
            assert!(b < count, "band index {b} out of range (count={count})");
            seen[b] = true;
        }
        for b in 0..count {
            assert!(seen[b], "band {b} owns no lines at all");
        }
    }

    // --- Quantizer round-trip ---

    #[test]
    fn quantizer_roundtrip_bounded_error() {
        let spectrum = make_multitone();
        let band_map = make_band_map_44100();
        let scalefac = [0u8; 39];

        // At fine quantization (steps 0-10), the roundtrip error should be
        // well below 100%. At coarser steps, values clip to zero and NRMSE
        // approaches 1.0 naturally — that's expected, not a bug.
        for &step in &[0.0, 5.0, 10.0] {
            let mut ix = [0i32; 576];
            let mut sign = [false; 576];
            quantize_spectrum(
                &spectrum, step, &band_map, &scalefac, false, &mut ix, &mut sign,
            );
            let recon = dequantize_spectrum(&ix, step, &band_map, &scalefac, false, &sign);

            let mut total_error: f32 = 0.0;
            let mut total_energy: f32 = 0.0;
            for i in 0..576 {
                let err = spectrum[i] - recon[i];
                total_error += err * err;
                total_energy += spectrum[i] * spectrum[i];
            }

            let nrmse = sqrt(total_error / (total_energy + EPS));
            assert!(
                nrmse < 0.5,
                "NRMSE {nrmse} >= 0.5 at step {step} — quantizer too lossy at fine step"
            );
        }
    }

    #[test]
    fn quantizer_values_monotonic_with_step() {
        // coarser step → smaller-or-equal ix value for every line
        let spectrum = make_multitone();
        let band_map = make_band_map_44100();
        let scalefac = [0u8; 39];

        let mut prev_ix = [i32::MAX; 576];

        for &step in &[0.0, 4.0, 8.0, 16.0, 32.0] {
            let mut ix = [0i32; 576];
            let mut sign = [false; 576];
            quantize_spectrum(
                &spectrum, step, &band_map, &scalefac, false, &mut ix, &mut sign,
            );

            let mut violations = 0usize;
            for i in 0..576 {
                if ix[i] > prev_ix[i] {
                    violations += 1;
                }
            }
            assert!(
                violations == 0,
                "{violations} values increased at step {step} (should be monotonic decreasing)"
            );
            prev_ix = ix;
        }
    }

    #[test]
    fn quantizer_sign_preserved() {
        let mut spectrum = [0.0f32; 576];
        for i in 0..576 {
            spectrum[i] = if i % 2 == 0 { 0.5 } else { -0.5 };
        }
        let band_map = make_band_map_44100();
        let scalefac = [0u8; 39];
        let mut ix = [0i32; 576];
        let mut sign = [false; 576];
        quantize_spectrum(
            &spectrum, 0.0, &band_map, &scalefac, false, &mut ix, &mut sign,
        );

        for i in 0..576 {
            assert_eq!(
                sign[i],
                i % 2 != 0,
                "sign[{i}] = {}, expected {}",
                sign[i],
                i % 2 != 0
            );
        }
    }

    #[test]
    fn estimate_bits_monotonic() {
        let spectrum = make_multitone();
        let band_map = make_band_map_44100();
        let scalefac = [0u8; 39];
        let mut prev_bits = u32::MAX;

        for step in (0..40).step_by(4) {
            let mut ix = [0i32; 576];
            let mut sign = [false; 576];
            quantize_spectrum(
                &spectrum,
                step as f32,
                &band_map,
                &scalefac,
                false,
                &mut ix,
                &mut sign,
            );
            let bits = crate::huffman::estimate_bits(&ix, BlockType::Long);
            assert!(
                bits <= prev_bits,
                "bits {bits} > prev {prev_bits} at step {step} — estimator not monotonic"
            );
            prev_bits = bits;
        }
    }

    // --- Inner loop convergence ---
    //
    // The rate loop binary-searches the *entire* representable `step`
    // range (see `inner_loop`'s doc comment for why an earlier version's
    // "linear scan upward from step=0 only" search was itself a bug, not
    // just an optimization opportunity) for the finest step that still
    // fits the bit budget. `assert_optimal_step` checks the actual
    // invariant that guarantees: the returned step fits, and the next
    // *finer* step (one less) does not — i.e. this is genuinely the
    // boundary the search claims to have found, not merely "some step
    // that happens to fit".
    fn assert_optimal_step(
        spectrum: &[f32; 576],
        band_map: &[usize; 576],
        scalefac: &[u8; 39],
        bit_budget: u32,
        step: f32,
    ) {
        let mut ix = [0i32; 576];
        let mut sign = [false; 576];
        quantize_spectrum(
            spectrum, step, band_map, scalefac, false, &mut ix, &mut sign,
        );
        assert!(
            crate::huffman::estimate_bits(&ix, BlockType::Long) <= bit_budget,
            "returned step {step} does not actually fit budget {bit_budget}"
        );
        if step as i32 > MIN_STEP {
            quantize_spectrum(
                spectrum,
                step - 1.0,
                band_map,
                scalefac,
                false,
                &mut ix,
                &mut sign,
            );
            assert!(
                crate::huffman::estimate_bits(&ix, BlockType::Long) > bit_budget,
                "step {step} isn't optimal -- {} (one finer) also fits budget {bit_budget}",
                step - 1.0
            );
        }
    }

    #[test]
    fn inner_loop_converges_under_generous_budget() {
        let spectrum = make_multitone();
        let band_map = make_band_map_44100();
        let scalefac = [0u8; 39];

        let result = inner_loop(
            &spectrum,
            10_000,
            &band_map,
            &scalefac,
            false,
            BlockType::Long,
        );
        assert!(result.converged, "should converge with generous budget");
        assert_optimal_step(&spectrum, &band_map, &scalefac, 10_000, result.step);
    }

    #[test]
    fn inner_loop_coarsens_under_tight_budget() {
        let spectrum = make_multitone();
        let band_map = make_band_map_44100();
        let scalefac = [0u8; 39];

        let result = inner_loop(&spectrum, 100, &band_map, &scalefac, false, BlockType::Long);
        let bits = crate::huffman::estimate_bits(&result.ix, BlockType::Long);
        assert!(
            bits <= 100,
            "tight budget (100) should be honored exactly; got {bits} bits"
        );
        assert_optimal_step(&spectrum, &band_map, &scalefac, 100, result.step);
    }

    #[test]
    fn inner_loop_finds_exact_optimal_step_for_small_signal() {
        // A single small-magnitude line -- exactly the case an earlier,
        // "only ever coarsen from step=0" version of this loop handled
        // by silently quantizing to zero forever (see `inner_loop`'s doc
        // comment): a full-range search must still find *some* nonzero,
        // budget-fitting representation for it, including steps well
        // below zero.
        let spectrum = make_tone_spectrum(10, 0.001);
        let band_map = make_band_map_44100();
        let scalefac = [0u8; 39];

        let result = inner_loop(
            &spectrum,
            10_000,
            &band_map,
            &scalefac,
            false,
            BlockType::Long,
        );
        assert!(result.converged);
        assert!(
            result.ix[10] > 0,
            "a generous budget must recover a nonzero representation for a small \
             but real signal, not silently settle for all-zero"
        );
        assert_optimal_step(&spectrum, &band_map, &scalefac, 10_000, result.step);
    }

    #[test]
    fn zero_spectrum_costs_zero_bits() {
        let ix = [0i32; 576];
        assert_eq!(crate::huffman::estimate_bits(&ix, BlockType::Long), 0);
    }

    // --- Outer loop: scalefactor amplification ---

    #[test]
    fn outer_loop_amplifies_unmasked_band() {
        // A single active band, resolved by `inner_loop`'s full-range
        // binary search alone (see that function's doc comment): with
        // nothing else in the spectrum competing for bits, the search
        // just picks whatever global step makes *this* band as precise
        // as the budget allows -- no scalefactor amplification needed
        // to satisfy even a strict SMR demand on it. An earlier version
        // of the rate loop's narrower, upward-only search couldn't do
        // that, so this test's original single-band setup only ever
        // demonstrated the outer loop's amplification path by relying
        // on that narrower search's limits, not on any real trade-off.
        //
        // To exercise scalefactor amplification meaningfully now, this
        // needs two bands *competing* for the same (tight) bit budget:
        // one with a strict SMR demand, one tolerant. A shared global
        // step can satisfy at most one well; only per-band scalefactor
        // differentiation can keep the strict band precise while
        // staying under budget.
        let band_map = make_band_map_44100();
        let strict_band = band_map[20];
        let tolerant_band = band_map[400];
        assert_ne!(
            strict_band, tolerant_band,
            "test setup needs two distinct scalefactor bands"
        );

        let mut spectrum = [0.0f32; 576];
        for i in 20..25 {
            spectrum[i] = 0.8;
        }
        for i in 400..405 {
            spectrum[i] = 0.8;
        }

        let mut smr = ScalefactorBandSmr {
            bands: [1.0f32; 22],
            short_bands: [1.0f32; 13],
        };
        smr.bands[strict_band] = 1e5; // demands very low distortion

        let result = quantize_granule(&spectrum, &smr, 60, BlockType::Long, 44100);
        assert!(
            result.scalefac.values[strict_band] > result.scalefac.values[tolerant_band],
            "strict band ({strict_band}) should get more scalefactor amplification \
             than the tolerant band ({tolerant_band}): strict={}, tolerant={}",
            result.scalefac.values[strict_band],
            result.scalefac.values[tolerant_band]
        );
    }

    #[test]
    fn outer_loop_short_block_uses_short_bands_not_long_bands() {
        // Regression test for the short-block SMR grid-mismatch bug (see
        // `ScalefactorBandSmr::short_bands`'s doc comment and
        // `docs/investigation-log.md` §11): before this fix, `quantize_granule` had
        // no `short_bands` field to read at all and consulted `bands`
        // (the long-block grid) for every block type, including `Short`.
        //
        // `bands` and `short_bands` are set here to demand the *opposite*
        // thing at the two competing bands under test -- this can only
        // pass if the outer loop actually reads `short_bands` for a
        // `Short` granule; reading `bands` instead would amplify
        // `tolerant_band`, not `strict_band`, the reverse of what's
        // asserted below.
        let band_map = build_band_map(0, BlockType::Short);
        let strict_band = band_map[5];
        let tolerant_band = band_map[300];
        assert_ne!(
            strict_band, tolerant_band,
            "test setup needs two distinct short scalefactor bands"
        );

        let mut spectrum = [0.0f32; 576];
        for i in 5..10 {
            spectrum[i] = 0.8;
        }
        for i in 300..305 {
            spectrum[i] = 0.8;
        }

        let mut smr = ScalefactorBandSmr {
            bands: [1.0f32; 22],
            short_bands: [1.0f32; 13],
        };
        smr.bands[tolerant_band] = 1e5; // wrong-grid demand -- must be ignored for Short
        smr.short_bands[strict_band] = 1e5; // right-grid demand -- must be honored

        let result = quantize_granule(&spectrum, &smr, 60, BlockType::Short, 44100);
        assert!(
            result.scalefac.values[strict_band] > result.scalefac.values[tolerant_band],
            "strict short band ({strict_band}) should get more scalefactor amplification \
             than the tolerant one ({tolerant_band}): strict={}, tolerant={}",
            result.scalefac.values[strict_band],
            result.scalefac.values[tolerant_band]
        );
    }

    #[test]
    fn outer_loop_uniform_smr_stays_low() {
        let spectrum = make_multitone();
        let smr = ScalefactorBandSmr {
            bands: [1.0f32; 22],
            short_bands: [1.0f32; 13],
        };

        let result = quantize_granule(&spectrum, &smr, 1000, BlockType::Long, 44100);
        let max_sf: u8 = *result.scalefac.values.iter().max().unwrap_or(&0);
        assert!(
            max_sf <= 2,
            "uniform SMR=1 should keep scalefactors low, max={max_sf}"
        );
    }

    // --- Non-convergence / termination ---

    #[test]
    fn outer_loop_terminates_with_impossible_demands() {
        // Full-scale spectrum + a bit budget too tight to leave any real
        // headroom + an SMR demand steep enough that no amount of
        // scalefactor amplification within MAX_OUTER_ITERATIONS satisfies
        // it: each outer pass keeps every band under `distort > allowed`,
        // so `retry` never goes false and `scalefac[b]` never reaches
        // MAX_SCALEFAC either (8 iterations << 15) -- this is the
        // "designed to oscillate/never settle" case the DoD asks for,
        // not just an input that happens to be extreme.
        let mut spectrum = [0.0f32; 576];
        for i in 0..576 {
            spectrum[i] = 0.9;
        }

        let smr = ScalefactorBandSmr {
            bands: [1000.0f32; 22],
            short_bands: [1000.0f32; 13],
        };

        let result = quantize_granule(&spectrum, &smr, 50, BlockType::Long, 44100);

        assert!(
            !result.converged,
            "impossible SMR demand within MAX_OUTER_ITERATIONS should report non-convergence, \
             not silently claim success"
        );
        for &v in &result.ix {
            assert!(v >= 0, "ix values must be non-negative");
        }
        let bits = crate::huffman::estimate_bits(&result.ix, BlockType::Long);
        assert!(bits < 20_000, "bit count {bits} should be bounded");
    }

    #[test]
    fn inner_loop_reports_non_convergence_at_step_cap() {
        // An amplitude large enough that even the maximum representable
        // step (~45, the 8-bit `global_gain` field's ceiling once biased)
        // still doesn't quantize down to 0 -- since 0 bits trivially
        // satisfies any non-negative budget, a spectrum that *can* reach
        // all-zero always eventually "converges" no matter how tight the
        // budget; only a magnitude the step cap itself can't tame ever
        // exercises `converged == false` from the inner loop's own side
        // (as opposed to the outer loop's non-convergence tested above).
        let spectrum = make_tone_spectrum(10, 1.0e6);
        let band_map = make_band_map_44100();
        let scalefac = [0u8; 39];

        let result = inner_loop(&spectrum, 0, &band_map, &scalefac, false, BlockType::Long);
        assert!(
            !result.converged,
            "a magnitude the step cap can't tame should report non-convergence"
        );
        // The final requantize must use the *clamped* step, matching the
        // clamped global_gain -- see the M5 review notes on the
        // step-cap/global_gain mismatch this guards against.
        let expected_step = (MAX_GLOBAL_GAIN - GLOBAL_GAIN_BIAS) as f32;
        assert_eq!(result.step, expected_step);
        assert_eq!(result.global_gain, MAX_GLOBAL_GAIN);
    }

    #[test]
    fn quantize_silence_produces_all_zeros() {
        let spectrum = [0.0f32; 576];
        let smr = ScalefactorBandSmr {
            bands: [1.0f32; 22],
            short_bands: [1.0f32; 13],
        };

        let result = quantize_granule(&spectrum, &smr, 1000, BlockType::Long, 44100);
        for &v in &result.ix {
            assert_eq!(v, 0);
        }
        for &s in &result.sign {
            assert!(!s);
        }
    }

    #[test]
    fn scalefac_value_scale_flag() {
        // scalefac_scale=false -> ×2, scalefac_scale=true -> ×4 (see
        // scalefac_value's doc comment for the derivation/cross-check).
        assert_eq!(scalefac_value(3, false), 6);
        assert_eq!(scalefac_value(3, true), 12);
        assert_eq!(scalefac_value(15, false), 30);
        assert_eq!(scalefac_value(31, true), 124);
    }
}
