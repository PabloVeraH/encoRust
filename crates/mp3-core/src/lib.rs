//! `mp3-core`: a pure-Rust MPEG-1/2 Layer III (MP3) encoder.
//!
//! This crate contains **only** the codec. It performs no file I/O and no
//! JS interop — see `mp3-cli` and `mp3-wasm` for those. It is
//! `no_std`-friendly (build with `--no-default-features`) so it can run
//! inside a `wasm32-unknown-unknown` real-time audio callback.
//!
//! # Where to start
//!
//! This crate is a scaffold: every DSP-heavy function is a documented
//! `todo!()`. The build guide lives in `docs/mp3-encoder/` at the
//! repository root — start at `docs/mp3-encoder/00-overview.md`, then
//! follow `docs/mp3-encoder/14-roadmap-and-milestones.md` for current
//! status and implementation order. Each module below corresponds to
//! exactly one section of exactly one phase chapter; see that module's
//! doc comment for the chapter reference.
#![cfg_attr(not(feature = "std"), no_std)]
#![warn(missing_docs)]
#![warn(clippy::all)]
#![deny(unsafe_op_in_unsafe_fn)]

extern crate alloc;

pub mod bitstream;
mod encoder;
pub mod error;
pub mod filterbank;
pub mod frame;
pub mod huffman;
pub mod io;
pub mod mdct;
pub mod psychoacoustic;
pub mod quantize;
pub mod types;

pub use encoder::{Encoder, EncoderConfig};
pub use error::EncodeError;
pub use types::{Bitrate, ChannelMode, MpegVersion, SampleRate};
