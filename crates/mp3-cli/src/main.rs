//! `encorust`: a thin CLI wrapping `mp3-core`. See
//! `docs/mp3-encoder/12-phase9-cli-and-wasm.md` §1.
//!
//! # Scaffold status
//!
//! Argument parsing and WAV reading below are real, working code — they
//! don't depend on any not-yet-implemented DSP. Driving `mp3_core::Encoder`
//! will currently panic (`todo!()`) until milestones M1-M8 land; see
//! `docs/mp3-encoder/14-roadmap-and-milestones.md` for status.

use std::path::PathBuf;

use clap::Parser;
use mp3_core::{Bitrate, ChannelMode, EncoderConfig, SampleRate};

/// WAV -> MP3 encoder built on the pure-Rust `mp3-core` crate.
#[derive(Parser, Debug)]
#[command(name = "encorust", version, about)]
struct Args {
    /// Input WAV file.
    input: PathBuf,

    /// Output MP3 file.
    #[arg(short, long)]
    output: PathBuf,

    /// Constant/average bitrate in kbps (mutually exclusive with
    /// `--vbr-quality`).
    #[arg(short = 'b', long, conflicts_with = "vbr_quality")]
    bitrate: Option<u32>,

    /// VBR quality target, 0 (highest) - 9 (smallest) — see
    /// `docs/mp3-encoder/10-phase7-bit-reservoir-and-rate-control.md` §4.
    #[arg(long, conflicts_with = "bitrate")]
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

    let wav = hound::WavReader::open(&args.input).unwrap_or_else(|e| {
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
        mp3_core::bitstream::RateControl::Cbr(bitrate)
    } else if let Some(q) = args.vbr_quality {
        mp3_core::bitstream::RateControl::Vbr(mp3_core::bitstream::reservoir::VbrQuality(q))
    } else {
        eprintln!("specify either --bitrate or --vbr-quality");
        std::process::exit(1);
    };

    let config = EncoderConfig {
        sample_rate,
        channel_mode,
        rate_control,
    };

    // TODO(M1-M9): once mp3_core::Encoder is implemented, read `wav` in
    // version.samples_per_frame()-sized chunks (see
    // docs/mp3-encoder/12-phase9-cli-and-wasm.md §1) and write
    // args.output. mp3_core::Encoder::new currently panics (todo!()) —
    // that is expected scaffold behavior, not a bug in this CLI.
    let _encoder = mp3_core::Encoder::new(config);
    eprintln!(
        "encorust: mp3-core is a scaffold (see \
         docs/mp3-encoder/14-roadmap-and-milestones.md) — encoding is not \
         yet implemented"
    );
}
