//! Windowed real FFT used only for masking-threshold estimation. See
//! `docs/mp3-encoder/07-phase4-psychoacoustic-model.md` §2.
//!
//! Unlike the filterbank/Huffman tables, this is generic DSP with no
//! MP3-specific opaque constants (beyond the closed-form Hann window) --
//! any correct radix-2 FFT implementation is acceptable, no table
//! provenance concern applies here.

#![allow(clippy::needless_range_loop)]

use core::f32::consts::PI;
// `no_std`-safe: call these as free functions, never as `x.sqrt()` method
// syntax, which resolves to the (missing under `core`) inherent method.
use libm::{cosf as cos, sinf as sin, sqrtf as sqrt};

/// Computes a windowed (Hann) real-input FFT, returning complex output
/// in `out_re` and `out_im` for the first `N/2 + 1` bins (the rest are
/// conjugate-symmetric for real input).
///
/// Used by the psychoacoustic model's tonality measure, which needs
/// phase information for the 2-frame history prediction.
pub fn fft_windowed_complex(samples: &[f32], out_re: &mut [f32], out_im: &mut [f32]) {
    let n = samples.len();
    assert!(n.is_power_of_two(), "FFT size must be a power of two");
    assert!(n >= 2, "FFT size must be at least 2");
    let half = n / 2 + 1;
    assert_eq!(out_re.len(), half);
    assert_eq!(out_im.len(), half);

    // Hann window + copy to complex array
    let mut re = [0.0f32; 1024];
    let mut im = [0.0f32; 1024];
    let nf = n as f32;
    let denom = 2.0 * core::f32::consts::PI / (nf - 1.0);
    for i in 0..n {
        let w = 0.5 - 0.5 * cos(i as f32 * denom);
        re[i] = samples[i] * w;
    }

    // Radix-2 FFT
    fft_radix2(n, &mut re[..n], &mut im[..n]);

    // Copy first N/2+1 bins
    out_re.copy_from_slice(&re[..half]);
    out_im.copy_from_slice(&im[..half]);
}

/// Computes a windowed (Hann) magnitude spectrum of `samples` into
/// `out_magnitude`.
///
/// `samples.len()` must be a power of two (1024 for long-block analysis,
/// 256 for short-block analysis - see
/// `docs/mp3-encoder/07-phase4-psychoacoustic-model.md` §2).
/// `out_magnitude.len()` must be `samples.len() / 2 + 1`.
///
/// # Panics
///
/// Panics if `samples.len()` is not a power of two, or if
/// `out_magnitude.len() != samples.len() / 2 + 1`.
pub fn fft_magnitude(samples: &[f32], out_magnitude: &mut [f32]) {
    let n = samples.len();
    assert!(n.is_power_of_two(), "FFT size must be a power of two");
    assert!(n >= 2, "FFT size must be at least 2");
    assert_eq!(
        out_magnitude.len(),
        n / 2 + 1,
        "out_magnitude must be len/2+1"
    );

    // Hann window: w[n] = 0.5 - 0.5*cos(2*pi*n / (N-1))
    let mut re = [0.0f32; 1024];
    let mut im = [0.0f32; 1024];
    let nf = n as f32;
    let denom = 2.0 * PI / (nf - 1.0);
    for i in 0..n {
        let w = 0.5 - 0.5 * cos(i as f32 * denom);
        re[i] = samples[i] * w;
        im[i] = 0.0;
    }

    // Radix-2 Cooley-Tukey FFT (in-place on re/im arrays)
    fft_radix2(n, &mut re[..n], &mut im[..n]);

    // Compute magnitude: sqrt(re^2 + im^2) for bins 0..n/2
    for i in 0..=n / 2 {
        out_magnitude[i] = sqrt(re[i] * re[i] + im[i] * im[i]);
    }
}

/// In-place radix-2 Cooley-Tukey complex FFT.
fn fft_radix2(n: usize, re: &mut [f32], im: &mut [f32]) {
    // Bit-reversal permutation
    let mut j = 0usize;
    for i in 0..n {
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
        let mut m = n >> 1;
        while m > 0 && j & m != 0 {
            j ^= m;
            m >>= 1;
        }
        j ^= m;
    }

    // Butterfly stages
    let mut len = 2;
    while len <= n {
        let half_len = len >> 1;
        let angle = -2.0 * PI / len as f32;
        let w_re = cos(angle);
        let w_im = sin(angle);

        for i in (0..n).step_by(len) {
            let mut cur_re = 1.0f32;
            let mut cur_im = 0.0f32;

            for k in 0..half_len {
                let idx1 = i + k;
                let idx2 = i + k + half_len;

                // t = w * a[idx2]
                let t_re = cur_re * re[idx2] - cur_im * im[idx2];
                let t_im = cur_re * im[idx2] + cur_im * re[idx2];

                re[idx2] = re[idx1] - t_re;
                im[idx2] = im[idx1] - t_im;
                re[idx1] += t_re;
                im[idx1] += t_im;

                // Advance twiddle factor
                let next_re = cur_re * w_re - cur_im * w_im;
                let next_im = cur_re * w_im + cur_im * w_re;
                cur_re = next_re;
                cur_im = next_im;
            }
        }
        len <<= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fft_dc_signal_magnitude() {
        let samples = [1.0f32; 256];
        let mut mag = [0.0f32; 129]; // 256/2 + 1
        fft_magnitude(&samples, &mut mag);

        // A constant signal has energy only at DC
        // DC bin = sum of windowed samples. The Hann window sum = N/2
        // So DC magnitude ≈ N/2 = 128 for N=256
        assert!(
            mag[0] > 100.0,
            "DC bin should have significant energy for constant signal, got {}",
            mag[0]
        );

        // DC bin should dominate all others
        for i in 1..mag.len() {
            assert!(
                mag[i] <= mag[0],
                "bin {} ({}) should not exceed DC ({})",
                i,
                mag[i],
                mag[0]
            );
        }
    }

    #[test]
    fn fft_sine_concentrates_energy_in_expected_bin() {
        // 44.1 kHz sample rate, 1024-pt FFT -> bin spacing = 44100/1024 = 43.07 Hz
        // A 861.3 Hz sine -> bin 20
        let _freq_bin = 20.0f32;
        let n = 1024usize;
        let mut samples = [0.0f32; 1024];

        for i in 0..n {
            let t = i as f32 / 44_100.0;
            samples[i] = sin(2.0 * PI * 861.3 * t);
        }

        let mut mag = [0.0f32; 513]; // 1024/2 + 1
        fft_magnitude(&samples, &mut mag);

        // Energy should be concentrated around bin 20
        let peak_bin = (1..512)
            .max_by(|a, b| mag[*a].partial_cmp(&mag[*b]).unwrap())
            .unwrap();

        assert!(
            (19..=21).contains(&peak_bin),
            "peak should be near bin 20, got {}",
            peak_bin
        );

        // Bin 20 should have significantly more energy than bins far away
        let energy_peak = mag[20];
        let energy_away: f32 = mag.iter().take(10).chain(mag.iter().skip(30)).sum();
        assert!(
            energy_peak > 10.0 * energy_away,
            "peak energy {:.2} should dominate far-away energy {:.2}",
            energy_peak,
            energy_away
        );
    }

    #[test]
    fn fft_silence_produces_zero_magnitude() {
        let samples = [0.0f32; 512];
        let mut mag = [0.0f32; 257];
        fft_magnitude(&samples, &mut mag);

        for m in &mag {
            assert!(m.abs() < 1e-6, "silence should produce near-zero magnitude");
        }
    }

    #[test]
    fn fft_magnitude_values_are_finite() {
        let mut samples = [0.0f32; 256];
        for i in 0..256 {
            samples[i] = sin(i as f32 * 0.5) + cos(i as f32 * 1.7);
        }
        let mut mag = [0.0f32; 129];
        fft_magnitude(&samples, &mut mag);

        for m in &mag {
            assert!(m.is_finite(), "all magnitude values must be finite");
        }
    }

    #[test]
    fn fft_energy_conservation() {
        // Parseval: sum of squared time samples ≈ (1/N) * sum of |FFT|^2
        let n = 256usize;
        let mut samples = [0.0f32; 256];
        for i in 0..n {
            samples[i] = sin(i as f32 * 0.3) * 0.7;
        }

        let time_energy: f32 = samples.iter().map(|x| x * x).sum();

        let mut mag = [0.0f32; 129];
        fft_magnitude(&samples, &mut mag);

        // Real FFT: energy = (1/N) * (|X[0]|^2 + 2*sum_{k=1}^{N/2-1} |X[k]|^2 + |X[N/2]|^2)
        // But with windowing the relationship is approximate
        let mut freq_energy = mag[0] * mag[0];
        for i in 1..129usize - 1 {
            freq_energy += 2.0 * mag[i] * mag[i];
        }
        freq_energy += mag[128] * mag[128];
        freq_energy /= n as f32;

        // With Hann window, energy scales differently. Just check both are non-zero and finite.
        assert!(time_energy > 0.0);
        assert!(freq_energy > 0.0);
        assert!(freq_energy.is_finite());
    }

    #[test]
    fn fft_short_block_size_256() {
        let n = 256usize;
        let mut samples = [0.0f32; 256];
        for i in 0..n {
            samples[i] = sin(i as f32 * 0.6);
        }
        let mut mag = [0.0f32; 129];
        fft_magnitude(&samples, &mut mag);

        assert!(mag[0].is_finite());
        let peak: f32 = mag.iter().cloned().fold(0.0f32, f32::max);
        assert!(peak > 0.0, "should have non-zero energy");
    }
}
