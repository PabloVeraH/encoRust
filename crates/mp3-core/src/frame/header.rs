//! The 32-bit MPEG frame header. Pure bit-packing, no DSP — see
//! `docs/mp3-encoder/04-phase1-pcm-io-and-framing.md` §6, and
//! `docs/mp3-encoder/11-phase8-bitstream-multiplexing.md` §2 for where
//! this fits in the full frame layout.

use crate::types::{Bitrate, ChannelMode, SampleRate};

/// Every field of an MP3 frame header (ISO/IEC 11172-3 Annex B bit
/// layout). Layer is always Layer III for this encoder, so it is not a
/// field here — `to_bits` hard-codes the Layer III bit pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameHeader {
    /// Sample rate (encodes both the `sampling_frequency` field and,
    /// indirectly via [`SampleRate::version`], the `ID` version bit).
    pub sample_rate: SampleRate,
    /// `protection_bit`: `false` means a CRC follows the header (see
    /// `docs/mp3-encoder/11-phase8-bitstream-multiplexing.md` §3). Note
    /// the standard's inverted polarity — `1` means *no* CRC.
    pub crc_protected: bool,
    /// Selected bitrate for this frame (CBR: constant across all frames;
    /// VBR/ABR: chosen per-frame after quantization, see
    /// `docs/mp3-encoder/10-phase7-bit-reservoir-and-rate-control.md`).
    pub bitrate: Bitrate,
    /// Whether this frame carries one extra padding byte to make the
    /// average bitrate hit its target exactly (frame sizes computed by
    /// the standard's formula are not always integers).
    pub padding: bool,
    pub(crate) private_bit: bool,
    /// Stereo/joint/dual/mono coding mode.
    pub channel_mode: ChannelMode,
    /// Copyright flag — passthrough metadata, not enforced by this
    /// encoder.
    pub copyright: bool,
    /// "Original" flag — passthrough metadata.
    pub original: bool,
}

impl FrameHeader {
    /// Packs this header into its 32-bit on-the-wire representation
    /// (MSB-first, as consumed by [`crate::bitstream::writer::BitWriter`]).
    ///
    /// # Panics
    ///
    /// Always, in this scaffold — every field-to-bit-position mapping
    /// here must be transcribed from ISO/IEC 11172-3 Annex B and unit
    /// tested per field before this is real. See
    /// `docs/mp3-encoder/04-phase1-pcm-io-and-framing.md` §6.
    #[must_use]
    pub fn to_bits(self) -> u32 {
        todo!(
            "M1: pack sync(11) + version(2) + layer(2)=Layer3 + \
               protection(1) + bitrate_index(4) + sampling_frequency(2) + \
               padding(1) + private(1) + mode(2) + mode_extension(2) + \
               copyright(1) + original(1) + emphasis(2) = 32 bits — field \
               values in docs/mp3-encoder/04-phase1 §2/§6; the widths \
               MUST sum to 32 (a 31-bit sum means version was written as \
               1 bit — a real bug this scaffold shipped with once)"
        )
    }
}

#[cfg(test)]
mod tests {
    // TODO(M1): for a representative set of FrameHeader values, assert
    // to_bits() places each field at its documented bit position (write
    // a test-only inverse parser as the checking tool — mp3-core itself
    // never needs to parse headers it didn't write). See
    // docs/mp3-encoder/04-phase1-pcm-io-and-framing.md §6.
}
