# encoRust

A pure-Rust MPEG-1/2 Layer III (MP3) encoder — no C dependencies, no FFI —
designed for real-time use and WebAssembly targets.

## Why

The Rust ecosystem has several pure-Rust MP3 *decoders* (Symphonia,
puremp3, nanomp3), but every MP3 *encoder* available today is an FFI
binding to a C library (LAME, Shine). encoRust fills that gap with a
codec written natively in Rust:

- **Real-time friendly**: the encode path is allocation-free after
  construction (enforced by a counting-allocator regression test — see
  `crates/mp3-core/tests/m10_zero_alloc.rs`), with predictable
  worst-case latency — suitable for live audio (e.g. encoding a
  microphone feed).
- **WASM as a first-class target**: the core crate is `no_std`-friendly
  and compiles to `wasm32-unknown-unknown`, so the same encoder runs on a
  server or inside a browser `AudioWorklet`.
- **Full-featured by design**: Psychoacoustic Model II, CBR/ABR/VBR with
  bit reservoir, complete block switching (long/start/short/stop/mixed),
  joint stereo (mid/side + intensity), all Huffman tables, and both
  MPEG-1 (32/44.1/48 kHz) and MPEG-2 LSF (16/22.05/24 kHz).

MP3 is patent-free worldwide since 2017. This project is written from the
ISO/IEC 11172-3 / 13818-3 specifications and independent research.

## Workspace layout

```
encoRust/
└── crates/
    ├── mp3-core/   # the codec — no I/O, no_std-friendly (alloc only)
    ├── mp3-cli/    # `encorust` binary: WAV in -> MP3 out
    └── mp3-wasm/   # wasm-bindgen bridge for browser / Node real-time use
```

`mp3-core` never touches the filesystem or the network; file handling
lives in `mp3-cli` and JS interop in `mp3-wasm`, so the codec core stays
embeddable anywhere.

## Building

```bash
# Native build + tests + lints
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings

# Core crate for WebAssembly (no_std check)
cargo build -p mp3-core --target wasm32-unknown-unknown --no-default-features
```

CI (`.github/workflows/ci.yml`) runs all of the above plus a
`--no-default-features` test pass and an MSRV check on every push.

MSRV: 1.82 (see `rust-toolchain.toml`).

## Usage

```bash
encorust input.wav -o output.mp3 -b 192        # CBR (VBR/ABR coming soon)
```

## License

MIT OR Apache-2.0, at your option.