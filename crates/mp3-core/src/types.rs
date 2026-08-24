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
#[non_exhaustive]
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

    /// 2-bit frame header `version` field: `11` = MPEG-1, `10` = MPEG-2 LSF.
    #[must_use]
    pub const fn header_bits(self) -> u8 {
        match self {
            Self::Mpeg1 => 0b11,
            Self::Mpeg2Lsf => 0b10,
        }
    }
}

/// Supported sample rates across both MPEG-1 and MPEG-2 LSF. MPEG-2.5
/// (an unofficial de facto extension for 8/11.025/12 kHz, never ISO
/// standardized) is intentionally not included — see
/// `docs/mp3-encoder/00-overview.md` §1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
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

    /// The 2-bit `sampling_frequency` frame header field value.
    ///
    /// The same 2-bit codes are reused between MPEG-1 and MPEG-2 LSF;
    /// the header's 2-bit version field disambiguates which sample-rate
    /// row applies on decode.
    ///
    /// ISO/IEC 11172-3 Annex B, Table B.1 / ISO/IEC 13818-3 §2.4.2.3:
    /// `00` = 44100/22050, `01` = 48000/24000, `10` = 32000/16000.
    #[must_use]
    pub fn header_bits(self) -> u8 {
        match self {
            Self::Hz44100 | Self::Hz22050 => 0b00,
            Self::Hz48000 | Self::Hz24000 => 0b01,
            Self::Hz32000 | Self::Hz16000 => 0b10,
        }
    }
}

/// Legal MPEG Layer III bitrates, per ISO/IEC 11172-3 Annex B Table B.1
/// (MPEG-1) and ISO/IEC 13818-3 (MPEG-2 LSF). "Parse, don't validate" —
/// an invalid bitrate is unrepresentable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Bitrate {
    /// 8 kbps — MPEG-2 LSF only.
    Kbps8,
    /// 16 kbps — MPEG-2 LSF only.
    Kbps16,
    /// 24 kbps — MPEG-2 LSF only.
    Kbps24,
    /// 32 kbps — both MPEG-1 (index 1) and MPEG-2 LSF (index 4).
    Kbps32,
    /// 40 kbps — both MPEG-1 (index 2) and MPEG-2 LSF (index 5).
    Kbps40,
    /// 48 kbps — both MPEG-1 (index 3) and MPEG-2 LSF (index 6).
    Kbps48,
    /// 56 kbps — both MPEG-1 (index 4) and MPEG-2 LSF (index 7).
    Kbps56,
    /// 64 kbps — both MPEG-1 (index 5) and MPEG-2 LSF (index 8).
    Kbps64,
    /// 80 kbps — both MPEG-1 (index 6) and MPEG-2 LSF (index 9).
    Kbps80,
    /// 96 kbps — both MPEG-1 (index 7) and MPEG-2 LSF (index 10).
    Kbps96,
    /// 112 kbps — both MPEG-1 (index 8) and MPEG-2 LSF (index 11).
    Kbps112,
    /// 128 kbps — both MPEG-1 (index 9) and MPEG-2 LSF (index 12).
    Kbps128,
    /// 144 kbps — MPEG-2 LSF only (index 13).
    Kbps144,
    /// 160 kbps — both MPEG-1 (index 10) and MPEG-2 LSF (index 14).
    Kbps160,
    /// 192 kbps — MPEG-1 only (index 11).
    Kbps192,
    /// 224 kbps — MPEG-1 only (index 12).
    Kbps224,
    /// 256 kbps — MPEG-1 only (index 13).
    Kbps256,
    /// 320 kbps — MPEG-1 only (index 14).
    Kbps320,
}

impl Bitrate {
    /// The bitrate value in kbps.
    #[must_use]
    pub const fn as_kbps(self) -> u32 {
        match self {
            Self::Kbps8 => 8,
            Self::Kbps16 => 16,
            Self::Kbps24 => 24,
            Self::Kbps32 => 32,
            Self::Kbps40 => 40,
            Self::Kbps48 => 48,
            Self::Kbps56 => 56,
            Self::Kbps64 => 64,
            Self::Kbps80 => 80,
            Self::Kbps96 => 96,
            Self::Kbps112 => 112,
            Self::Kbps128 => 128,
            Self::Kbps144 => 144,
            Self::Kbps160 => 160,
            Self::Kbps192 => 192,
            Self::Kbps224 => 224,
            Self::Kbps256 => 256,
            Self::Kbps320 => 320,
        }
    }

    /// Returns the 4-bit `bitrate_index` for this bitrate given a version,
    /// or `None` if this bitrate is not legal for that version.
    ///
    /// Index values 1–14 per the standard tables; index 0 (`free format`)
    /// is never returned by this encoder.
    #[must_use]
    pub fn header_index(self, version: MpegVersion) -> Option<u8> {
        match version {
            MpegVersion::Mpeg1 => match self {
                Self::Kbps32 => Some(1),
                Self::Kbps40 => Some(2),
                Self::Kbps48 => Some(3),
                Self::Kbps56 => Some(4),
                Self::Kbps64 => Some(5),
                Self::Kbps80 => Some(6),
                Self::Kbps96 => Some(7),
                Self::Kbps112 => Some(8),
                Self::Kbps128 => Some(9),
                Self::Kbps160 => Some(10),
                Self::Kbps192 => Some(11),
                Self::Kbps224 => Some(12),
                Self::Kbps256 => Some(13),
                Self::Kbps320 => Some(14),
                _ => None,
            },
            MpegVersion::Mpeg2Lsf => match self {
                Self::Kbps8 => Some(1),
                Self::Kbps16 => Some(2),
                Self::Kbps24 => Some(3),
                Self::Kbps32 => Some(4),
                Self::Kbps40 => Some(5),
                Self::Kbps48 => Some(6),
                Self::Kbps56 => Some(7),
                Self::Kbps64 => Some(8),
                Self::Kbps80 => Some(9),
                Self::Kbps96 => Some(10),
                Self::Kbps112 => Some(11),
                Self::Kbps128 => Some(12),
                Self::Kbps144 => Some(13),
                Self::Kbps160 => Some(14),
                _ => None,
            },
        }
    }

    /// Looks up a [`Bitrate`] by its kbps value.
    #[must_use]
    pub fn from_kbps(kbps: u32) -> Option<Self> {
        Some(match kbps {
            8 => Self::Kbps8,
            16 => Self::Kbps16,
            24 => Self::Kbps24,
            32 => Self::Kbps32,
            40 => Self::Kbps40,
            48 => Self::Kbps48,
            56 => Self::Kbps56,
            64 => Self::Kbps64,
            80 => Self::Kbps80,
            96 => Self::Kbps96,
            112 => Self::Kbps112,
            128 => Self::Kbps128,
            144 => Self::Kbps144,
            160 => Self::Kbps160,
            192 => Self::Kbps192,
            224 => Self::Kbps224,
            256 => Self::Kbps256,
            320 => Self::Kbps320,
            _ => return None,
        })
    }
}

/// How channel data is coded. `JointStereoMs`/`JointStereoIntensity` are
/// **encoder-side strategy choices**, not raw bitstream fields — the
/// bitstream only records a 2-bit `mode` (mono/stereo/joint/dual) plus,
/// for joint stereo, a 2-bit `mode_extension`. See
/// `docs/mp3-encoder/04-phase1-pcm-io-and-framing.md` §3.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
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

    /// 2-bit frame header `mode` field:
    /// `00` = stereo, `01` = joint stereo, `10` = dual channel, `11` = mono.
    #[must_use]
    pub const fn header_mode_bits(self) -> u8 {
        match self {
            Self::Stereo => 0b00,
            Self::JointStereoMs | Self::JointStereoIntensity => 0b01,
            Self::DualMono => 0b10,
            Self::Mono => 0b11,
        }
    }

    /// 2-bit `mode_extension` field — meaningful only when
    /// `mode == joint stereo` (`01`). Bit 1 = MS stereo on,
    /// bit 0 = intensity stereo on. Returns `00` for non-joint modes.
    #[must_use]
    pub const fn header_mode_extension_bits(self) -> u8 {
        match self {
            Self::JointStereoMs => 0b10,
            Self::JointStereoIntensity => 0b01,
            _ => 0b00,
        }
    }

    /// Whether this mode uses two coded channels (stereo, joint, or dual).
    #[must_use]
    pub const fn is_stereo(self) -> bool {
        self.channel_count() > 1
    }
}
