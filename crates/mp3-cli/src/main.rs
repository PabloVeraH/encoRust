//! `encorust`: a thin CLI wrapping `mp3-core`. See
//! `docs/mp3-encoder/12-phase9-cli-and-wasm.md` §1.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

use clap::Parser;
use mp3_core::bitstream::RateControl;
use mp3_core::io::PcmBuffer;
use mp3_core::{Bitrate, ChannelMode, EncoderConfig, MpegVersion, SampleRate};

/// WAV -> MP3 encoder built on the pure-Rust `mp3-core` crate.
#[derive(Parser, Debug)]
#[command(name = "encorust", version, about)]
struct Args {
    /// Input WAV file.
    input: PathBuf,

    /// Output MP3 file.
    #[arg(short, long)]
    output: PathBuf,

    /// Constant bitrate in kbps (mutually exclusive with `--abr` and
    /// `--vbr-quality`).
    #[arg(short = 'b', long, conflicts_with_all = ["vbr_quality", "abr"])]
    bitrate: Option<u32>,

    /// Average bitrate in kbps (mutually exclusive with `--bitrate` and
    /// `--vbr-quality`).
    #[arg(long, conflicts_with_all = ["bitrate", "vbr_quality"])]
    abr: Option<u32>,

    /// VBR quality target, 0 (highest) - 9 (smallest) — see
    /// `docs/mp3-encoder/10-phase7-bit-reservoir-and-rate-control.md` §4.
    #[arg(long, conflicts_with_all = ["bitrate", "abr"])]
    vbr_quality: Option<u8>,
}

fn sample_rate_from_hz(hz: u32) -> Option<SampleRate> {
    match hz {
        44_100 => Some(SampleRate::Hz44100),
        48_000 => Some(SampleRate::Hz48000),
        32_000 => Some(SampleRate::Hz32000),
        22_050 => Some(SampleRate::Hz22050),
        24_000 => Some(SampleRate::Hz24000),
        16_000 => Some(SampleRate::Hz16000),
        _ => None,
    }
}

fn main() {
    let args = Args::parse();

    let mut wav = hound::WavReader::open(&args.input).unwrap_or_else(|e| {
        eprintln!("failed to open {}: {e}", args.input.display());
        std::process::exit(1);
    });
    let spec = wav.spec();

    let sample_rate = sample_rate_from_hz(spec.sample_rate).unwrap_or_else(|| {
        eprintln!(
            "unsupported sample rate {} Hz — see \
             docs/mp3-encoder/04-phase1-pcm-io-and-framing.md §2 for the \
             supported set",
            spec.sample_rate
        );
        std::process::exit(1);
    });

    let channel_mode = match spec.channels {
        1 => ChannelMode::Mono,
        2 => ChannelMode::Stereo,
        n => {
            eprintln!("unsupported channel count: {n} (expected 1 or 2)");
            std::process::exit(1);
        }
    };

    let rate_control = if let Some(kbps) = args.bitrate {
        let bitrate = Bitrate::from_kbps(kbps).unwrap_or_else(|| {
            eprintln!(
                "unsupported bitrate {kbps} kbps — see \
                 docs/mp3-encoder/04-phase1-pcm-io-and-framing.md §2 for \
                 the legal MPEG-1/LSF values"
            );
            std::process::exit(1);
        });
        RateControl::Cbr(bitrate)
    } else if let Some(kbps) = args.abr {
        let bitrate = Bitrate::from_kbps(kbps).unwrap_or_else(|| {
            eprintln!(
                "unsupported bitrate {kbps} kbps — see \
                 docs/mp3-encoder/04-phase1-pcm-io-and-framing.md §2 for \
                 the legal MPEG-1/LSF values"
            );
            std::process::exit(1);
        });
        RateControl::Abr(bitrate)
    } else if let Some(q) = args.vbr_quality {
        RateControl::Vbr(mp3_core::bitstream::reservoir::VbrQuality(q))
    } else {
        eprintln!("specify --bitrate, --abr, or --vbr-quality");
        std::process::exit(1);
    };

    let config = EncoderConfig {
        sample_rate,
        channel_mode,
        rate_control,
    };

    let mut encoder = mp3_core::Encoder::new(config).unwrap_or_else(|e| {
        eprintln!("failed to create encoder: {e}");
        std::process::exit(1);
    });

    let version = sample_rate.version();
    let samples_per_frame = version.samples_per_frame();
    let n_channels = channel_mode.channel_count();
    let frame_chunk_size = samples_per_frame * n_channels;

    let out_file = File::create(&args.output).unwrap_or_else(|e| {
        eprintln!("failed to create {}: {e}", args.output.display());
        std::process::exit(1);
    });
    let mut out_writer = BufWriter::new(out_file);

    let mut sample_buf: Vec<i16> = Vec::with_capacity(frame_chunk_size);
    let mut total_frames: u64 = 0;

    for sample_result in wav.samples::<i16>() {
        let sample = sample_result.unwrap_or_else(|e| {
            eprintln!("error reading WAV sample: {e}");
            std::process::exit(1);
        });
        sample_buf.push(sample);

        if sample_buf.len() >= frame_chunk_size {
            encode_and_write(
                &sample_buf[..frame_chunk_size],
                &mut encoder,
                &mut out_writer,
                channel_mode,
                version,
            );
            // Remove consumed samples, keeping any overflow
            if sample_buf.len() > frame_chunk_size {
                sample_buf.drain(..frame_chunk_size);
            } else {
                sample_buf.clear();
            }
            total_frames += 1;
        }
    }

    // Handle trailing partial frame (pad with silence)
    if !sample_buf.is_empty() {
        sample_buf.resize(frame_chunk_size, 0i16);
        encode_and_write(
            &sample_buf,
            &mut encoder,
            &mut out_writer,
            channel_mode,
            version,
        );
        total_frames += 1;
    }

    // Flush end-of-stream
    let mut finish_buf = Vec::new();
    let finish_bytes = encoder.finish(&mut finish_buf).unwrap_or_else(|e| {
        eprintln!("encoder finish failed: {e}");
        std::process::exit(1);
    });
    if finish_bytes > 0 {
        out_writer
            .write_all(&finish_buf[..finish_bytes])
            .unwrap_or_else(|e| {
                eprintln!("failed to write finish bytes: {e}");
                std::process::exit(1);
            });
    }

    out_writer.flush().unwrap_or_else(|e| {
        eprintln!("failed to flush output: {e}");
        std::process::exit(1);
    });

    if total_frames == 0 {
        eprintln!("warning: WAV file had no samples — output is empty");
    }
}

fn encode_and_write(
    frame_samples: &[i16],
    encoder: &mut mp3_core::Encoder,
    out_writer: &mut BufWriter<File>,
    channel_mode: ChannelMode,
    version: MpegVersion,
) {
    let pcm =
        PcmBuffer::from_i16_interleaved(frame_samples, channel_mode, version).unwrap_or_else(|e| {
            eprintln!("internal error building PCM buffer: {e}");
            std::process::exit(1);
        });

    let mut mp3_bytes = Vec::new();
    encoder
        .encode_frame(&pcm, &mut mp3_bytes)
        .unwrap_or_else(|e| {
            eprintln!("encoding failed: {e}");
            std::process::exit(1);
        });

    out_writer.write_all(&mp3_bytes).unwrap_or_else(|e| {
        eprintln!("failed to write MP3 frame: {e}");
        std::process::exit(1);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_rate_from_hz_accepts_every_legal_mpeg1_and_lsf_rate() {
        assert_eq!(sample_rate_from_hz(44_100), Some(SampleRate::Hz44100));
        assert_eq!(sample_rate_from_hz(48_000), Some(SampleRate::Hz48000));
        assert_eq!(sample_rate_from_hz(32_000), Some(SampleRate::Hz32000));
        assert_eq!(sample_rate_from_hz(22_050), Some(SampleRate::Hz22050));
        assert_eq!(sample_rate_from_hz(24_000), Some(SampleRate::Hz24000));
        assert_eq!(sample_rate_from_hz(16_000), Some(SampleRate::Hz16000));
    }

    #[test]
    fn sample_rate_from_hz_rejects_unsupported_rates() {
        // 11025/8000 Hz are real WAV sample rates but not legal for
        // either MPEG-1 or MPEG-2 LSF Layer III (see
        // docs/mp3-encoder/04-phase1-pcm-io-and-framing.md §2).
        assert_eq!(sample_rate_from_hz(11_025), None);
        assert_eq!(sample_rate_from_hz(8_000), None);
        assert_eq!(sample_rate_from_hz(0), None);
    }
}
