//! MPEG frame header bit-packing. See
//! `docs/mp3-encoder/04-phase1-pcm-io-and-framing.md`.

mod header;

pub use header::FrameHeader;
