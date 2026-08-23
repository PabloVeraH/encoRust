//! Real-time factor benchmark: measures wall-clock time to encode N
//! seconds of audio. See `docs/mp3-encoder/13-testing-and-validation.md`
//! "Real-time factor benchmarking" and the M9 Definition of Done
//! (`docs/mp3-encoder/12-phase9-cli-and-wasm.md` §4: "≥4x real-time
//! scalar, ≥15x with simd").
//!
//! Run: `cargo bench -p mp3-core`. Criterion reports mean wall-clock
//! time per iteration; the real-time factor is
//! `AUDIO_DURATION_SECONDS / mean_iteration_seconds`. Any RTF number
//! quoted from this benchmark in a milestone report must state the
//! hardware it ran on (chapter 13) — a number without that is not a
//! reproducible claim.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use mp3_core::bitstream::reservoir::RateControl;
use mp3_core::io::PcmBuffer;
use mp3_core::{Bitrate, ChannelMode, Encoder, EncoderConfig, SampleRate};

const AUDIO_DURATION_SECONDS: usize = 5;
const FRAME_SAMPLES: usize = 1152; // MPEG-1, samples per channel per frame

/// Deterministic broadband pseudo-noise, not silence or a pure tone --
/// per this project's own repeated finding across the M7/M8 reviews, a
/// sparse spectrum compresses to near-zero bits and would understate
/// real encoding cost (fewer nonzero Huffman symbols, fewer outer-loop
/// iterations).
fn noise_frame(seed: &mut u32, n_channels: usize) -> Vec<i16> {
    (0..FRAME_SAMPLES * n_channels)
        .map(|_| {
            *seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            (((*seed >> 16) as i32 - 32768) / 4) as i16
        })
        .collect()
}

fn bench_encode(c: &mut Criterion, name: &str, channel_mode: ChannelMode) {
    let n_channels = channel_mode.channel_count();
    let frames_per_run = (AUDIO_DURATION_SECONDS * 44_100) / FRAME_SAMPLES;

    // Generate the PCM once, outside the timed closure -- only encoding
    // itself should be measured.
    let mut seed = 42u32;
    let frames: Vec<Vec<i16>> = (0..frames_per_run)
        .map(|_| noise_frame(&mut seed, n_channels))
        .collect();

    c.bench_function(name, |b| {
        b.iter(|| {
            let config = EncoderConfig {
                sample_rate: SampleRate::Hz44100,
                channel_mode,
                rate_control: RateControl::Cbr(Bitrate::Kbps128),
            };
            let mut encoder = Encoder::new(config).expect("encoder creation");
            let mut out = Vec::new();
            for frame in &frames {
                let pcm = PcmBuffer::from_i16_interleaved(
                    frame,
                    channel_mode,
                    SampleRate::Hz44100.version(),
                )
                .expect("PCM buffer");
                encoder.encode_frame(&pcm, &mut out).expect("encode");
            }
            black_box(out.len());
        });
    });
}

fn encode_stereo_128kbps(c: &mut Criterion) {
    bench_encode(
        c,
        &format!("encode_{AUDIO_DURATION_SECONDS}s_stereo_128kbps"),
        ChannelMode::Stereo,
    );
}

fn encode_mono_128kbps(c: &mut Criterion) {
    bench_encode(
        c,
        &format!("encode_{AUDIO_DURATION_SECONDS}s_mono_128kbps"),
        ChannelMode::Mono,
    );
}

criterion_group!(benches, encode_stereo_128kbps, encode_mono_128kbps);
criterion_main!(benches);
