//! Shared types and standard-mandated framing constants.
//!
//! See `docs/mp3-encoder/04-phase1-pcm-io-and-framing.md` for the standard
//! clauses each of these maps to. Named constants exist so every later
//! module references `SAMPLES_PER_GRANULE` etc. by name instead of a bare
//! literal — see `docs/mp3-encoder/01-architecture.md` §4.

// --- Framing constants (ISO/IEC 11172-3 §2.4.1; ISO/IEC 13818-3) ---

/// Samples per channel per granule — universal across MPEG-1 and
/// MPEG-2 LSF for Layer III.
pub const SAMPLES_PER_GRANULE: usize = 576;

/// Maximum granules per frame across supported versions (the MPEG-1
/// value). **MPEG-2 LSF frames carry only ONE granule** — use
/// [`MpegVersion::granules_per_frame`] for the per-version value; use
/// this constant only to size fixed buffers. See
/// `docs/mp3-encoder/04-phase1-pcm-io-and-framing.md` §1.
pub const MAX_GRANULES_PER_FRAME: usize = 2;

/// Maximum samples per channel per frame (the MPEG-1 value, 1152; LSF
/// frames are half this — [`MpegVersion::samples_per_frame`]).
/// Buffer-sizing only — never use as "the" frame length in logic.
pub const MAX_SAMPLES_PER_FRAME: usize = MAX_GRANULES_PER_FRAME * SAMPLES_PER_GRANULE;

/// The analysis polyphase filterbank always operates on 32 subbands.
pub const SUBBANDS: usize = 32;

/// Spectral lines per subband per granule for long blocks
/// (`SUBBANDS * 18 == SAMPLES_PER_GRANULE`).
pub const SAMPLES_PER_SUBBAND_PER_GRANULE: usize = 18;

/// Number of short windows per granule when `BlockType::Short` is active.
pub const SHORT_WINDOWS_PER_GRANULE: usize = 3;

/// Spectral lines per short window, per subband
/// (`SUBBANDS * 6 * SHORT_WINDOWS_PER_GRANULE == SAMPLES_PER_GRANULE`).
pub const SAMPLES_PER_SUBBAND_PER_SHORT_WINDOW: usize = 6;

/// Maximum channel count this encoder supports (mono = 1, everything
/// else = 2; true multichannel is out of scope, see
/// `docs/mp3-encoder/00-overview.md` §1).
pub const MAX_CHANNELS: usize = 2;

/// MPEG audio version — determines which sample-rate/bitrate table
/// applies. `Mpeg1` is ISO/IEC 11172-3; `Mpeg2Lsf` ("Low Sampling
/// Frequency") is ISO/IEC 13818-3.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MpegVersion {
    /// ISO/IEC 11172-3 — 32/44.1/48 kHz.
    Mpeg1,
    /// ISO/IEC 13818-3 — 16/22.05/24 kHz.
    Mpeg2Lsf,
}

impl MpegVersion {
    /// Granules per frame: 2 for MPEG-1, **1 for MPEG-2 LSF** — the
    /// structural difference that cascades into the frame-length formula
    /// (chapter 10 §4), side-info size (chapter 11 §4), and the
    /// `encode_frame` contract. See
    /// `docs/mp3-encoder/04-phase1-pcm-io-and-framing.md` §1.
    #[must_use]
    pub const fn granules_per_frame(self) -> usize {
        match self {
            Self::Mpeg1 => 2,
            Self::Mpeg2Lsf => 1,
        }
    }

    /// Samples per channel per frame: 1152 (MPEG-1) or 576 (MPEG-2 LSF).
    #[must_use]
    pub const fn samples_per_frame(self) -> usize {
        self.granules_per_frame() * SAMPLES_PER_GRANULE
    }
}

/// Supported sample rates across both MPEG-1 and MPEG-2 LSF. MPEG-2.5
/// (an unofficial de facto extension for 8/11.025/12 kHz, never ISO
/// standardized) is intentionally not included — see
/// `docs/mp3-encoder/00-overview.md` §1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleRate {
    /// 44.1 kHz (MPEG-1).
    Hz44100,
    /// 48 kHz (MPEG-1).
    Hz48000,
    /// 32 kHz (MPEG-1).
    Hz32000,
    /// 22.05 kHz (MPEG-2 LSF).
    Hz22050,
    /// 24 kHz (MPEG-2 LSF).
    Hz24000,
    /// 16 kHz (MPEG-2 LSF).
    Hz16000,
}

impl SampleRate {
    /// Which MPEG version's tables this sample rate belongs to.
    #[must_use]
    pub const fn version(self) -> MpegVersion {
        match self {
            Self::Hz44100 | Self::Hz48000 | Self::Hz32000 => MpegVersion::Mpeg1,
            Self::Hz22050 | Self::Hz24000 | Self::Hz16000 => MpegVersion::Mpeg2Lsf,
        }
    }

    /// The sample rate in Hz.
    #[must_use]
    pub const fn as_hz(self) -> u32 {
        match self {
            Self::Hz44100 => 44_100,
            Self::Hz48000 => 48_000,
            Self::Hz32000 => 32_000,
            Self::Hz22050 => 22_050,
            Self::Hz24000 => 24_000,
            Self::Hz16000 => 16_000,
        }
    }

    /// The 2-bit `sampling_frequency` frame header field value (shared
    /// between MPEG-1 and MPEG-2 LSF; the header's 2-bit version field
    /// disambiguates which table applies on decode).
    ///
    /// Expected mapping per the guide (`docs/mp3-encoder/04-phase1` §2 —
    /// a secondary source; the pinning test must cite Annex B or two
    /// cross-checked decoders): `00` = 44100/22050, `01` = 48000/24000,
    /// `10` = 32000/16000, `11` = reserved.
    ///
    /// # Panics
    ///
    /// Always, in this scaffold — implement in M1 alongside its
    /// provenance test. See `docs/mp3-encoder/00-overview.md` §4.1.
    #[must_use]
    pub fn header_bits(self) -> u8 {
        todo!("M1: implement per 04-phase1 §2 + provenance test vs. Annex B Table B.1")
    }
}

/// Encoder-facing bitrate selection for CBR/ABR, in kbps. Validity
/// against the version-specific bitrate table (14 legal values per
/// ISO/IEC 11172-3 Annex B, Table B.1 for MPEG-1; a different, lower
/// table for MPEG-2 LSF) is enforced at `Encoder::new` — see
/// `docs/mp3-encoder/04-phase1-pcm-io-and-framing.md` §2.
///
/// TODO(M1): replace this newtype with a closed enum over the exact
/// legal values once the bitrate table has been transcribed and
/// cross-checked, so invalid bitrates are unrepresentable rather than
/// merely rejected at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bitrate(pub u32);

/// How channel data is coded. `JointStereoMs`/`JointStereoIntensity` are
/// **encoder-side strategy choices**, not raw bitstream fields — the
/// bitstream only records a 2-bit `mode` (mono/stereo/joint/dual) plus,
/// for joint stereo, a 2-bit `mode_extension`. See
/// `docs/mp3-encoder/04-phase1-pcm-io-and-framing.md` §3.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelMode {
    /// Single channel.
    Mono,
    /// Two independently coded channels.
    Stereo,
    /// Two channels coded as mid/side.
    JointStereoMs,
    /// Two channels coded with intensity stereo above some frequency.
    JointStereoIntensity,
    /// Two independent mono channels transmitted together (not a
    /// perceptual stereo technique — e.g. bilingual broadcast audio).
    DualMono,
}

impl ChannelMode {
    /// Number of PCM input channels this mode expects.
    #[must_use]
    pub const fn channel_count(self) -> usize {
        match self {
            Self::Mono => 1,
            Self::Stereo | Self::JointStereoMs | Self::JointStereoIntensity | Self::DualMono => 2,
        }
    }
}
