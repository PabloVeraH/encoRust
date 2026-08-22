//! The 32 fixed Huffman code tables, region splitting, table selection,
//! and bit emission. See
//! `docs/mp3-encoder/09-phase6-huffman-coding.md`.
//!
//! This module holds the largest block of load-bearing numeric data in
//! the project (thousands of individual variable-length codes) — see
//! `docs/mp3-encoder/09-phase6-huffman-coding.md` §2 for the mandatory
//! sourcing/cross-check process before [`tables`] is populated for real.

pub mod encode;
pub mod tables;

pub use encode::{encode_granule, estimate_bits};
pub use tables::{Count1Table, HuffmanTable, VlcEntry};
