//! The 32-bit MPEG frame header. Pure bit-packing, no DSP — see
//! `docs/mp3-encoder/04-phase1-pcm-io-and-framing.md` §6, and
//! `docs/mp3-encoder/11-phase8-bitstream-multiplexing.md` §2 for where
//! this fits in the full frame layout.

use crate::types::{Bitrate, ChannelMode, SampleRate};

/// Every field of an MP3 frame header (ISO/IEC 11172-3 Annex B bit
/// layout). All fields pack into exactly 32 bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameHeader {
    /// Sample rate (encodes both the `sampling_frequency` field and,
    /// indirectly via [`SampleRate::version`], the version bit).
    pub sample_rate: SampleRate,
    /// `protection_bit` (inverted polarity on the wire):
    /// `true` = CRC follows the header; bit is `0` (CRC present).
    /// `false` = no CRC; bit is `1`. See
    /// `docs/mp3-encoder/11-phase8-bitstream-multiplexing.md` §3.
    pub crc_present: bool,
    /// Selected bitrate for this frame.
    pub bitrate: Bitrate,
    /// Whether this frame carries one extra padding byte.
    pub padding: bool,
    /// Private-use bit — always `false` from this encoder.
    pub private_bit: bool,
    /// Stereo/joint/dual/mono coding mode.
    pub channel_mode: ChannelMode,
    /// Copyright flag — passthrough metadata.
    pub copyright: bool,
    /// "Original" flag — passthrough metadata.
    pub original: bool,
}

impl FrameHeader {
    /// Packs this header into its 32-bit on-the-wire representation
    /// (MSB-first, as consumed by [`crate::bitstream::writer::BitWriter`]).
    ///
    /// Returns `None` if the bitrate is not a legal value for the
    /// MPEG version implied by [`SampleRate::version`].
    #[must_use]
    pub fn to_bits(self) -> Option<u32> {
        let version = self.sample_rate.version();
        let br_index = self.bitrate.header_index(version)?;

        let mut bits: u32 = 0;

        bits |= 0x7FF << 21; // syncword
        bits |= u32::from(version.header_bits()) << 19;
        bits |= 1u32 << 17; // layer = Layer III
        bits |= u32::from(!self.crc_present) << 16; // invert: 0=CRC present, 1=no CRC
        bits |= u32::from(br_index) << 12;
        bits |= u32::from(self.sample_rate.header_bits()) << 10;
        bits |= u32::from(self.padding) << 9;
        bits |= u32::from(self.private_bit) << 8;
        bits |= u32::from(self.channel_mode.header_mode_bits()) << 6;
        bits |= u32::from(self.channel_mode.header_mode_extension_bits()) << 4;
        bits |= u32::from(self.copyright) << 3;
        bits |= u32::from(self.original) << 2;
        // emphasis = 00 (none)

        Some(bits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Bitrate, ChannelMode, SampleRate};

    /// Test-only inverse parser: extracts each field from a 32-bit header,
    /// checking against the documented bit positions.
    struct RawHeader {
        syncword: u32,
        version: u32,
        layer: u32,
        protection: u32,
        bitrate_index: u32,
        sampling_freq: u32,
        padding: u32,
        private_bit: u32,
        mode: u32,
        mode_ext: u32,
        copyright: u32,
        original: u32,
        emphasis: u32,
    }

    impl RawHeader {
        fn from_bits(bits: u32) -> Self {
            Self {
                syncword: (bits >> 21) & 0x7FF,
                version: (bits >> 19) & 0x3,
                layer: (bits >> 17) & 0x3,
                protection: (bits >> 16) & 0x1,
                bitrate_index: (bits >> 12) & 0xF,
                sampling_freq: (bits >> 10) & 0x3,
                padding: (bits >> 9) & 0x1,
                private_bit: (bits >> 8) & 0x1,
                mode: (bits >> 6) & 0x3,
                mode_ext: (bits >> 4) & 0x3,
                copyright: (bits >> 3) & 0x1,
                original: (bits >> 2) & 0x1,
                emphasis: bits & 0x3,
            }
        }
    }

    fn make_header_mpeg1_stereo() -> FrameHeader {
        FrameHeader {
            sample_rate: SampleRate::Hz44100,
            crc_present: false,
            bitrate: Bitrate::Kbps128,
            padding: false,
            private_bit: false,
            channel_mode: ChannelMode::Stereo,
            copyright: false,
            original: true,
        }
    }

    fn make_header_mpeg2_mono() -> FrameHeader {
        FrameHeader {
            sample_rate: SampleRate::Hz22050,
            crc_present: true,
            bitrate: Bitrate::Kbps64,
            padding: true,
            private_bit: true,
            channel_mode: ChannelMode::Mono,
            copyright: true,
            original: false,
        }
    }

    fn make_header_joint_ms() -> FrameHeader {
        FrameHeader {
            sample_rate: SampleRate::Hz48000,
            crc_present: false,
            bitrate: Bitrate::Kbps192,
            padding: false,
            private_bit: false,
            channel_mode: ChannelMode::JointStereoMs,
            copyright: false,
            original: false,
        }
    }

    #[test]
    fn header_roundtrip_mpeg1_stereo() {
        let hdr = make_header_mpeg1_stereo();
        let bits = hdr.to_bits().expect("valid header");
        let raw = RawHeader::from_bits(bits);

        assert_eq!(raw.syncword, 0x7FF, "syncword");
        assert_eq!(raw.version, 0b11, "MPEG-1 version = 11");
        assert_eq!(raw.layer, 0b01, "Layer III = 01");
        assert_eq!(raw.protection, 1, "no CRC → protection_bit = 1");
        assert_eq!(raw.bitrate_index, 9, "128 kbps for MPEG-1 = index 9");
        assert_eq!(raw.sampling_freq, 0b00, "44100 = 00");
        assert_eq!(raw.padding, 0, "no padding");
        assert_eq!(raw.private_bit, 0);
        assert_eq!(raw.mode, 0b00, "stereo = 00");
        assert_eq!(raw.mode_ext, 0b00, "mode_extension = 00 (not joint)");
        assert_eq!(raw.copyright, 0);
        assert_eq!(raw.original, 1);
        assert_eq!(raw.emphasis, 0b00, "emphasis = none");
    }

    #[test]
    fn header_roundtrip_mpeg2_lsf_mono() {
        let hdr = make_header_mpeg2_mono();
        let bits = hdr.to_bits().expect("valid header");
        let raw = RawHeader::from_bits(bits);

        assert_eq!(raw.syncword, 0x7FF);
        assert_eq!(raw.version, 0b10, "MPEG-2 LSF version = 10");
        assert_eq!(raw.layer, 0b01);
        assert_eq!(raw.protection, 0, "CRC present → protection_bit = 0");
        assert_eq!(raw.bitrate_index, 8, "64 kbps for MPEG-2 LSF = index 8");
        assert_eq!(raw.sampling_freq, 0b00, "22050 = 00");
        assert_eq!(raw.padding, 1);
        assert_eq!(raw.private_bit, 1);
        assert_eq!(raw.mode, 0b11, "mono = 11");
        assert_eq!(raw.mode_ext, 0b00);
        assert_eq!(raw.copyright, 1);
        assert_eq!(raw.original, 0);
        assert_eq!(raw.emphasis, 0b00);
    }

    #[test]
    fn header_roundtrip_joint_stereo_ms() {
        let hdr = make_header_joint_ms();
        let bits = hdr.to_bits().expect("valid header");
        let raw = RawHeader::from_bits(bits);

        assert_eq!(raw.mode, 0b01, "joint stereo = 01");
        assert_eq!(raw.mode_ext, 0b10, "MS stereo = mode_ext 10");
    }

    #[test]
    fn header_invalid_bitrate_for_version_returns_none() {
        // Kbps8 is only legal for MPEG-2 LSF
        let hdr = FrameHeader {
            sample_rate: SampleRate::Hz44100, // MPEG-1
            crc_present: false,
            bitrate: Bitrate::Kbps8,
            padding: false,
            private_bit: false,
            channel_mode: ChannelMode::Mono,
            copyright: false,
            original: false,
        };
        assert!(hdr.to_bits().is_none());
    }

    #[test]
    fn header_syncword_is_top_11_bits() {
        let hdr = make_header_mpeg1_stereo();
        let bits = hdr.to_bits().unwrap();
        // MSB 11 bits should be all 1s, i.e., bits 31-21
        assert_eq!(bits >> 21, 0x7FF);
    }

    #[test]
    fn header_total_width_is_32() {
        let hdr = make_header_mpeg1_stereo();
        let bits = hdr.to_bits().unwrap();
        // Low 2 bits (emphasis) should be 00
        assert_eq!(bits & 0x3, 0b00);
    }

    #[test]
    fn bitrate_index_boundary_values() {
        // Low end: Kbps32 for MPEG-1 = index 1
        let hdr = FrameHeader {
            sample_rate: SampleRate::Hz44100,
            crc_present: false,
            bitrate: Bitrate::Kbps32,
            padding: false,
            private_bit: false,
            channel_mode: ChannelMode::Mono,
            copyright: false,
            original: false,
        };
        let bits = hdr.to_bits().unwrap();
        let raw = RawHeader::from_bits(bits);
        assert_eq!(raw.bitrate_index, 1);

        // High end: Kbps320 for MPEG-1 = index 14
        let hdr = FrameHeader {
            sample_rate: SampleRate::Hz44100,
            crc_present: false,
            bitrate: Bitrate::Kbps320,
            padding: false,
            private_bit: false,
            channel_mode: ChannelMode::Mono,
            copyright: false,
            original: false,
        };
        let bits = hdr.to_bits().unwrap();
        let raw = RawHeader::from_bits(bits);
        assert_eq!(raw.bitrate_index, 14);
    }
}
