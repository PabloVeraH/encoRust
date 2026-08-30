//! M8 end-to-end tests: encode PCM → MP3 frame → verify structure.
//!
//! See `docs/mp3-encoder/11-phase8-bitstream-multiplexing.md` §7.

use mp3_core::types::MpegVersion;
use mp3_core::{
    bitstream::reservoir::RateControl, io::PcmBuffer, Bitrate, ChannelMode, Encoder, EncoderConfig,
    SampleRate,
};

/// Encode a single frame of silence and verify the output is a valid
/// MPEG-1 frame (syncword, correct header fields, side-info size).
#[test]
fn encode_single_frame_produces_valid_header() {
    let config = EncoderConfig::new(
        SampleRate::Hz44100,
        ChannelMode::Mono,
        RateControl::Cbr(Bitrate::Kbps128),
    );
    let mut encoder = Encoder::new(config).expect("encoder creation");

    // 1152 mono samples for MPEG-1
    let samples: Vec<i16> = vec![0; 1152];
    let pcm = PcmBuffer::from_i16_interleaved(&samples, ChannelMode::Mono, MpegVersion::Mpeg1)
        .expect("PCM buffer");

    let mut out = Vec::new();
    let bytes_written = encoder.encode_frame(&pcm, &mut out).expect("encode");

    // Frame should be 417 bytes at 128 kbps / 44.1 kHz (unpadded)
    // floor(144000 * 128 / 44100) = floor(417.23...) = 417
    assert_eq!(bytes_written, 417);
    assert_eq!(out.len(), 417);

    // Check syncword: first 11 bits should be all 1s
    let header_bytes = [out[0], out[1], out[2], out[3]];
    let header = u32::from_be_bytes(header_bytes);
    let syncword = header >> 21;
    assert_eq!(syncword, 0x7FF, "invalid syncword: 0x{:X}", syncword);

    // Check version bits (bits 19-20): 11 = MPEG-1
    let version_bits = (header >> 19) & 0x3;
    assert_eq!(version_bits, 0b11, "expected MPEG-1");

    // Check layer bits (bits 17-18): 01 = Layer III
    let layer_bits = (header >> 17) & 0x3;
    assert_eq!(layer_bits, 0b01, "expected Layer III");

    // Check protection_bit (bit 16): 1 = no CRC
    let protection = (header >> 16) & 0x1;
    assert_eq!(protection, 1, "expected no CRC");

    // Check bitrate index (bits 12-15): 128 kbps @ MPEG-1 = index 9
    let br_index = (header >> 12) & 0xF;
    assert_eq!(br_index, 9, "expected bitrate index 9 for 128 kbps MPEG-1");

    // Check sample rate index (bits 10-11): 44100 Hz @ MPEG-1 = index 0
    let sr_index = (header >> 10) & 0x3;
    assert_eq!(sr_index, 0, "expected sample rate index 0 for 44100 Hz");

    // Check padding bit (bit 9): should be 0 for first frame at 128/44.1
    let padding = (header >> 9) & 0x1;
    assert_eq!(padding, 0, "unexpected padding");

    // Check channel mode (bits 6-7): 11 = mono
    let mode = (header >> 6) & 0x3;
    assert_eq!(mode, 0b11, "expected mono mode");

    // Side info starts at byte 4, should be 17 bytes for mono
    // Verify it's all non-zero or at least present
    assert!(out.len() > 4 + 17, "frame too short for header + side info");
}

/// Encode a single stereo frame and verify the header.
#[test]
fn encode_stereo_frame_produces_valid_header() {
    let config = EncoderConfig::new(
        SampleRate::Hz44100,
        ChannelMode::Stereo,
        RateControl::Cbr(Bitrate::Kbps192),
    );
    let mut encoder = Encoder::new(config).expect("encoder creation");

    // 1152 stereo samples (2304 interleaved)
    let samples: Vec<i16> = vec![0; 2304]; // 1152 * 2 = 2304
    let pcm = PcmBuffer::from_i16_interleaved(&samples, ChannelMode::Stereo, MpegVersion::Mpeg1)
        .expect("PCM buffer");

    let mut out = Vec::new();
    let bytes_written = encoder.encode_frame(&pcm, &mut out).expect("encode");

    // 192 kbps @ 44.1 kHz: floor(144000 * 192 / 44100) = floor(626.94) = 626
    assert_eq!(bytes_written, 626);
    assert_eq!(out.len(), 626);

    // Verify syncword
    let header = u32::from_be_bytes([out[0], out[1], out[2], out[3]]);
    let syncword = header >> 21;
    assert_eq!(syncword, 0x7FF);

    // Channel mode should be stereo (00)
    let mode = (header >> 6) & 0x3;
    assert_eq!(mode, 0b00, "expected stereo mode");
}

/// Encode multiple frames and verify the padding accumulator works correctly.
#[test]
fn multiple_frames_have_correct_sizes() {
    let config = EncoderConfig::new(
        SampleRate::Hz44100,
        ChannelMode::Mono,
        RateControl::Cbr(Bitrate::Kbps128),
    );
    let mut encoder = Encoder::new(config).expect("encoder creation");

    let samples: Vec<i16> = vec![0; 1152];

    // At 128 kbps / 44.1 kHz, the fractional size is 417.9592... bytes
    // So most frames are padded (418); only rare frames are unpadded (417).
    // Frame 0 (accumulator=0): unpadded = 417.
    // Frame 1+ (accumulator > 0): padded = 418 for many frames until the
    // accumulator cycles back near 0.
    let mut sizes = Vec::new();
    for _ in 0..6 {
        let pcm = PcmBuffer::from_i16_interleaved(&samples, ChannelMode::Mono, MpegVersion::Mpeg1)
            .expect("PCM buffer");
        let mut out = Vec::new();
        let written = encoder.encode_frame(&pcm, &mut out).expect("encode");
        sizes.push(written);
    }

    // First frame is unpadded (417), subsequent frames are padded (418)
    assert_eq!(sizes[0], 417, "frame 0: unpadded");
    assert_eq!(sizes[1], 418, "frame 1: padded");
    assert_eq!(sizes[2], 418, "frame 2: padded");
    assert_eq!(sizes[3], 418, "frame 3: padded");
    assert_eq!(sizes[4], 418, "frame 4: padded");
    assert_eq!(sizes[5], 418, "frame 5: padded");
}

/// `Encoder::new` must refuse configurations it doesn't correctly
/// support rather than silently emitting a non-conformant bitstream --
/// see the M8 review notes on why LSF and joint-stereo were previously
/// silent-corruption footguns.
#[test]
fn rejects_lsf_sample_rates() {
    for sr in [
        SampleRate::Hz22050,
        SampleRate::Hz24000,
        SampleRate::Hz16000,
    ] {
        let config = EncoderConfig::new(sr, ChannelMode::Mono, RateControl::Cbr(Bitrate::Kbps128));
        assert!(
            Encoder::new(config).is_err(),
            "{sr:?} (LSF) should be rejected, not silently mis-encoded"
        );
    }
}

#[test]
fn rejects_joint_stereo_modes() {
    // JointStereoIntensity is still rejected (M12 implements MS, not intensity).
    let config = EncoderConfig::new(
        SampleRate::Hz44100,
        ChannelMode::JointStereoIntensity,
        RateControl::Cbr(Bitrate::Kbps128),
    );
    assert!(
        Encoder::new(config).is_err(),
        "JointStereoIntensity should still be rejected"
    );
}

#[test]
fn joint_stereo_ms_is_accepted() {
    let config = EncoderConfig::new(
        SampleRate::Hz44100,
        ChannelMode::JointStereoMs,
        RateControl::Cbr(Bitrate::Kbps128),
    );
    assert!(
        Encoder::new(config).is_ok(),
        "JointStereoMs should be accepted"
    );
}

/// `Encoder::new` succeeding says nothing about `encode_frame` actually
/// working. This exercises the real pipeline with stereo content
/// deliberately shaped so the two channels' psychoacoustic models are
/// likely to make *different* transient (block_type) decisions on the
/// same granule -- left channel has a sharp attack partway through the
/// stream, right channel stays quiet throughout -- which is exactly the
/// case the block-type reconciliation step (`docs/plus.md` M12 review)
/// exists for. This doesn't decode the output (no reference decoder
/// dependency in this crate), but every internal consistency invariant
/// `encode_frame` checks via `debug_assert!` (side-info bit widths,
/// main_data capacity, granule shape agreement) runs in a debug test
/// build, so a reconciliation regression that reintroduces mismatched
/// shapes has a real chance of tripping one of those rather than
/// silently producing a plausible-looking but wrong frame.
#[test]
fn joint_stereo_ms_encodes_divergent_channel_content_without_panicking() {
    let config = EncoderConfig::new(
        SampleRate::Hz44100,
        ChannelMode::JointStereoMs,
        RateControl::Cbr(Bitrate::Kbps128),
    );
    let mut encoder = Encoder::new(config).expect("encoder creation");

    let n_frames = 10;
    let n_samples = 1152 * n_frames;
    let mut interleaved = vec![0i16; n_samples * 2];
    for i in 0..n_samples {
        // Left: quiet, then a sharp attack after the halfway point.
        let left = if i < n_samples / 2 {
            (0.01 * libm::sinf(i as f32 * 0.5) * 20000.0) as i16
        } else {
            (0.85 * libm::sinf(i as f32 * 1.3) * 20000.0) as i16
        };
        // Right: stays quiet the whole time -- independently, its own
        // psychoacoustic model should have no reason to leave `Long`.
        let right = (0.01 * libm::sinf(i as f32 * 0.5) * 20000.0) as i16;
        interleaved[i * 2] = left;
        interleaved[i * 2 + 1] = right;
    }

    let mut out = Vec::new();
    let mut frame_offset = 0usize;
    for (i, chunk) in interleaved.chunks(1152 * 2).enumerate() {
        let pcm =
            PcmBuffer::from_i16_interleaved(chunk, ChannelMode::JointStereoMs, MpegVersion::Mpeg1)
                .expect("PCM buffer");
        let written = encoder
            .encode_frame(&pcm, &mut out)
            .expect("encode_frame must not error on divergent-channel content");
        assert!(
            written == 417 || written == 418,
            "frame {i}: unexpected size {written} (expected 417 or 418 for 128 kbps/44.1 kHz)"
        );

        let header = u32::from_be_bytes([
            out[frame_offset],
            out[frame_offset + 1],
            out[frame_offset + 2],
            out[frame_offset + 3],
        ]);
        assert_eq!(header >> 21, 0x7FF, "frame {i}: invalid syncword");
        frame_offset += written;
    }
    assert_eq!(out.len(), frame_offset);
    assert_eq!(n_frames, 10, "sanity check on the loop bound assumed above");
}

/// Found during the M9 review: `Encoder::new`'s own doc comment already
/// promised to reject an unsupported bitrate, but nothing actually
/// checked it -- `Bitrate::header_index` returns `None` for bitrates
/// that are legal for the *other* MPEG version (144 kbps is LSF-only,
/// 320 kbps is MPEG-1-only), and that mismatch went uncaught until deep
/// inside the first `encode_frame` call's `FrameHeader::to_bits()`. A
/// caller that doesn't loudly surface a rare `encode_frame` error (e.g.
/// `mp3-wasm`'s `WasmEncoder`, before this same review fixed it) would
/// then silently produce nothing forever. Reject at construction, where
/// every caller already handles a `Result`.
#[test]
fn rejects_bitrate_invalid_for_version() {
    // 144 kbps only has a header_index for MPEG-2 LSF, not MPEG-1.
    let config = EncoderConfig::new(
        SampleRate::Hz44100, // MPEG-1
        ChannelMode::Mono,
        RateControl::Cbr(Bitrate::Kbps144),
    );
    assert!(
        Encoder::new(config).is_err(),
        "144 kbps is LSF-only and must be rejected for an MPEG-1 sample rate"
    );
}

#[test]
fn accepts_discrete_stereo_and_dual_mono() {
    for mode in [ChannelMode::Stereo, ChannelMode::DualMono] {
        let config = EncoderConfig::new(
            SampleRate::Hz44100,
            mode,
            RateControl::Cbr(Bitrate::Kbps128),
        );
        assert!(
            Encoder::new(config).is_ok(),
            "{mode:?} doesn't need a cross-channel transform, should be accepted"
        );
    }
}

// --- Multi-frame main_data_begin round-trip ---
//
// Chapter 11 §5 calls a main_data_begin/reservoir mistake "the most
// likely source of 'produces a file but it doesn't decode' bugs" at this
// milestone. This section parses the *actual transmitted bytes* (not
// internal encoder state) back out with a minimal test-only bit reader,
// exactly as a decoder would, and checks the invariants a decoder
// depends on: main_data_begin never references data that hasn't been
// physically transmitted yet, and each frame's declared part2_3_length
// total is consistent with how many bytes it actually had available.

struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> BitReader<'a> {
    fn read_bits(&mut self, n: u8) -> u32 {
        let mut v = 0u32;
        for _ in 0..n {
            let byte = self.data[self.pos / 8];
            let bit = (byte >> (7 - self.pos % 8)) & 1;
            v = (v << 1) | u32::from(bit);
            self.pos += 1;
        }
        v
    }
}

/// Parsed side-info fields relevant to the main_data_begin invariant, for
/// one frame (mono, 2 granules -- MPEG-1 only, matching this encoder's
/// current scope).
struct ParsedSideInfo {
    main_data_begin: u32,
    part2_3_length_total: u32,
}

fn parse_mono_side_info(frame: &[u8]) -> ParsedSideInfo {
    let mut r = BitReader {
        data: &frame[4..], // skip the 4-byte header
        pos: 0,
    };
    let main_data_begin = r.read_bits(9);
    r.read_bits(5); // private_bits (mono)
    r.read_bits(4); // scfsi
    let mut total = 0u32;
    for _ in 0..2 {
        // 2 granules, 1 channel (mono)
        total += r.read_bits(12); // part2_3_length
        r.read_bits(9); // big_values
        r.read_bits(8); // global_gain
        r.read_bits(4); // scalefac_compress
        let ws = r.read_bits(1);
        if ws == 1 {
            r.read_bits(2 + 1 + 5 + 5 + 3 + 3 + 3); // block_type..subblock_gain×3
        } else {
            r.read_bits(5 + 5 + 5 + 4 + 3); // table_select×3, region0/1_count
        }
        r.read_bits(1 + 1 + 1); // preflag, scalefac_scale, count1table_select
    }
    ParsedSideInfo {
        main_data_begin,
        part2_3_length_total: total,
    }
}

#[test]
fn main_data_begin_never_references_untransmitted_bytes() {
    // This is the invariant a real decoder depends on (chapter 11 §5).
    // The current encoder keeps every frame self-contained
    // (main_data_begin == 0 always -- see Encoder's doc comment on the
    // reservoir's scope limitation), so this should hold trivially *for
    // now*; the value of this test is as a tripwire for whenever
    // cross-frame reservoir borrowing gets implemented, since a
    // first-attempt implementation of that got caught by exactly this
    // check (with this same quiet-then-loud content, chosen because
    // constant-complexity content never draws a reservoir down at all --
    // see the git history / M8 review notes for the failing run).
    let config = EncoderConfig::new(
        SampleRate::Hz44100,
        ChannelMode::Mono,
        RateControl::Cbr(Bitrate::Kbps64),
    );
    let mut encoder = Encoder::new(config).expect("encoder creation");

    let mut seed: u32 = 42;
    let mut next_sample = |divisor: i32| {
        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
        (((seed >> 16) as i32 - 32768) / divisor) as i16
    };

    const NUM_QUIET_FRAMES: usize = 4;
    const NUM_LOUD_FRAMES: usize = 8;
    let mut frame_bounds = Vec::new(); // (start, end) byte offsets in `out`
    let mut out = Vec::new();

    for frame_idx in 0..(NUM_QUIET_FRAMES + NUM_LOUD_FRAMES) {
        let divisor = if frame_idx < NUM_QUIET_FRAMES { 40 } else { 2 };
        let samples: Vec<i16> = (0..1152).map(|_| next_sample(divisor)).collect();
        let pcm = PcmBuffer::from_i16_interleaved(&samples, ChannelMode::Mono, MpegVersion::Mpeg1)
            .expect("PCM buffer");
        let start = out.len();
        encoder.encode_frame(&pcm, &mut out).expect("encode");
        frame_bounds.push((start, out.len()));
    }

    // Physical main_data byte position, tracked the same way a decoder
    // would: bytes accumulate frame over frame, main_data_begin looks
    // backward into that running total.
    let mut transmitted_main_data_bytes: u32 = 0;

    for (i, &(start, end)) in frame_bounds.iter().enumerate() {
        let frame = &out[start..end];
        let side_info_bytes = 17; // mono
        let this_frame_capacity = (frame.len() - 4 - side_info_bytes) as u32;

        let parsed = parse_mono_side_info(frame);

        assert!(
            parsed.main_data_begin <= transmitted_main_data_bytes,
            "frame {i}: main_data_begin ({}) references {} more bytes \
             than have been transmitted so far ({transmitted_main_data_bytes}) \
             -- a decoder would read uninitialized/wrong data",
            parsed.main_data_begin,
            parsed
                .main_data_begin
                .saturating_sub(transmitted_main_data_bytes),
        );

        if i == 0 {
            assert_eq!(
                parsed.main_data_begin, 0,
                "the very first frame has no prior data to reference back to"
            );
        }

        // part2_3_length is in *bits*; it must fit within what this
        // frame's declared main_data_begin backward-reference plus its
        // own physical capacity can actually supply.
        let available_bits = (parsed.main_data_begin + this_frame_capacity) * 8;
        assert!(
            parsed.part2_3_length_total <= available_bits,
            "frame {i}: part2_3_length total ({} bits) exceeds what main_data_begin \
             ({} bytes) + this frame's own capacity ({this_frame_capacity} bytes) \
             can supply ({available_bits} bits)",
            parsed.part2_3_length_total,
            parsed.main_data_begin,
        );

        transmitted_main_data_bytes += this_frame_capacity;
    }
}
