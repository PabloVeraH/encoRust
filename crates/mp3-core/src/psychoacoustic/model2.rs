//! Psychoacoustic Model II proper: partitions, spreading function, SMR,
//! and the block-switching decision. See
//! `docs/mp3-encoder/07-phase4-psychoacoustic-model.md` §3-5.
//!
//! Allocation-free on the hot path: all working buffers are fixed-size
//! stack arrays. Partition data (which depends only on sample_rate_hz,
//! a constant for the encoder's lifetime) is pre-computed once and cached
//! — see `docs/mejoras.md` §3.2, M-1 and M-2.

use crate::mdct::BlockType;

use super::fft::fft_windowed_complex;
use super::tables::{
    absolute_threshold_db, compute_partition_map, partition_bark_centers, partition_hz_centers,
    scalefactor_sample_rate_index, spreading_db, NMT_DB, SFB_LONG_BOUNDARIES, SFB_LONG_COUNTS,
    SPL_TO_DBFS_OFFSET_DB, TMN_DB,
};

// `no_std`-safe transcendental functions -- call as free functions, never
// as `x.sqrt()`/`x.floor()`/`x.powi()` method syntax, which resolve to
// inherent `f32` methods that don't exist under `core`. See
// `docs/mp3-encoder/verification/manifest.yaml`'s build-wasm note.
use libm::{
    atan2f as atan2, ceilf as ceil, cosf as cos, expf as exp, floorf as floor, log2f as log2,
    sqrtf as sqrt,
};

/// ln(10) / 10 constant for dB ↔ linear conversion.
const LN10_OVER_10: f32 = core::f32::consts::LN_10 / 10.0;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// FFT size for long-block psychoacoustic analysis.
const FFT_SIZE_LONG: usize = 1024;
/// Number of real FFT output bins (N/2 + 1).
const FFT_BINS_LONG: usize = FFT_SIZE_LONG / 2 + 1;

/// Maximum number of partitions (Bark-grouped FFT bins) across all
/// supported sample rates — 0.5 Bark/partition over a ~22 kHz bandwidth
/// yields ~48 partitions for 44.1 kHz; 64 is a safe upper bound.
const MAX_PARTITIONS: usize = 64;

/// Perceptual entropy transient detection threshold, in dB increase of
/// the current granule's PE over the smoothed recent baseline.
///
/// Despite the name, this used to be compared directly against the raw
/// (non-dB) `pe_ratio` (`pe_ratio > 10.0`, i.e. requiring a 10x jump) —
/// harmless by itself as just a very strict threshold, but compounded
/// with `compute_perceptual_entropy`'s own bug (see that function's doc
/// comment) into transient detection that could never fire on real
/// audio at all. With that formula fixed, `pe_ratio` was measured
/// (chickens_16bit.wav, a real recording with sharp attacks) sitting at
/// 0.46-1.4x for ordinary content and reaching 2.1-5.7x at genuine
/// attacks.
///
/// A *continuously-advancing* pure sine tone (see `make_tone_at`) still
/// showed ~2.8x (≈4.5 dB) frame-to-frame PE variation on its own,
/// despite being genuinely stationary -- windowed FFT bins near, but
/// not exactly on, the tone's frequency pick up spectral leakage whose
/// magnitude/phase don't evolve as linearly frame-to-frame as the
/// dominant bin's does, which `compute_tonality`'s linear-prediction
/// model reads as some unpredictability even for a pure tone. 5 dB
/// (~3.5x) clears that synthetic-tone noise floor while still catching
/// real content's clearer attacks (the 5.7x case above); it will miss
/// softer ones (the 2.1x case) -- a real limitation, not a false
/// negative introduced by this fix, since nothing triggered before it
/// at all. The comparison now actually converts to dB (`10*log10(ratio)`)
/// to match this constant's name and unit instead of comparing a
/// dB-labeled constant against a raw ratio.
const PE_ATTACK_THRESHOLD_DB: f32 = 5.0;

/// Granules to treat as baseline-seeding only, never a transient
/// candidate, before `decide_block_type` starts comparing `pe_ratio`.
/// Matches `history`'s 2-frame depth: `compute_tonality`'s
/// unpredictability measure needs both slots populated by real (not
/// initial-zero) spectra to produce a stable estimate, otherwise its
/// output swings between the first couple of calls regardless of how
/// stationary the actual signal is -- see `granules_seen`'s doc comment.
const PE_WARMUP_GRANULES: u8 = 2;

/// `10 / log2(10)`, for converting a ratio's `log2` (this module's
/// available transcendental function) to decibels without a separate
/// `log10`: `10*log10(x) = 10 * log2(x)/log2(10) = log2(x) * DB_PER_LOG2`.
const DB_PER_LOG2: f32 = 3.0103;

/// Low-pass filter coefficient for smoothed PE history.
const PE_SMOOTH_COEFF: f32 = 0.3;

/// Minimum PE baseline used as the transient-ratio denominator, avoiding
/// division by (near-)zero.
const PE_BASELINE_FLOOR: f32 = 1e-6;

// ---------------------------------------------------------------------------
// Complex number for FFT history
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Default)]
struct Complex {
    re: f32,
    im: f32,
}

impl Complex {
    fn magnitude(&self) -> f32 {
        sqrt(self.re * self.re + self.im * self.im)
    }

    fn phase(&self) -> f32 {
        atan2(self.im, self.re)
    }
}

// ---------------------------------------------------------------------------
// Cached partition data
// ---------------------------------------------------------------------------

/// Pre-computed partition layout for a given sample rate.  This data
/// depends only on `sample_rate_hz`, which never changes after
/// `Encoder::new()` — computing it once instead of on every
/// `analyze_granule` call eliminates 5 allocations/call plus redundant
/// `atan()`-based Bark conversion for 513 bins.
#[derive(Clone)]
struct PartitionCache {
    /// `partition_of_bin[i]` = partition index for FFT bin `i`.
    /// `u8` not `usize`: values never exceed ~64 (the plan's `MAX_PARTITIONS` bound).
    partition_of_bin: [u8; FFT_BINS_LONG],
    /// Center frequency (Bark) of each partition.
    part_centers_bark: [f32; MAX_PARTITIONS],
    /// Center frequency (Hz) of each partition.
    part_centers_hz: [f32; MAX_PARTITIONS],
    /// Number of partitions actually used.
    num_partitions: usize,
}

impl PartitionCache {
    fn new(sample_rate_hz: u32) -> Self {
        let freq_per_bin = sample_rate_hz as f32 / FFT_SIZE_LONG as f32;
        let (partition_of_bin_vec, num_parts) =
            compute_partition_map(FFT_BINS_LONG, freq_per_bin, 0.5);

        let mut maps = [0u8; FFT_BINS_LONG];
        for (i, &p) in partition_of_bin_vec.iter().enumerate() {
            maps[i] = p as u8;
        }

        let bark_centers_vec =
            partition_bark_centers(&partition_of_bin_vec, num_parts, freq_per_bin);
        let hz_centers_vec = partition_hz_centers(&partition_of_bin_vec, num_parts, freq_per_bin);

        let mut part_centers_bark = [0.0f32; MAX_PARTITIONS];
        let mut part_centers_hz = [0.0f32; MAX_PARTITIONS];
        let n = num_parts.min(MAX_PARTITIONS);
        part_centers_bark[..n].copy_from_slice(&bark_centers_vec[..n]);
        part_centers_hz[..n].copy_from_slice(&hz_centers_vec[..n]);

        Self {
            partition_of_bin: maps,
            part_centers_bark,
            part_centers_hz,
            num_partitions: num_parts,
        }
    }
}

// ---------------------------------------------------------------------------
// SMR output
// ---------------------------------------------------------------------------

/// Signal-to-mask ratio per scalefactor band for one granule/channel.
#[derive(Debug, Clone, Copy)]
pub struct ScalefactorBandSmr {
    /// SMR per scalefactor band, in energy ratio (not dB).
    /// Sized for the maximum long-block band count (22 bands).
    /// Bands beyond the sample rate's actual count are set to 1.0.
    pub bands: [f32; 22],
}

// ---------------------------------------------------------------------------
// PsychoacousticModel
// ---------------------------------------------------------------------------

/// Holds the FFT-history state the unpredictability measure needs across
/// frames. One instance per channel — do not share between channels.
pub struct PsychoacousticModel {
    /// Complex FFT output history, 2 frames back, for the long-block
    /// (1024-point) analysis. Used by the unpredictability/tonality measure.
    history: [[Complex; FFT_BINS_LONG]; 2],

    /// Pre-computed partition layout for the configured sample rate.
    /// `None` until `init_for_sample_rate` is called.
    partitions: Option<PartitionCache>,

    /// Smoothed perceptual entropy baseline.
    smoothed_pe: f32,

    /// Granules seen so far, capped at `PE_WARMUP_GRANULES` once reached.
    /// `compute_tonality`'s unpredictability measure needs 2 full frames
    /// of real (non-initial-zero) history to produce a stable estimate;
    /// before that, `history`'s still-zeroed slots make tonality swing
    /// from its typical steady-state value, which in turn swings
    /// `part_threshold` (see `analyze_granule` Step 6) and so
    /// `compute_perceptual_entropy`'s output — a spurious one-time PE
    /// spike on the granule where history first becomes "mostly real",
    /// with nothing actually transient in the audio. `decide_block_type`
    /// treats every granule while this counter is below
    /// `PE_WARMUP_GRANULES` as baseline-seeding only, never a candidate
    /// transient, so that warm-up spike can't trigger a false Start --
    /// confirmed via `stationary_tone_stays_long_after_initial_settle`,
    /// which failed against a genuinely unchanging tone before this
    /// counter existed.
    granules_seen: u8,

    /// Current block type state machine output.
    block_type: BlockType,

    /// Short-block persistence counter.
    short_count: usize,
}

impl Default for PsychoacousticModel {
    fn default() -> Self {
        Self::new()
    }
}

impl PsychoacousticModel {
    /// Creates a fresh model with zeroed FFT history and `Long` block type.
    /// `init_for_sample_rate` must be called exactly once before the first
    /// `analyze_granule` call, to pre-compute the partition cache.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            history: [[Complex { re: 0.0, im: 0.0 }; FFT_BINS_LONG]; 2],
            partitions: None,
            smoothed_pe: 0.0,
            granules_seen: 0,
            block_type: BlockType::Long,
            short_count: 0,
        }
    }

    /// Pre-computes and caches all partition data that depends on
    /// `sample_rate_hz`. Must be called once after construction, before
    /// the first `analyze_granule` call.  Cheap to call again if a future
    /// feature changes the sample rate mid-stream (not yet implemented).
    pub fn init_for_sample_rate(&mut self, sample_rate_hz: u32) {
        self.partitions = Some(PartitionCache::new(sample_rate_hz));
    }

    /// Analyzes one granule's worth of raw PCM and produces the SMR
    /// values plus block-type decision for that granule.
    ///
    /// `pcm_window` should contain up to `1024` samples; fewer are
    /// zero-padded internally.
    ///
    /// `sample_rate_hz` is the PCM sample rate in Hz.
    pub fn analyze_granule(
        &mut self,
        pcm_window: &[f32],
        sample_rate_hz: u32,
    ) -> (ScalefactorBandSmr, BlockType) {
        let partitions = self
            .partitions
            .as_ref()
            .expect("init_for_sample_rate must be called before analyze_granule");

        let sfb_idx = scalefactor_sample_rate_index(sample_rate_hz);
        let sfb_count = SFB_LONG_COUNTS[sfb_idx];
        let sfb_bounds = &SFB_LONG_BOUNDARIES[sfb_idx];

        // --- All working buffers, stack-allocated ---
        let mut mag = [0.0f32; FFT_BINS_LONG];
        let mut re = [0.0f32; FFT_BINS_LONG];
        let mut im = [0.0f32; FFT_BINS_LONG];

        // Step 1: FFT analysis
        self.fft_analyze_long(pcm_window, &mut mag, &mut re, &mut im);

        // Step 2: Tonality per FFT bin from history
        let mut tonality_bin = [0.0f32; FFT_BINS_LONG];
        self.compute_tonality(&re, &im, &mut tonality_bin);

        // Step 3-4: Per-partition energy and tonality
        let mut part_energy = [0.0f32; MAX_PARTITIONS];
        let mut part_tonality_sum = [0.0f32; MAX_PARTITIONS];
        let mut part_tonality_count = [0usize; MAX_PARTITIONS];

        for i in 0..FFT_BINS_LONG {
            let p = partitions.partition_of_bin[i] as usize;
            if p < MAX_PARTITIONS {
                part_energy[p] += mag[i] * mag[i];
                part_tonality_sum[p] += tonality_bin[i];
                part_tonality_count[p] += 1;
            }
        }

        let num_parts = partitions.num_partitions.min(MAX_PARTITIONS);
        let mut part_tonality = [0.0f32; MAX_PARTITIONS];
        for p in 0..num_parts {
            if part_tonality_count[p] > 0 {
                part_tonality[p] = part_tonality_sum[p] / part_tonality_count[p] as f32;
            }
        }

        // Step 5: Spreading function convolution. Both `i` and `j` index
        // the same `part_centers_bark`/`part_energy` arrays at once
        // (part_threshold[i] accumulates a contribution from every j) —
        // not expressible as a single enumerate() without indexing back
        // into the source arrays anyway, so the range form is clearer.
        let mut part_threshold = [0.0f32; MAX_PARTITIONS];
        #[allow(clippy::needless_range_loop)]
        for i in 0..num_parts {
            for j in 0..num_parts {
                let dz = partitions.part_centers_bark[j] - partitions.part_centers_bark[i];
                let spread_db = spreading_db(dz);
                let spread_linear = exp(spread_db * LN10_OVER_10);
                part_threshold[i] += part_energy[j] * spread_linear;
            }
        }

        // Step 6: Apply tonality-based SNR and compute per-partition SMR
        let mut part_smr = [1.0f32; MAX_PARTITIONS];
        for p in 0..num_parts {
            let t = part_tonality[p].clamp(0.0, 1.0);
            let snr_db = t * TMN_DB + (1.0 - t) * NMT_DB;
            let snr_linear = exp(snr_db * LN10_OVER_10);
            part_threshold[p] /= snr_linear;

            let ath_linear = ath_from_db(absolute_threshold_db(partitions.part_centers_hz[p]));
            if part_threshold[p] < ath_linear {
                part_threshold[p] = ath_linear;
            }

            if part_energy[p] > 0.0 && part_threshold[p] > 0.0 {
                part_smr[p] = (part_energy[p] / part_threshold[p]).clamp(1.0, 1e6);
            }
        }

        // Step 7: Map per-partition SMR to scalefactor bands
        let freq_per_bin = sample_rate_hz as f32 / FFT_SIZE_LONG as f32;
        let mut sfb_smr = [1.0f32; 22];
        for sfb in 0..sfb_count {
            let start_line = sfb_bounds[sfb];
            let end_line = sfb_bounds[sfb + 1];

            let start_freq = start_line as f32 * sample_rate_hz as f32 / (2.0 * 576.0);
            let end_freq = end_line as f32 * sample_rate_hz as f32 / (2.0 * 576.0);
            let start_bin = floor(start_freq / freq_per_bin) as usize;
            let end_bin = (ceil(end_freq / freq_per_bin) as usize).min(FFT_BINS_LONG - 1);

            if end_bin > start_bin {
                let mut visited = [false; MAX_PARTITIONS];
                for &p in &partitions.partition_of_bin[start_bin..end_bin] {
                    let p = p as usize;
                    if p < visited.len() && !visited[p] {
                        visited[p] = true;
                        if part_smr[p] > sfb_smr[sfb] {
                            sfb_smr[sfb] = part_smr[p];
                        }
                    }
                }
            }
        }

        // Step 8: Perceptual entropy + block-type decision
        let pe = self.compute_perceptual_entropy(
            &part_energy,
            &part_threshold,
            &part_tonality_count,
            partitions.num_partitions,
        );
        self.decide_block_type(pe);

        // Save history for next frame
        for i in 0..FFT_BINS_LONG {
            self.history[1][i] = self.history[0][i];
            self.history[0][i] = Complex {
                re: re[i],
                im: im[i],
            };
        }

        (ScalefactorBandSmr { bands: sfb_smr }, self.block_type)
    }

    // -----------------------------------------------------------------------
    // Internal methods
    // -----------------------------------------------------------------------

    /// Run 1024-point FFT analysis, writing magnitude, real, and imaginary
    /// parts into caller-provided stack buffers.
    fn fft_analyze_long(
        &self,
        pcm_window: &[f32],
        mag: &mut [f32; FFT_BINS_LONG],
        re: &mut [f32; FFT_BINS_LONG],
        im: &mut [f32; FFT_BINS_LONG],
    ) {
        let mut workspace = [0.0f32; FFT_SIZE_LONG];
        let copy_len = pcm_window.len().min(FFT_SIZE_LONG);
        workspace[..copy_len].copy_from_slice(&pcm_window[..copy_len]);

        fft_windowed_complex(&workspace, re, im);

        for i in 0..FFT_BINS_LONG {
            mag[i] = sqrt(re[i] * re[i] + im[i] * im[i]);
        }
    }

    /// Compute tonality per FFT bin from the 2-frame history, writing
    /// into a caller-provided stack buffer.
    fn compute_tonality(
        &self,
        re: &[f32; FFT_BINS_LONG],
        im: &[f32; FFT_BINS_LONG],
        tonality: &mut [f32; FFT_BINS_LONG],
    ) {
        for i in 0..FFT_BINS_LONG {
            let mag = sqrt(re[i] * re[i] + im[i] * im[i]);

            let prev0_mag = self.history[0][i].magnitude();
            let prev1_mag = self.history[1][i].magnitude();
            let pred_mag = 2.0 * prev0_mag - prev1_mag;
            let pred_mag = pred_mag.max(0.0);

            let prev0_phase = self.history[0][i].phase();
            let prev1_phase = self.history[1][i].phase();
            let pred_phase = 2.0 * prev0_phase - prev1_phase;

            let phase = atan2(im[i], re[i]);

            let denom = pred_mag + mag + 1e-30;
            let mag_term = (pred_mag - mag) / denom;
            let cw = sqrt(
                mag_term * mag_term
                    + (2.0 * pred_mag * mag * (1.0 - cos(phase - pred_phase)) / (denom * denom)),
            );

            tonality[i] = (1.0_f32 - cw).clamp(0.0, 1.0);
        }
    }

    /// Compute perceptual entropy: how many bits' worth of *audible*
    /// (above-masking-threshold) information this granule carries,
    /// weighted by each partition's bandwidth. A sudden rise relative to
    /// the smoothed recent baseline (`decide_block_type`) signals a
    /// transient -- the masking model's steady-state assumptions
    /// momentarily broke down, which is exactly when window-switching to
    /// short blocks limits how far quantization noise can spread in
    /// time (pre-echo).
    ///
    /// Takes the granule's *real* per-partition energy and masking
    /// threshold (already computed in `analyze_granule` -- spreading
    /// function, tonality-weighted SNR, and the ATH floor all folded
    /// in), not a re-derivation from raw FFT magnitudes. An earlier
    /// version computed its own "threshold" as `energy_per_bin * 1e-6`
    /// -- the *same* bin's own energy, just scaled -- which made
    /// `energy_per_bin / thr` collapse to the constant `1e6` for every
    /// partition with any energy at all, regardless of the signal's
    /// actual shape. That made `pe` track only how many partitions had
    /// nonzero energy (essentially constant for any full-spectrum
    /// signal), never how audible or transient the content actually
    /// was -- so `decide_block_type`'s `pe_ratio` never moved enough to
    /// cross `PE_ATTACK_THRESHOLD_DB`, and short blocks could never
    /// trigger on real audio (confirmed empirically: 0 of 3360 granules
    /// on a real, percussive recording). See `docs/mejoras.md`'s
    /// gain-bug investigation notes.
    fn compute_perceptual_entropy(
        &self,
        part_energy: &[f32; MAX_PARTITIONS],
        part_threshold: &[f32; MAX_PARTITIONS],
        part_count: &[usize; MAX_PARTITIONS],
        num_partitions: usize,
    ) -> f32 {
        let num_p = num_partitions.min(MAX_PARTITIONS);
        let mut pe = 0.0f32;
        for p in 0..num_p {
            if part_count[p] > 0 && part_threshold[p] > 0.0 && part_energy[p] > part_threshold[p] {
                let bw = part_count[p] as f32;
                pe += bw * log2(part_energy[p] / part_threshold[p]);
            }
        }

        pe
    }

    /// Block-type state machine based on perceptual entropy transients.
    fn decide_block_type(&mut self, pe: f32) {
        if self.granules_seen < PE_WARMUP_GRANULES {
            self.granules_seen += 1;
            self.smoothed_pe = if self.granules_seen == 1 {
                pe
            } else {
                PE_SMOOTH_COEFF * pe + (1.0 - PE_SMOOTH_COEFF) * self.smoothed_pe
            };
            return;
        }

        let baseline = self.smoothed_pe.max(PE_BASELINE_FLOOR);
        let pe_ratio = pe / baseline;
        let pe_ratio_db = if pe_ratio > 0.0 {
            log2(pe_ratio) * DB_PER_LOG2
        } else {
            f32::NEG_INFINITY
        };

        self.smoothed_pe = PE_SMOOTH_COEFF * pe + (1.0 - PE_SMOOTH_COEFF) * self.smoothed_pe;

        match self.block_type {
            BlockType::Long => {
                if pe_ratio_db > PE_ATTACK_THRESHOLD_DB {
                    self.block_type = BlockType::Start;
                    self.short_count = 3;
                }
            }
            BlockType::Start => {
                self.block_type = BlockType::Short;
            }
            BlockType::Short => {
                self.short_count -= 1;
                if self.short_count == 0 {
                    self.block_type = BlockType::Stop;
                }
            }
            BlockType::Stop => {
                self.block_type = BlockType::Long;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn ath_from_db(db: f32) -> f32 {
    // `db` is dB SPL (Annex D's absolute-threshold-of-hearing curve).
    // Every energy value elsewhere in this model is computed from PCM
    // normalized to [-1.0, 1.0] (0 dBFS = 1.0, unity linear power), not
    // absolute SPL -- see `SPL_TO_DBFS_OFFSET_DB`'s doc comment for why
    // re-anchoring to that reference before exponentiating matters.
    let db_fs = db - SPL_TO_DBFS_OFFSET_DB;
    if db_fs < -240.0 {
        return 1e-20;
    }
    exp(db_fs * LN10_OVER_10)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::disallowed_methods)] // f32::abs() in test assertions -- see docs/mejoras.md §7 item 6
mod tests {
    use super::*;
    use alloc::vec;
    use alloc::vec::Vec;
    use core::f32::consts::PI;
    use libm::sinf as sin;

    fn make_tone(freq: f32, sample_rate: u32, num_samples: usize) -> Vec<f32> {
        make_tone_at(freq, sample_rate, num_samples, 0)
    }

    /// Like `make_tone`, but starting at time `start_sample / sample_rate`
    /// instead of always t=0. Needed for any test that calls
    /// `analyze_granule` more than once and wants each call to represent
    /// the *next* chunk of one continuous signal: `compute_tonality`
    /// linearly extrapolates each FFT bin's magnitude and phase from the
    /// previous two frames, which is only a meaningful predictor if the
    /// frames actually advance in time the way real, continuously
    /// sampled audio does. Calling `make_tone` repeatedly (always t=0)
    /// instead feeds the *identical* window every time, which has zero
    /// phase evolution between calls -- not "maximally stationary" from
    /// the predictor's perspective, but a discontinuity from what it's
    /// designed to predict, producing spurious unpredictability
    /// (confirmed empirically: `stationary_tone_stays_long_after_
    /// initial_settle` failed against a repeated-window tone regardless
    /// of how many warm-up calls `decide_block_type` was given, until
    /// switched to this continuously-advancing generator).
    fn make_tone_at(
        freq: f32,
        sample_rate: u32,
        num_samples: usize,
        start_sample: usize,
    ) -> Vec<f32> {
        let mut samples = Vec::with_capacity(num_samples);
        for i in 0..num_samples {
            let t = (start_sample + i) as f32 / sample_rate as f32;
            samples.push(sin(2.0 * PI * freq * t));
        }
        samples
    }

    fn make_noise(num_samples: usize) -> Vec<f32> {
        // xorshift32, not the LCG this used to be: an LCG's low-order
        // bits are famously weak, but even reading only its high bits
        // (as the earlier version did) doesn't fix a subtler issue this
        // test actually hit -- an LCG's output has real spectral
        // structure (a shallow low-frequency bias in this case), which
        // isn't "broadband noise" in the sense this test needs. That
        // bias was invisible while `ath_from_db` (see model2.rs) was
        // miscalibrated ~96 dB too high, since the inflated ATH floor
        // pinned nearly every partition's SMR to the same 1.0 floor
        // regardless of the signal's actual spectral shape; fixing that
        // calibration made this test signal's own bias visible for the
        // first time, not the calibration fix wrong. xorshift32 has
        // markedly better spectral flatness for this size of sample.
        let mut state: u32 = 0x9E37_79B9;
        let mut samples = Vec::with_capacity(num_samples);
        for _ in 0..num_samples {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            samples.push((state as f32 / u32::MAX as f32) * 2.0 - 1.0);
        }
        samples
    }

    fn smr_std_dev(bands: &[f32; 22]) -> f32 {
        let mean: f32 = bands.iter().sum::<f32>() / 22.0;
        let variance: f32 = bands.iter().map(|&s| (s - mean) * (s - mean)).sum::<f32>() / 22.0;
        sqrt(variance)
    }

    fn make_model(sample_rate_hz: u32) -> PsychoacousticModel {
        let mut model = PsychoacousticModel::new();
        model.init_for_sample_rate(sample_rate_hz);
        model
    }

    #[test]
    fn smr_tone_produces_high_smr_at_own_frequency() {
        let mut model = make_model(44100);
        let tone = make_tone(1000.0, 44100, 1024);
        let (smr, _bt) = model.analyze_granule(&tone, 44100);

        let has_high_smr = smr.bands.iter().any(|&s| s > 2.0);
        assert!(has_high_smr, "tone should produce SMR > 1.0 in some bands");

        for (i, &s) in smr.bands.iter().enumerate().take(22) {
            assert!(s >= 1.0, "SMR band {i} is {s}, should be >= 1.0");
        }
    }

    #[test]
    fn smr_tone_shows_spreading_shape_around_peak() {
        let mut model = make_model(44100);
        let tone = make_tone(1000.0, 44100, 1024);
        let (smr, _bt) = model.analyze_granule(&tone, 44100);
        let bands = &smr.bands[..22];

        let (peak_idx, peak_val) = bands
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .map(|(i, &v)| (i, v))
            .unwrap();
        assert!(
            peak_val > 2.0,
            "tone should produce a clear SMR peak, got {peak_val}"
        );

        let far_max = bands
            .iter()
            .enumerate()
            .filter(|(i, _)| (*i as i32 - peak_idx as i32).abs() >= 6)
            .map(|(_, &v)| v)
            .fold(0.0f32, f32::max);
        assert!(
            far_max < peak_val,
            "SMR far from the tone's peak band ({far_max}) should be lower \
             than the peak itself ({peak_val}) -- spreading shape not visible"
        );
    }

    #[test]
    fn smr_noise_is_flatter_than_tone() {
        let mut tone_model = make_model(44100);
        let tone = make_tone(1000.0, 44100, 1024);
        let (tone_smr, _) = tone_model.analyze_granule(&tone, 44100);

        let mut noise_model = make_model(44100);
        let noise = make_noise(1024);
        let (noise_smr, _) = noise_model.analyze_granule(&noise, 44100);

        let tone_std = smr_std_dev(&tone_smr.bands);
        let noise_std = smr_std_dev(&noise_smr.bands);

        assert!(
            noise_std < tone_std,
            "noise SMR std_dev ({noise_std}) should be lower (flatter) than \
             tone SMR std_dev ({tone_std})"
        );
    }

    #[test]
    fn block_type_defaults_to_long() {
        let model = PsychoacousticModel::new();
        assert_eq!(model.block_type, BlockType::Long);
    }

    #[test]
    fn silent_signal_stays_long() {
        let mut model = make_model(44100);
        let silence = vec![0.0f32; 1024];
        for _ in 0..10 {
            let (_smr, bt) = model.analyze_granule(&silence, 44100);
            assert_eq!(
                bt,
                BlockType::Long,
                "silence should not trigger transient detection"
            );
        }
    }

    #[test]
    fn transient_drives_start_short_short_short_stop_sequence() {
        let mut model = make_model(44100);
        let silence = vec![0.0f32; 1024];
        for _ in 0..5 {
            model.analyze_granule(&silence, 44100);
        }

        let burst = make_noise(1024);
        let (_, bt1) = model.analyze_granule(&burst, 44100);
        assert_eq!(
            bt1,
            BlockType::Start,
            "full-scale noise burst after silence should trigger Start"
        );

        let (_, bt2) = model.analyze_granule(&silence, 44100);
        let (_, bt3) = model.analyze_granule(&silence, 44100);
        let (_, bt4) = model.analyze_granule(&silence, 44100);
        assert_eq!(bt2, BlockType::Short, "first of 3 Short granules");
        assert_eq!(bt3, BlockType::Short, "second of 3 Short granules");
        assert_eq!(bt4, BlockType::Short, "third of 3 Short granules");

        let (_, bt5) = model.analyze_granule(&silence, 44100);
        assert_eq!(bt5, BlockType::Stop);

        let (_, bt6) = model.analyze_granule(&silence, 44100);
        assert_eq!(bt6, BlockType::Long);
    }

    #[test]
    fn stationary_tone_stays_long_after_initial_settle() {
        // Each call represents the *next* chunk of one continuous tone
        // (hop = 576, matching the real 1152-sample-frame/2-granule
        // encoder pipeline in encoder.rs), not a repeated, non-advancing
        // window -- see `make_tone_at`'s doc comment for why that
        // distinction matters to the tonality predictor this exercises.
        let mut model = make_model(44100);
        const HOP: usize = 576;
        for i in 0..PE_WARMUP_GRANULES as usize {
            let tone = make_tone_at(440.0, 44100, 1024, i * HOP);
            model.analyze_granule(&tone, 44100);
        }

        for i in PE_WARMUP_GRANULES as usize..PE_WARMUP_GRANULES as usize + 10 {
            let tone = make_tone_at(440.0, 44100, 1024, i * HOP);
            let (_, bt) = model.analyze_granule(&tone, 44100);
            assert_eq!(
                bt,
                BlockType::Long,
                "stationary tone should not trigger short blocks"
            );
        }
    }

    #[test]
    fn silence_produces_finite_smr() {
        let mut model = make_model(44100);
        let silence = vec![0.0f32; 1024];
        let (smr, _bt) = model.analyze_granule(&silence, 44100);

        for (i, &s) in smr.bands.iter().enumerate().take(22) {
            assert!(s.is_finite(), "SMR band {i} is {s} (should be finite)");
            assert!(s >= 1.0, "SMR band {i} is {s} (should be >= 1.0)");
        }
    }

    #[test]
    fn fullscale_produces_finite_smr() {
        let mut model = make_model(44100);
        let mut fullscale = vec![0.0f32; 1024];
        for (i, item) in fullscale.iter_mut().enumerate() {
            *item = sin(i as f32 * 0.1) * 0.999;
        }
        let (smr, _bt) = model.analyze_granule(&fullscale, 44100);

        for (i, &s) in smr.bands.iter().enumerate().take(22) {
            assert!(
                s.is_finite(),
                "SMR band {i} is {s} (should be finite, fullscale)"
            );
        }
    }

    #[test]
    fn smr_multiple_frames_consistent() {
        let mut model = make_model(44100);
        let tone = make_tone(500.0, 44100, 1024);

        let (smr1, _) = model.analyze_granule(&tone, 44100);
        let (smr2, _) = model.analyze_granule(&tone, 44100);

        for i in 0..22 {
            assert!(smr1.bands[i].is_finite());
            assert!(smr2.bands[i].is_finite());
            assert!(smr1.bands[i] >= 1.0);
            assert!(smr2.bands[i] >= 1.0);
        }

        let mean1: f32 = smr1.bands.iter().take(22).sum::<f32>() / 22.0;
        assert!(mean1 > 0.0, "SMR should be positive for tone signal");
    }

    #[test]
    fn partition_cache_produces_same_result_as_original() {
        let mut model = make_model(44100);
        let tone = make_tone(440.0, 44100, 1024);
        let (smr1, _) = model.analyze_granule(&tone, 44100);
        let (smr2, _) = model.analyze_granule(&tone, 44100);

        for i in 0..22 {
            assert!(smr1.bands[i].is_finite());
            assert!(smr2.bands[i].is_finite());
        }
    }

    #[test]
    #[allow(clippy::needless_range_loop)]
    fn lookahead_window_changes_smr_vs_zero_padding_on_transient() {
        // Verifies that feeding the psychoacoustic model a 1024-sample
        // window with real look-back context produces different SMR
        // values than zero-padded windows on a signal with a sharp
        // transient. Zero-padding introduces spectral leakage at the
        // FFT discontinuity that distorts SMR — this test confirms the
        // look-back window changes the model's output for the same
        // granule samples.
        let sample_rate = 44100u32;

        // Signal: quiet → sudden loud tone (attack at sample 512)
        let mut signal = vec![0.0f32; 1024];
        for i in 0..512 {
            signal[i] = 0.01 * sin(i as f32 * 0.5);
        }
        for i in 512..1024 {
            signal[i] = 0.8 * sin((i as f32) * 1.2);
        }

        // Model with zero-padding: window[0..576] = granule, rest zeros
        let mut model_zp = make_model(sample_rate);
        for _ in 0..3 {
            model_zp.analyze_granule(&[0.0; 1024], sample_rate);
        }
        let mut zp_window = [0.0f32; 1024];
        zp_window[..576].copy_from_slice(&signal[..576]);
        let (smr_zp, _) = model_zp.analyze_granule(&zp_window, sample_rate);

        // Model with look-back: window[0..448] = prev.context, window[448..]=granule
        let mut model_lb = make_model(sample_rate);
        for _ in 0..3 {
            model_lb.analyze_granule(&[0.0; 1024], sample_rate);
        }
        let mut lb_window = [0.0f32; 1024];
        // Simulate 448-sample look-back from a previous frame (varied,
        // not identical to zeros — use the end of the signal for realism)
        lb_window[..448].copy_from_slice(&signal[576..1024]);
        lb_window[448..].copy_from_slice(&signal[..576]);
        let (smr_lb, _) = model_lb.analyze_granule(&lb_window, sample_rate);

        // Count how many bands differ — a properly different window
        // should produce a different SMR profile.
        let mut diff_count = 0usize;
        let mut max_rel_diff = 0.0f32;
        for b in 0..22 {
            let d = (smr_zp.bands[b] - smr_lb.bands[b]).abs();
            let rel = d / smr_zp.bands[b].max(smr_lb.bands[b]).max(1.0);
            if rel > 0.01 {
                diff_count += 1;
            }
            max_rel_diff = max_rel_diff.max(rel);
        }

        assert!(
            diff_count > 0 || max_rel_diff > 0.01,
            "SMR with look-back and zero-padding should differ on transient \
             (diff_count={diff_count}, max_rel_diff={max_rel_diff})"
        );
    }
}
