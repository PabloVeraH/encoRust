//! `mp3-core`: a pure-Rust MPEG-1/2 Layer III (MP3) encoder.
//!
//! This crate contains **only** the codec. It performs no file I/O and no
//! JS interop — see `mp3-cli` and `mp3-wasm` for those. It is
//! `no_std`-friendly (build with `--no-default-features`) so it can run
//! inside a `wasm32-unknown-unknown` real-time audio callback.
//!
//! # Public API
//!
//! The stable public API consists of [`Encoder`], [`EncoderConfig`],
//! [`EncodeError`], and the types re-exported from [`types`]:
//! [`Bitrate`], [`ChannelMode`], [`MpegVersion`], [`SampleRate`].
//! Additional types needed by `mp3-cli`/`mp3-wasm` — [`PcmBuffer`]
//! ([`io`]), [`RateControl`]/[`VbrQuality`] ([`bitstream`]) — are
//! re-exported at the crate root for convenience but share the same
//! semver-exempt status as the module they live in.
//!
//! # Internal modules
//!
//! The modules below are public *only* so integration tests can reach
//! them — they are explicitly exempt from semver guarantees.  External
//! consumers must not depend on their contents directly.  See
//! `docs/mp3-encoder/01-architecture.md` §5.
#![cfg_attr(not(feature = "std"), no_std)]
#![deny(missing_docs)]
#![warn(clippy::all)]
#![deny(unsafe_op_in_unsafe_fn)]

extern crate alloc;

pub mod bitstream;
mod encoder;
pub mod error;
#[doc(hidden)]
pub mod filterbank;
#[doc(hidden)]
pub mod frame;
#[doc(hidden)]
pub mod huffman;
pub mod io;
#[doc(hidden)]
pub mod mdct;
#[doc(hidden)]
pub mod psychoacoustic;
#[doc(hidden)]
pub mod quantize;
pub mod types;

pub use encoder::{Encoder, EncoderConfig};
pub use error::EncodeError;
pub use types::{Bitrate, ChannelMode, MpegVersion, SampleRate};

// Re-exports for convenience (mp3-cli/mp3-wasm need these). The
// canonical home is still `bitstream::reservoir` / `io::pcm`; these
// re-exports exist so callers don't have to reach into an
// implementation-detail module path.
pub use bitstream::reservoir::RateControl;
pub use bitstream::reservoir::VbrQuality;
pub use io::PcmBuffer;
