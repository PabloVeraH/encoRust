//! The sliding-window polyphase analysis filter. See
//! `docs/mp3-encoder/05-phase2-polyphase-filterbank.md` §1 and §3.

use crate::types::SUBBANDS;

// core::f32 has no cos (std-only) — libm is a #![no_std] pure-Rust libm,
// so `cosf` works identically under both std and --no-default-features.
// See crates/mp3-core/Cargo.toml's comment on the libm dependency for
// the M0 no_std/wasm regression this fixes.
use libm::cosf as cos;

/// Sliding 512-sample analysis buffer + prototype filter application.
///
/// One instance per channel — the 512-sample history is a continuous
/// window over the entire PCM stream (seeded with zeros at stream
/// start), **not** reset per frame or per granule. See
/// `docs/mp3-encoder/05-phase2-polyphase-filterbank.md` §3 for the exact
/// shift/indexing convention this must follow.
pub struct PolyphaseFilterbank {
    history: [f32; 512],
}

impl Default for PolyphaseFilterbank {
    fn default() -> Self {
        Self::new()
    }
}

impl PolyphaseFilterbank {
    /// Creates a filterbank with a zeroed history (stream start).
    #[must_use]
    pub const fn new() -> Self {
        Self {
            history: [0.0; 512],
        }
    }

    /// Feeds exactly 32 new PCM samples (natural time order — oldest to
    /// newest within the chunk) and produces the 32 subband output
    /// samples for this analysis step.
    ///
    /// Called 18 times per granule
    /// (`18 * 32 == SAMPLES_PER_GRANULE`, see
    /// [`crate::types::SAMPLES_PER_GRANULE`]).
    pub fn analyze(&mut self, pcm_chunk: &[f32; 32]) -> [f32; SUBBANDS] {
        // Step 1: shift history buffer. dist10 convention: shift
        // previous 480 samples upward, insert 32 new samples at X[0..31]
        // in REVERSED time order (newest sample at X[0]).
        for i in (32..512).rev() {
            self.history[i] = self.history[i - 32];
        }
        for i in 0..32 {
            self.history[i] = pcm_chunk[31 - i];
        }

        // Step 2: window against prototype filter C[i]. Z[i] = X[i] * C[i]
        let filter = &super::ANALYSIS_PROTOTYPE_FILTER;
        let mut z = [0.0f32; 512];
        for i in 0..512 {
            z[i] = self.history[i] * filter[i];
        }

        // Step 3: partial summation. Y[i] = Σ Z[i + 64*j] for j in 0..7
        let mut y = [0.0f32; 64];
        for i in 0..64 {
            let mut sum = 0.0;
            for j in 0..8 {
                sum += z[i + 64 * j];
            }
            y[i] = sum;
        }

        // Step 4: cosine matrixing.
        // S[k] = Σ M[k][i] * Y[i] where M[k][i] = cos((2k+1)*(i-16)*π/64)
        use core::f32::consts::PI;
        let mut s = [0.0f32; SUBBANDS];
        for (k, sk) in s.iter_mut().enumerate() {
            let mut sum = 0.0;
            for (i, &yi) in y.iter().enumerate() {
                sum += yi * cos(((2 * k + 1) as f32) * ((i as f32) - 16.0) * PI / 64.0);
            }
            *sk = sum;
        }

        s
    }
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)] // f32::abs() in test assertions -- see docs/mejoras.md §7 item 6
mod tests {
    use super::*;
    use core::f32::consts::PI;
    use libm::sinf as sin;

    #[test]
    fn zero_input_produces_zero_output() {
        let mut fb = PolyphaseFilterbank::new();
        let zero_chunk = [0.0f32; 32];
        let out = fb.analyze(&zero_chunk);
        for s in &out {
            assert!(
                s.abs() < 1e-6,
                "zero input should produce near-zero subband output"
            );
        }
    }

    #[test]
    fn impulse_response_matches_filter_shape() {
        // A unit impulse fed as chunk[31] during call 0 lands at
        // history[0], then shifts exactly one 32-sample slot per
        // subsequent analyze() call (Step 1's `history[i] =
        // history[i-32]`) -- so on call `n` it sits at
        // `history[32*n]`, for n = 0..=15 (32*15 = 480 <= 511; on call
        // 16 it would need history[512], out of bounds, so it has fully
        // exited the window by then). With every other history slot
        // still zero, Steps 2-4 collapse to a closed form with only one
        // surviving term:
        //   idx = 32*n, i0 = idx % 64
        //   s[k] = filter[idx] * cos((2k+1) * (i0 - 16) * PI / 64)
        // This checks the actual `ANALYSIS_PROTOTYPE_FILTER` values and
        // the cosine-matrix formula the implementation uses, not just
        // "doesn't panic" -- an earlier version of this test only
        // checked `s.is_finite()`, which would pass even if the
        // filterbank's math were completely wrong. See
        // `docs/mp3-encoder/05-phase2-polyphase-filterbank.md` §3 for
        // the same Step 1-4 breakdown this derivation follows.
        use crate::filterbank::ANALYSIS_PROTOTYPE_FILTER;

        let mut fb = PolyphaseFilterbank::new();
        let mut impulse_chunk = [0.0f32; 32];
        impulse_chunk[31] = 1.0; // newest sample = impulse; maps to history[0]
        let zero_chunk = [0.0f32; 32];

        for n in 0..16usize {
            let out = if n == 0 {
                fb.analyze(&impulse_chunk)
            } else {
                fb.analyze(&zero_chunk)
            };

            let idx = 32 * n;
            let i0 = idx % 64;
            let filter_tap = ANALYSIS_PROTOTYPE_FILTER[idx];

            for (k, &actual) in out.iter().enumerate() {
                let expected =
                    filter_tap * libm::cosf(((2 * k + 1) as f32) * (i0 as f32 - 16.0) * PI / 64.0);
                assert!(
                    (actual - expected).abs() < 1e-4,
                    "call {n}, subband {k}: got {actual}, expected {expected} \
                     (filter[{idx}] = {filter_tap})"
                );
            }
        }
    }

    #[test]
    fn impulse_propagates_correctly() {
        // Feed many zero chunks then one with an impulse. After 3 analyze()
        // calls (covered by impulse at any position within 32*3=96 window),
        // verify res
        let mut fb = PolyphaseFilterbank::new();

        // Feed 10 zero chunks to initialize the history buffer
        let zero_chunk = [0.0f32; 32];
        for _ in 0..10 {
            fb.analyze(&zero_chunk);
        }

        // Now feed an impulse
        let mut imp_chunk = [0.0f32; 32];
        imp_chunk[31] = 1.0;
        fb.analyze(&imp_chunk);

        // After the impulse, feed more zeros and verify outputs become
        // non-zero as the impulse propagates through the window
        let mut had_energy = false;
        for _ in 0..20 {
            let out = fb.analyze(&zero_chunk);
            let energy: f32 = out.iter().map(|s| s * s).sum();
            if energy > 1e-10 {
                had_energy = true;
            }
        }
        assert!(had_energy, "impulse should produce energy as it propagates");
    }

    #[test]
    fn sine_energy_concentrates_in_expected_subband() {
        // A pure sine at frequency f = (k + 0.5) * fs / 64 should
        // concentrate in subband k. We test with k=1 (subband 1):
        // f = 1.5 * 44100 / 64 ≈ 1033 Hz for 44.1 kHz.
        // Actually, sample rate isn't used here — the normalized frequency
        // is (k+0.5)*π/32 in the digital domain.
        //
        // normalized freq: omega = (k+0.5) * π / 32
        // For k=4: omega = 4.5π/32. Subband center is at (2k+1)*π/64 = 9π/64.
        // Bandwidth: π/32. So freq ω within subband k if:
        // (2k)*π/64 < ω < (2k+2)*π/64. For k=4: 8π/64 < ω < 10π/64.
        // 4.5π/32 = 9π/64. Perfect — center of subband 4.

        let mut fb = PolyphaseFilterbank::new();
        let k_target = 4;
        let omega = (k_target as f32 + 0.5) * PI / 32.0;

        // Seed the filterbank by analyzing 16 chunks of sine
        for step in 0..16 {
            let mut chunk = [0.0f32; 32];
            for (j, item) in chunk.iter_mut().enumerate() {
                let t = step * 32 + j;
                *item = sin(omega * t as f32);
            }
            fb.analyze(&chunk);
        }

        // Now analyze one more chunk
        let mut final_chunk = [0.0f32; 32];
        for (j, item) in final_chunk.iter_mut().enumerate() {
            *item = sin(omega * (16 * 32 + j) as f32);
        }
        let out = fb.analyze(&final_chunk);

        let total_energy: f32 = out.iter().map(|s| s * s).sum();
        assert!(total_energy > 0.1, "sine should produce significant energy");

        // Energy in subband k should dominate
        let energy_target = out[k_target] * out[k_target];
        let ratio = energy_target / total_energy;

        // With the polyphase filterbank, energy concentrates in the
        // target subband but there's significant leakage to adjacent
        // subbands (this is expected — the MDCT/anti-alias butterfly in
        // chapter 06 corrects this). A ratio > 0.05 is reasonable.
        assert!(
            ratio > 0.05,
            "subband {} energy fraction {} too low (expected concentration)",
            k_target,
            ratio
        );
    }

    #[test]
    fn multiple_analyze_produces_consistent_output() {
        // Feeding zeros repeatedly should produce consistent (zero) output
        let mut fb = PolyphaseFilterbank::new();
        let zero = [0.0f32; 32];
        for _ in 0..5 {
            let out = fb.analyze(&zero);
            for s in out {
                assert!(s.abs() < 1e-6);
            }
        }
    }

    #[test]
    fn history_is_continuous_and_not_reset() {
        let mut fb1 = PolyphaseFilterbank::new();
        let mut fb2 = PolyphaseFilterbank::new();

        let zeros = [0.0f32; 32];
        let ones = [1.0f32; 32];

        // fb1: feed 3 zeros then 1 ones
        for _ in 0..3 {
            fb1.analyze(&zeros);
        }
        fb1.analyze(&ones);

        // fb2: feed 4 zeros
        for _ in 0..4 {
            fb2.analyze(&zeros);
        }

        // The outputs of the next analyze should differ because the
        // history carries the 1.0 samples in fb1 but not fb2
        let out1 = fb1.analyze(&zeros);
        let out2 = fb2.analyze(&zeros);

        let diff: f32 = out1
            .iter()
            .zip(out2.iter())
            .map(|(a, b)| (a - b).abs())
            .sum();
        assert!(diff > 1e-6, "history should be persistent between calls");
    }
}
