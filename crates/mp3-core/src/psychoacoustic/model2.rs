//! Psychoacoustic Model II proper: partitions, spreading function, SMR,
//! and the block-switching decision. See
//! `docs/mp3-encoder/07-phase4-psychoacoustic-model.md` §3-5.

extern crate alloc;
use alloc::vec;
use alloc::vec::Vec;

use crate::mdct::BlockType;

use super::fft::fft_windowed_complex;
use super::tables::{
    absolute_threshold_db, compute_partition_map, partition_bark_centers, partition_hz_centers,
    scalefactor_sample_rate_index, spreading_db, NMT_DB, SFB_LONG_BOUNDARIES, SFB_LONG_COUNTS,
    TMN_DB,
};

// `no_std`-safe transcendental functions -- call as free functions, never
// as `x.sqrt()`/`x.floor()`/`x.powi()` method syntax, which resolve to
// inherent `f32` methods that don't exist under `core`. See
// `docs/mp3-encoder/verification/manifest.yaml`'s build-wasm note.
use libm::{
    atan2f as atan2, ceilf as ceil, cosf as cos, expf as exp, floorf as floor, log10f as log10,
    sqrtf as sqrt,
};

/// ln(10) / 10 constant for dB ↔ linear conversion.
const LN10_OVER_10: f32 = core::f32::consts::LN_10 / 10.0;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// FFT size for long-block psychoacoustic analysis.
const FFT_SIZE_LONG: usize = 1024;
/// FFT size for short-block psychoacoustic analysis.
#[allow(dead_code)]
const FFT_SIZE_SHORT: usize = 256;
/// Number of real FFT output bins (N/2 + 1).
const FFT_BINS_LONG: usize = FFT_SIZE_LONG / 2 + 1;
#[allow(dead_code)]
const FFT_BINS_SHORT: usize = FFT_SIZE_SHORT / 2 + 1;

/// Perceptual entropy transient detection threshold (dB increase over
/// filtered PE). Tuning parameter — see chapter 07 §5.
const PE_ATTACK_THRESHOLD_DB: f32 = 10.0;

/// Low-pass filter coefficient for smoothed PE history.
const PE_SMOOTH_COEFF: f32 = 0.3;

/// Minimum PE baseline used as the transient-ratio denominator, avoiding
/// division by (near-)zero after a run of silence while still letting any
/// non-trivial PE spike produce a very large `pe_ratio`.
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

/// FFT magnitude + phase for one bin, kept across frames for the
/// unpredictability (tonality) measure.
#[allow(dead_code)]
#[derive(Clone, Copy, Default)]
struct FftBin {
    magnitude: f32,
    phase: f32,
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

    /// Smoothed (exponential moving average) perceptual entropy, used as
    /// the baseline that the current frame's PE is compared against to
    /// detect a transient. Reflects frames *up to and not including* the
    /// current one -- see `decide_block_type`.
    smoothed_pe: f32,

    /// Whether `smoothed_pe` has been seeded by at least one prior frame.
    /// The very first call has no baseline to compare against, so it
    /// seeds `smoothed_pe` and reports `Long` unconditionally rather than
    /// judging a "transient" relative to a baseline that doesn't exist.
    pe_initialized: bool,

    /// Current block type state machine output: `Long` in steady state,
    /// transitions to `Start`/`Short`/`Stop` around transients.
    block_type: BlockType,

    /// Short-block persistence counter (number of short-block granules
    /// remaining before transitioning back to Stop→Long).
    short_count: usize,
}

impl Default for PsychoacousticModel {
    fn default() -> Self {
        Self::new()
    }
}

impl PsychoacousticModel {
    /// Creates a fresh model with zeroed FFT history and `Long` block type.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            history: [[Complex { re: 0.0, im: 0.0 }; FFT_BINS_LONG]; 2],
            smoothed_pe: 0.0,
            pe_initialized: false,
            block_type: BlockType::Long,
            short_count: 0,
        }
    }

    /// Analyzes one granule's worth of raw PCM and produces the SMR
    /// values plus block-type decision for that granule.
    ///
    /// `pcm_window` should contain up to `1024` samples (the long-block
    /// analysis window, larger than one granule's 576 samples to give the
    /// masking-threshold FFT some look-ahead — see chapter 07 §2). Fewer
    /// samples are zero-padded internally; more are truncated.
    ///
    /// `sample_rate_hz` is the PCM sample rate in Hz (44100, 48000, etc.).
    pub fn analyze_granule(
        &mut self,
        pcm_window: &[f32],
        sample_rate_hz: u32,
    ) -> (ScalefactorBandSmr, BlockType) {
        let sfb_idx = scalefactor_sample_rate_index(sample_rate_hz);
        let sfb_count = SFB_LONG_COUNTS[sfb_idx];
        let sfb_bounds = &SFB_LONG_BOUNDARIES[sfb_idx];

        // Step 1: FFT analysis (1024-pt for long-block estimation)
        let (mag, re, im) = self.fft_analyze_long(pcm_window, sample_rate_hz);

        // Step 2: Tonality per FFT bin from history
        let tonality_bin = self.compute_tonality(&re, &im);

        // Step 3: Partition grouping
        let freq_per_bin = sample_rate_hz as f32 / FFT_SIZE_LONG as f32;
        let (partition_map, num_partitions) =
            compute_partition_map(FFT_BINS_LONG, freq_per_bin, 0.5);
        let part_centers_bark =
            partition_bark_centers(&partition_map, num_partitions, freq_per_bin);
        let part_centers_hz = partition_hz_centers(&partition_map, num_partitions, freq_per_bin);

        // Step 4: Per-partition energy and tonality
        let mut part_energy = vec![0.0f32; num_partitions];
        let mut part_tonality = vec![0.0f32; num_partitions];
        let mut part_tonality_count = vec![0usize; num_partitions];

        for i in 0..FFT_BINS_LONG {
            let p = partition_map[i];
            part_energy[p] += mag[i] * mag[i];
            part_tonality[p] += tonality_bin[i];
            part_tonality_count[p] += 1;
        }

        for p in 0..num_partitions {
            if part_tonality_count[p] > 0 {
                part_tonality[p] /= part_tonality_count[p] as f32;
            }
        }

        // Step 5: Spreading function convolution
        let mut part_threshold = vec![0.0f32; num_partitions];
        for i in 0..num_partitions {
            for j in 0..num_partitions {
                let dz = part_centers_bark[j] - part_centers_bark[i];
                let spread_db = spreading_db(dz);
                let spread_linear = exp(spread_db * LN10_OVER_10);
                part_threshold[i] += part_energy[j] * spread_linear;
            }
        }

        // Step 6: Apply tonality-based required SNR and compute per-partition
        // SMR. Tonal content (tonality → 1): needs higher SNR (TMN ≈ 29 dB);
        // noisy content (tonality → 0): needs lower SNR (NMT ≈ 6 dB).
        let mut part_smr = vec![1.0f32; num_partitions];
        for p in 0..num_partitions {
            let t = part_tonality[p].clamp(0.0, 1.0);
            let snr_db = t * TMN_DB + (1.0 - t) * NMT_DB;
            let snr_linear = exp(snr_db * LN10_OVER_10);
            part_threshold[p] /= snr_linear;

            // Apply absolute threshold of hearing floor, looked up at the
            // partition's actual center frequency (no lossy round-trip
            // through an approximate Bark-to-Hz inversion).
            let ath_linear = ath_from_db(absolute_threshold_db(part_centers_hz[p]));
            if part_threshold[p] < ath_linear {
                part_threshold[p] = ath_linear;
            }

            if part_energy[p] > 0.0 && part_threshold[p] > 0.0 {
                part_smr[p] = (part_energy[p] / part_threshold[p]).clamp(1.0, 1e6);
            }
        }

        // Step 7: Map per-partition SMR to scalefactor bands.
        //
        // Each SFB's SMR is the *maximum* SMR among its constituent
        // partitions, not `sum(energy)/sum(threshold)` or
        // `sum(energy)/min(threshold)` -- both of those combine an
        // extensive quantity (energy, which grows with how many
        // partitions/bins are summed) with one that doesn't scale the
        // same way, which systematically biases wide (typically
        // high-frequency) bands regardless of actual masking. Per-
        // partition SMR has no such bias (numerator and denominator come
        // from the same partition), and taking the max reflects the
        // standard's actual intent: the allowed distortion for a band is
        // set by whichever frequency within it is *least* masked, since a
        // quantizer that only satisfies the band's average would still
        // leave that point audibly distorted.
        let mut sfb_smr = [1.0f32; 22];
        for sfb in 0..sfb_count {
            let start_line = sfb_bounds[sfb];
            let end_line = sfb_bounds[sfb + 1];

            let start_freq = start_line as f32 * sample_rate_hz as f32 / (2.0 * 576.0);
            let end_freq = end_line as f32 * sample_rate_hz as f32 / (2.0 * 576.0);
            let start_bin = floor(start_freq / freq_per_bin) as usize;
            let end_bin = (ceil(end_freq / freq_per_bin) as usize).min(FFT_BINS_LONG - 1);

            if end_bin > start_bin {
                let mut visited = [false; 64]; // num_partitions ~48 for 44.1 kHz
                for &p in &partition_map[start_bin..end_bin] {
                    if p < visited.len() && !visited[p] {
                        visited[p] = true;
                        if part_smr[p] > sfb_smr[sfb] {
                            sfb_smr[sfb] = part_smr[p];
                        }
                    }
                }
            }
        }

        // Step 9: Block-type decision via perceptual entropy
        let pe = self.compute_perceptual_entropy(&mag, &partition_map, num_partitions);
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

    /// Run 1024-point FFT analysis on the PCM window, returning
    /// magnitude, real, and imaginary parts.
    fn fft_analyze_long(
        &self,
        pcm_window: &[f32],
        _sample_rate_hz: u32,
    ) -> (Vec<f32>, Vec<f32>, Vec<f32>) {
        let mut workspace = [0.0f32; FFT_SIZE_LONG];
        let copy_len = pcm_window.len().min(FFT_SIZE_LONG);
        workspace[..copy_len].copy_from_slice(&pcm_window[..copy_len]);

        let mut re = vec![0.0f32; FFT_BINS_LONG];
        let mut im = vec![0.0f32; FFT_BINS_LONG];
        fft_windowed_complex(&workspace, &mut re, &mut im);

        let mut mag = vec![0.0f32; FFT_BINS_LONG];
        for i in 0..FFT_BINS_LONG {
            mag[i] = sqrt(re[i] * re[i] + im[i] * im[i]);
        }

        (mag, re, im)
    }

    /// Compute tonality per FFT bin using the 2-frame history
    /// (unpredictability measure from Annex D §4.3).
    fn compute_tonality(&self, re: &[f32], im: &[f32]) -> Vec<f32> {
        let mut tonality = vec![0.0f32; FFT_BINS_LONG];

        for i in 0..FFT_BINS_LONG {
            let mag = sqrt(re[i] * re[i] + im[i] * im[i]);

            // Predict magnitude from previous 2 frames (linear extrapolation)
            let prev0_mag = self.history[0][i].magnitude();
            let prev1_mag = self.history[1][i].magnitude();
            let pred_mag = 2.0 * prev0_mag - prev1_mag;
            let pred_mag = pred_mag.max(0.0);

            // Predict phase from previous 2 frames
            let prev0_phase = self.history[0][i].phase();
            let prev1_phase = self.history[1][i].phase();
            let pred_phase = 2.0 * prev0_phase - prev1_phase;

            let phase = atan2(im[i], re[i]);

            // Normalized unpredictability: how much the actual differs from prediction
            let denom = pred_mag + mag + 1e-30;
            let mag_term = (pred_mag - mag) / denom;
            let cw = sqrt(
                mag_term * mag_term
                    + (2.0 * pred_mag * mag * (1.0 - cos(phase - pred_phase)) / (denom * denom)),
            );

            // Tonality = 1 - unpredictability, clamped to [0, 1]
            tonality[i] = (1.0_f32 - cw).clamp(0.0, 1.0);
        }

        tonality
    }

    /// Compute perceptual entropy from the FFT magnitude spectrum.
    /// PE = -sum over partitions of bandwidth * log10(energy/(thr*bw) + 1)
    fn compute_perceptual_entropy(
        &mut self,
        mag: &[f32],
        partition_map: &[usize],
        num_partitions: usize,
    ) -> f32 {
        let mut part_energy = vec![0.0f32; num_partitions];
        let mut part_count = vec![0usize; num_partitions];

        for i in 0..mag.len() {
            let p = partition_map[i];
            part_energy[p] += mag[i] * mag[i];
            part_count[p] += 1;
        }

        let mut pe = 0.0f32;
        for p in 0..num_partitions {
            if part_count[p] > 0 && part_energy[p] > 0.0 {
                let bw = part_count[p] as f32;
                let energy_per_bin = part_energy[p] / bw;
                // Simple threshold estimate: just use a small floor
                let thr = energy_per_bin * 1e-6 + 1e-30;
                pe += bw * log10(energy_per_bin / thr + 1.0);
            }
        }

        pe
    }

    /// Block-type state machine based on perceptual entropy transients.
    ///
    /// The ratio compares the current frame's PE against the smoothed
    /// baseline accumulated *before* this frame — comparing it against a
    /// baseline already blended with the current frame's own energy would
    /// cap `pe_ratio` at `1 / PE_SMOOTH_COEFF` regardless of how large the
    /// transient actually is (and pins it at exactly 1.0 the first time a
    /// transient follows silence, since the cold-start baseline collapses
    /// to the current value), making the threshold unreachable. See the
    /// M4 review notes for the derivation.
    fn decide_block_type(&mut self, pe: f32) {
        if !self.pe_initialized {
            // First frame ever: there is no baseline yet to judge a
            // transient against. Seed it and report `Long` (the default)
            // rather than comparing `pe` to a baseline that doesn't exist.
            self.smoothed_pe = pe;
            self.pe_initialized = true;
            return;
        }

        let baseline = self.smoothed_pe.max(PE_BASELINE_FLOOR);
        let pe_ratio = pe / baseline;

        // Update the smoothed baseline for the *next* call only after
        // computing this frame's ratio against the pre-update value.
        self.smoothed_pe = PE_SMOOTH_COEFF * pe + (1.0 - PE_SMOOTH_COEFF) * self.smoothed_pe;

        match self.block_type {
            BlockType::Long => {
                if pe_ratio > PE_ATTACK_THRESHOLD_DB {
                    self.block_type = BlockType::Start;
                    self.short_count = 3; // 3 short granules after Start
                }
            }
            BlockType::Start => {
                // `short_count` (3) was set on the Long->Start transition
                // above and belongs to the 3 granules about to be
                // reported as `Short` below -- this transition doesn't
                // itself report a `Short` granule, so it must not
                // consume one of the 3, or only 2 `Short` granules are
                // ever produced instead of the Long -> Start -> Short×3
                // -> Stop -> Long sequence chapter 07 §5 specifies.
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

/// Convert ATH dB SPL to linear power (arbitrary units, consistent with
/// the internal energy scale).
fn ath_from_db(db: f32) -> f32 {
    if db < -100.0 {
        return 1e-20;
    }
    exp(db * LN10_OVER_10)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use core::f32::consts::PI;
    use libm::sinf as sin;

    /// Generate a test tone at a specific frequency, sample rate, and duration.
    fn make_tone(freq: f32, sample_rate: u32, num_samples: usize) -> Vec<f32> {
        let mut samples = Vec::with_capacity(num_samples);
        for i in 0..num_samples {
            let t = i as f32 / sample_rate as f32;
            samples.push(sin(2.0 * PI * freq * t));
        }
        samples
    }

    /// Generate white noise samples.
    fn make_noise(num_samples: usize) -> Vec<f32> {
        let mut samples = Vec::with_capacity(num_samples);
        // Simple pseudo-random using a linear congruential generator
        let mut seed: u32 = 12345;
        for _ in 0..num_samples {
            seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
            samples.push(((seed >> 16) as f32 / 32768.0 - 0.5) * 2.0);
        }
        samples
    }

    // --- SMR shape tests ---

    /// SMR standard deviation across the 22 long-block bands (44.1 kHz),
    /// a measure of how "peaky" vs. "flat" the SMR profile is.
    fn smr_std_dev(bands: &[f32; 22]) -> f32 {
        let mean: f32 = bands.iter().sum::<f32>() / 22.0;
        let variance: f32 = bands.iter().map(|&s| (s - mean) * (s - mean)).sum::<f32>() / 22.0;
        sqrt(variance)
    }

    #[test]
    fn smr_tone_produces_high_smr_at_own_frequency() {
        let mut model = PsychoacousticModel::new();
        let tone = make_tone(1000.0, 44100, 1024);
        let (smr, _bt) = model.analyze_granule(&tone, 44100);

        // A pure 1 kHz tone should produce high SMR somewhere in the mid bands.
        let has_high_smr = smr.bands.iter().any(|&s| s > 2.0);
        assert!(has_high_smr, "tone should produce SMR > 1.0 in some bands");

        // All SMR values should be >= 1.0
        for (i, &s) in smr.bands.iter().enumerate().take(22) {
            assert!(s >= 1.0, "SMR band {i} is {s}, should be >= 1.0");
        }
    }

    #[test]
    fn smr_tone_shows_spreading_shape_around_peak() {
        // DoD (chapter 07 §6): "a pure tone produces a high SMR at its own
        // frequency and progressively lower SMR moving away from it
        // (spreading function visible in the output)".
        let mut model = PsychoacousticModel::new();
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

        // Bands well away (>=6 SFB) from the tone's own band should sit
        // markedly below the peak -- if the spreading function had no
        // effect, SMR would be flat across all bands instead.
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
        // DoD (chapter 07 §6): tone SMR is peaky (spreading-shaped), white
        // noise SMR is "comparatively flat" -- test the comparison
        // directly rather than an arbitrary absolute bound on noise alone.
        let mut tone_model = PsychoacousticModel::new();
        let tone = make_tone(1000.0, 44100, 1024);
        let (tone_smr, _) = tone_model.analyze_granule(&tone, 44100);

        let mut noise_model = PsychoacousticModel::new();
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

    // --- Block-type decision tests ---

    #[test]
    fn block_type_defaults_to_long() {
        let model = PsychoacousticModel::new();
        assert_eq!(model.block_type, BlockType::Long);
    }

    #[test]
    fn silent_signal_stays_long() {
        let mut model = PsychoacousticModel::new();

        // Feed several frames of silence
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
        // DoD (chapter 07 §6): a synthetic transient (full-scale noise
        // burst after silence) must be flagged, driving the model through
        // exactly the Long -> Start -> Short×3 -> Stop -> Long sequence
        // chapter 07 §5 specifies.
        let mut model = PsychoacousticModel::new();

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
        let mut model = PsychoacousticModel::new();
        let tone = make_tone(440.0, 44100, 1024);

        // The very first call seeds the PE baseline (no prior frame to
        // compare against); every call after that must not spuriously
        // trigger short blocks for a genuinely stationary signal.
        model.analyze_granule(&tone, 44100);

        for _ in 0..10 {
            let (_, bt) = model.analyze_granule(&tone, 44100);
            assert_eq!(
                bt,
                BlockType::Long,
                "stationary tone should not trigger short blocks"
            );
        }
    }

    // --- Edge cases ---

    #[test]
    fn silence_produces_finite_smr() {
        let mut model = PsychoacousticModel::new();
        let silence = vec![0.0f32; 1024];
        let (smr, _bt) = model.analyze_granule(&silence, 44100);

        for (i, &s) in smr.bands.iter().enumerate().take(22) {
            assert!(s.is_finite(), "SMR band {i} is {s} (should be finite)");
            assert!(s >= 1.0, "SMR band {i} is {s} (should be >= 1.0)");
        }
    }

    #[test]
    fn fullscale_produces_finite_smr() {
        let mut model = PsychoacousticModel::new();
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
        let mut model = PsychoacousticModel::new();
        let tone = make_tone(500.0, 44100, 1024);

        // Run two frames and check both produce valid results
        let (smr1, _) = model.analyze_granule(&tone, 44100);
        let (smr2, _) = model.analyze_granule(&tone, 44100);

        for i in 0..22 {
            assert!(smr1.bands[i].is_finite());
            assert!(smr2.bands[i].is_finite());
            assert!(smr1.bands[i] >= 1.0);
            assert!(smr2.bands[i] >= 1.0);
        }

        // SMR should be non-zero for tone
        let mean1: f32 = smr1.bands.iter().take(22).sum::<f32>() / 22.0;
        assert!(mean1 > 0.0, "SMR should be positive for tone signal");
    }
}
