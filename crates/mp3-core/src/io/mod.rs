//! PCM ingestion. See `docs/mp3-encoder/04-phase1-pcm-io-and-framing.md`.
//!
//! This module never touches a filesystem or a container format (WAV,
//! etc.) — callers hand it raw interleaved samples. File/format handling
//! lives in `mp3-cli`.

mod pcm;

pub use pcm::PcmBuffer;
