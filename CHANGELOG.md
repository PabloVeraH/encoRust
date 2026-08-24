# Changelog

All notable changes to encoRust will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added
- MPEG-1 Layer III CBR encoding (32/44.1/48 kHz, mono/stereo/dual-mono)
- Psychoacoustic Model II with block-switching state machine
- Inner/outer quantization loop with flat-SMR fallback
- Full Huffman region-splitting and table selection
- Bit reservoir bookkeeping and CBR frame-size padding
- CLI binary (`encorust`) and WASM bridge (`mp3-wasm`)
- Zero-alloc guarantee: `encode_frame` performs no heap allocations after construction
- Symphonia-based differential encode/decode test
- CI workflow (fmt, clippy, test, no_std, WASM, MSRV)

### Changed
- Reduced public API surface: internal modules marked `#[doc(hidden)]`
- Core types marked `#[non_exhaustive]`
- `EncoderConfig::new()` replaces struct-literal construction
- Psychoacoustic model: all working buffers are fixed-size stack arrays

### Removed
- VBR/ABR: now rejected at construction with clear error message (not yet implemented)
- Unused `simd` feature flag

### Fixed
- block_type forced to `BlockType::Long` until short blocks are structurally complete
- LICENSE-MIT / LICENSE-APACHE files added at repo root

## [0.1.0] — Unreleased

Initial pre-release with M0-M9 completion.