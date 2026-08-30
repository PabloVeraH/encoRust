//! `PcmBuffer`: deinterleaved, normalized PCM for exactly one MPEG frame.
//!
//! See `docs/mp3-encoder/04-phase1-pcm-io-and-framing.md` §4-5.

use crate::error::EncodeError;
use crate::types::{ChannelMode, MpegVersion, MAX_CHANNELS, MAX_SAMPLES_PER_FRAME};

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
#[derive(Debug)]
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
        let n_channels = mode.channel_count();
        let expected = n_channels * version.samples_per_frame();
        if samples.len() != expected {
            return Err(EncodeError::BufferLengthMismatch {
                expected,
                got: samples.len(),
            });
        }

        let mut channels = [[0.0f32; MAX_SAMPLES_PER_FRAME]; MAX_CHANNELS];
        let spc = version.samples_per_frame();

        for ch in 0..n_channels {
            for i in 0..spc {
                channels[ch][i] = f32::from(samples[i * n_channels + ch]) / 32768.0;
            }
        }

        Ok(Self {
            channels,
            channel_count: n_channels,
            samples_per_channel: spc,
        })
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
        let n_channels = mode.channel_count();
        let expected = n_channels * version.samples_per_frame();
        if samples.len() != expected {
            return Err(EncodeError::BufferLengthMismatch {
                expected,
                got: samples.len(),
            });
        }

        let mut channels = [[0.0f32; MAX_SAMPLES_PER_FRAME]; MAX_CHANNELS];
        let spc = version.samples_per_frame();

        for ch in 0..n_channels {
            for i in 0..spc {
                channels[ch][i] = samples[i * n_channels + ch];
            }
        }

        Ok(Self {
            channels,
            channel_count: n_channels,
            samples_per_channel: spc,
        })
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
#[allow(clippy::disallowed_methods)] // f32::abs() in test assertions -- see docs/investigation-log.md §7 item 6
mod tests {
    use super::*;
    // Pre-existing no_std-test gap — see the identical note in
    // bitstream/scalefactor_encode.rs's test module.
    use crate::types::{ChannelMode, MpegVersion};
    use alloc::vec::Vec;

    #[test]
    fn i16_deinterleave_stereo_mpeg1() {
        let spf = MpegVersion::Mpeg1.samples_per_frame(); // 1152
        let total = 2 * spf;
        let mut interleaved = Vec::with_capacity(total);
        for i in 0..spf {
            interleaved.push(i as i16); // left
            interleaved.push((i + 1000) as i16); // right
        }
        let buf =
            PcmBuffer::from_i16_interleaved(&interleaved, ChannelMode::Stereo, MpegVersion::Mpeg1)
                .expect("valid MPEG-1 stereo buffer");
        assert_eq!(buf.samples_per_channel(), spf);
        assert_eq!(buf.channel_count(), 2);
        let left = buf.channel(0);
        let right = buf.channel(1);
        assert_eq!(left.len(), spf);
        assert_eq!(right.len(), spf);
        // Check deinterleaving correctness
        assert!((left[0] - 0.0).abs() < 1e-6);
        assert!((left[1] - (1.0 / 32768.0)).abs() < 1e-6);
        assert!((right[0] - (1000.0 / 32768.0)).abs() < 1e-6);
        assert!((right[1] - (1001.0 / 32768.0)).abs() < 1e-6);
    }

    #[test]
    fn i16_deinterleave_mono_lsf() {
        let spf = MpegVersion::Mpeg2Lsf.samples_per_frame(); // 576
        let mut interleaved = Vec::with_capacity(spf);
        for i in 0..spf {
            interleaved.push(i as i16);
        }
        let buf =
            PcmBuffer::from_i16_interleaved(&interleaved, ChannelMode::Mono, MpegVersion::Mpeg2Lsf)
                .expect("valid LSF mono buffer");
        assert_eq!(buf.samples_per_channel(), spf);
        assert_eq!(buf.channel_count(), 1);
        let mono = buf.channel(0);
        assert_eq!(mono.len(), spf);
        assert!((mono[0] - 0.0).abs() < 1e-6);
        assert!((mono[spf - 1] - ((spf - 1) as f32 / 32768.0)).abs() < 1e-6);
    }

    #[test]
    fn f32_deinterleave_stereo_mpeg1() {
        let spf = MpegVersion::Mpeg1.samples_per_frame(); // 1152
        let total = 2 * spf;
        let mut interleaved = Vec::with_capacity(total);
        for i in 0..spf {
            interleaved.push(i as f32 / 100.0);
            interleaved.push((i as f32 + 0.5) / 100.0);
        }
        let buf =
            PcmBuffer::from_f32_interleaved(&interleaved, ChannelMode::Stereo, MpegVersion::Mpeg1)
                .expect("valid f32 stereo buffer");
        assert_eq!(buf.samples_per_channel(), spf);
        let left = buf.channel(0);
        let right = buf.channel(1);
        assert!((left[0] - 0.0).abs() < 1e-6);
        assert!((left[1] - 0.01).abs() < 1e-6);
        assert!((right[0] - 0.005).abs() < 1e-6);
    }

    #[test]
    fn f32_deinterleave_mono_lsf() {
        let spf = MpegVersion::Mpeg2Lsf.samples_per_frame(); // 576
        let mut interleaved = Vec::with_capacity(spf);
        for i in 0..spf {
            interleaved.push(i as f32 / 100.0);
        }
        let buf =
            PcmBuffer::from_f32_interleaved(&interleaved, ChannelMode::Mono, MpegVersion::Mpeg2Lsf)
                .expect("valid f32 LSF mono buffer");
        assert_eq!(buf.samples_per_channel(), spf);
        assert_eq!(buf.channel_count(), 1);
    }

    #[test]
    fn rejects_wrong_length_i16() {
        let samples = [0i16; 10]; // too short for any frame size
        let err = PcmBuffer::from_i16_interleaved(&samples, ChannelMode::Mono, MpegVersion::Mpeg1)
            .unwrap_err();
        assert!(matches!(err, EncodeError::BufferLengthMismatch { .. }));
    }

    #[test]
    fn rejects_wrong_length_f32() {
        let samples = [0.0f32; 10];
        let err =
            PcmBuffer::from_f32_interleaved(&samples, ChannelMode::Stereo, MpegVersion::Mpeg1)
                .unwrap_err();
        assert!(matches!(err, EncodeError::BufferLengthMismatch { .. }));
    }

    #[test]
    fn channel_0_is_used_for_mono() {
        let spf = MpegVersion::Mpeg2Lsf.samples_per_frame();
        let samples: Vec<i16> = (0..spf as i16).collect();
        let buf =
            PcmBuffer::from_i16_interleaved(&samples, ChannelMode::Mono, MpegVersion::Mpeg2Lsf)
                .expect("valid mono buffer");
        let chan = buf.channel(0);
        assert_eq!(chan.len(), spf);
        assert!((chan[5] - (5.0 / 32768.0)).abs() < 1e-6);
    }
}
