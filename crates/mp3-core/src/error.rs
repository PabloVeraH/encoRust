//! Caller-facing error type.
//!
//! Internal DSP invariants that "cannot happen" given a correctly
//! implemented pipeline (e.g. an out-of-range Huffman table index derived
//! from an already-validated quantizer output) use `debug_assert!`
//! instead of `Result` — see `docs/mp3-encoder/01-architecture.md` §4.
//! `EncodeError` is reserved for failures that genuinely depend on
//! caller-supplied configuration or data.

/// Errors returned by `mp3-core`'s public API.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum EncodeError {
    /// The requested sample rate is not one of the six defined by
    /// ISO/IEC 11172-3 (MPEG-1) or ISO/IEC 13818-3 (MPEG-2 LSF).
    #[error("unsupported sample rate")]
    UnsupportedSampleRate,

    /// The requested channel count is not 1 (mono) or 2
    /// (stereo/joint-stereo/dual-mono).
    #[error("unsupported channel count: {got} (expected 1 or 2)")]
    UnsupportedChannelCount {
        /// The channel count that was rejected.
        got: usize,
    },

    /// A PCM buffer's length did not match the expected frame size. See
    /// `docs/mp3-encoder/04-phase1-pcm-io-and-framing.md`.
    #[error("PCM buffer length mismatch: expected {expected} samples, got {got}")]
    BufferLengthMismatch {
        /// Expected sample count.
        expected: usize,
        /// Actual sample count received.
        got: usize,
    },

    /// The requested bitrate is not valid for the configured MPEG
    /// version. See `docs/mp3-encoder/04-phase1-pcm-io-and-framing.md` §2.
    #[error("invalid bitrate: {kbps} kbps")]
    InvalidBitrate {
        /// The rejected bitrate, in kbps.
        kbps: u32,
    },

    /// The requested [`crate::types::ChannelMode`] is valid per the
    /// standard but this encoder doesn't implement it yet: joint stereo
    /// (MS/intensity) needs a cross-channel transform applied before
    /// quantization (chapter 08) that doesn't exist yet -- encoding
    /// content as `JointStereoMs`/`JointStereoIntensity` without it would
    /// produce a frame whose header claims a joint-stereo transform that
    /// was never applied, which a real decoder would misinterpret. Fail
    /// loudly here rather than silently emit that.
    #[error("channel mode not yet implemented by this encoder: {mode:?}")]
    UnsupportedChannelMode {
        /// The rejected channel mode.
        mode: crate::types::ChannelMode,
    },

    /// The requested [`crate::bitstream::reservoir::RateControl`] mode
    /// is not yet implemented: VBR and ABR are placeholders that silently
    /// produce fixed 128 kbps CBR output with no warning. Fail loudly
    /// instead of silently ignoring the user's request. See
    /// `docs/investigation-log.md` §2.2.
    #[error("rate-control mode not yet implemented by this encoder: {variant}")]
    UnsupportedRateControl {
        /// The rejected rate-control variant ("Vbr" or "Abr").
        variant: &'static str,
    },
}
