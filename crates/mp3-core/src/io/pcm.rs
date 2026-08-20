//! `PcmBuffer`: deinterleaved, normalized PCM for exactly one MPEG frame.
//!
//! See `docs/mp3-encoder/04-phase1-pcm-io-and-framing.md` §4-5.

use crate::error::EncodeError;
use crate::types::{ChannelMode, MpegVersion, MAX_SAMPLES_PER_FRAME};

/// One frame's worth of PCM, deinterleaved and normalized to `f32` in
/// `[-1.0, 1.0]` — `version.samples_per_frame()` samples per channel
/// (1152 for MPEG-1, 576 for MPEG-2 LSF; see
/// `docs/mp3-encoder/04-phase1-pcm-io-and-framing.md` §1). Backing
/// arrays are sized to the max; only the valid prefix is exposed.
///
/// Normalization: `i16` input is divided by `32768.0` (2^15), not
/// `32767.0` — this keeps the scale symmetric around zero. `f32` input is
/// assumed already normalized and is validated, not rescaled. Keep this
/// choice consistent with how the psychoacoustic model's absolute
/// threshold of hearing is calibrated (chapter 07) — see
/// `docs/mp3-encoder/04-phase1-pcm-io-and-framing.md` §4.
pub struct PcmBuffer {
    channels: [[f32; MAX_SAMPLES_PER_FRAME]; 2], // index 1 unused when mono
    channel_count: usize,
    samples_per_channel: usize, // == version.samples_per_frame()
}

impl PcmBuffer {
    /// Builds a `PcmBuffer` from interleaved `i16` PCM
    /// (`[L0, R0, L1, R1, ...]` for stereo, `[S0, S1, ...]` for mono).
    ///
    /// `samples.len()` must equal
    /// `mode.channel_count() * version.samples_per_frame()` exactly —
    /// this constructor handles a single, already-framed chunk; the
    /// frame-boundary-spanning ring buffer needed for continuous
    /// streaming input lives on `Encoder`, not here (see
    /// `docs/mp3-encoder/04-phase1-pcm-io-and-framing.md` §5).
    ///
    /// # Errors
    ///
    /// Returns [`EncodeError::BufferLengthMismatch`] if the length does
    /// not match.
    pub fn from_i16_interleaved(
        samples: &[i16],
        mode: ChannelMode,
        version: MpegVersion,
    ) -> Result<Self, EncodeError> {
        let _ = (samples, mode, version);
        todo!("M1: deinterleave, divide by 32768.0 — see 04-phase1 §4")
    }

    /// Builds a `PcmBuffer` from interleaved `f32` PCM already normalized
    /// to `[-1.0, 1.0]`. See [`Self::from_i16_interleaved`] for the
    /// length contract.
    ///
    /// # Errors
    ///
    /// Returns [`EncodeError::BufferLengthMismatch`] if the length does
    /// not match.
    pub fn from_f32_interleaved(
        samples: &[f32],
        mode: ChannelMode,
        version: MpegVersion,
    ) -> Result<Self, EncodeError> {
        let _ = (samples, mode, version);
        todo!("M1: deinterleave — see 04-phase1 §4")
    }

    /// The valid samples for one channel (`0` = left/mono, `1` = right):
    /// a slice of length `samples_per_channel()`, so MPEG-2 LSF frames
    /// can never silently read the unused tail of the backing array.
    ///
    /// # Panics
    ///
    /// Panics if `channel >= self.channel_count()` — this is a
    /// caller-programming-error, not a data error, so it is not a
    /// `Result` (see `docs/mp3-encoder/01-architecture.md` §4 on
    /// `debug_assert!` vs. `Result`).
    #[must_use]
    pub fn channel(&self, channel: usize) -> &[f32] {
        assert!(channel < self.channel_count, "channel index out of range");
        &self.channels[channel][..self.samples_per_channel]
    }

    /// Number of channels in this buffer (1 or 2).
    #[must_use]
    pub const fn channel_count(&self) -> usize {
        self.channel_count
    }

    /// Valid samples per channel: 1152 (MPEG-1) or 576 (MPEG-2 LSF).
    #[must_use]
    pub const fn samples_per_channel(&self) -> usize {
        self.samples_per_channel
    }
}

#[cfg(test)]
mod tests {
    // TODO(M1): round-trip a synthetic [L0,R0,L1,R1,...] fixture through
    // both from_i16_interleaved and from_f32_interleaved and assert the
    // deinterleaved channel data matches — for BOTH an MPEG-1 (1152) and
    // an LSF (576) case. See
    // docs/mp3-encoder/04-phase1-pcm-io-and-framing.md §6.
}
