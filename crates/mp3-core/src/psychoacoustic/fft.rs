//! Windowed real FFT used only for masking-threshold estimation. See
//! `docs/mp3-encoder/07-phase4-psychoacoustic-model.md` §2.
//!
//! Unlike the filterbank/Huffman tables, this is generic DSP with no
//! MP3-specific opaque constants (beyond the closed-form Hann window) —
//! any correct radix-2 FFT implementation is acceptable, no table
//! provenance concern applies here.

/// Computes a windowed (Hann) magnitude spectrum of `samples` into
/// `out_magnitude`.
///
/// `samples.len()` must be a power of two (1024 for long-block analysis,
/// 256 for short-block analysis — see
/// `docs/mp3-encoder/07-phase4-psychoacoustic-model.md` §2).
/// `out_magnitude.len()` must be `samples.len() / 2 + 1`.
///
/// # Panics
///
/// Always, in this scaffold. Implement any correct radix-2 Cooley-Tukey
/// FFT (or equivalent), windowed with
/// `w[n] = 0.5 - 0.5 * cos(2*pi*n / (N-1))` before transform.
pub fn fft_magnitude(samples: &[f32], out_magnitude: &mut [f32]) {
    let _ = (samples, out_magnitude);
    todo!("M4: windowed real FFT -> magnitude spectrum — see 07-phase4 §2")
}

#[cfg(test)]
mod tests {
    // TODO(M4): known-input test — a single-bin-frequency sine wave
    // should produce magnitude energy concentrated in the expected FFT
    // bin. See docs/mp3-encoder/07-phase4-psychoacoustic-model.md §6.
}
