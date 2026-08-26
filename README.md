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
- **Full-featured by design**: Psychoacoustic Model II with look-ahead
  window, CBR/ABR with bit reservoir, complete block switching
  (long/start/short/stop), joint stereo (mid/side), all Huffman
  tables, and both MPEG-1 (32/44.1/48 kHz) and MPEG-2 LSF
  (16/22.05/24 kHz) on the roadmap.

### Compared to existing encoders

**vs LAME (C, LGPL)** — the de-facto quality benchmark:

- **License**: MIT/Apache-2.0 with no LGPL restrictions. Static linking in a
  proprietary product is fine — no object-file relinking requirement, no
  copyleft obligations on the host application.
- **WASM without glue**: compiles directly to `wasm32-unknown-unknown`.
  LAME requires emscripten, a custom build toolchain, and manual FFI
  bindings to get the same result.
- **Zero `unsafe`**: the entire workspace passes `#![deny(unsafe_op_in_unsafe_fn)]`.
  LAME is ~40,000 lines of C with the memory-safety risk that entails.
- **Auditable provenance**: every constant table is verified against ≥2
  independent sources (ISO standard text, dist10 reference, minimp3 CC0).
  Each milestone was independently re-reviewed, bugs found and fixed,
  and the review process is documented in the roadmap.

**vs Shine (C, LGPL, fixed-point arithmetic)** — a deliberately minimal,
CBR-only encoder:

- **Psychoacoustic Model II**: Shine uses a simplified model. encoRust
  implements the full standard model (Bark partitions, asymmetric spreading
  function, tonality/unpredictability measure, dynamic ATH) — audible
  quality at lower bitrates depends on this.
- **Room to grow**: VBR, intensity stereo, cross-frame bit reservoir
  smoothing, and MPEG-2 LSF are explicitly deferred, not structurally
  impossible. The architecture already accommodates them.

**vs any encoder used through FFI bindings** (lame-rs, shine-rs):

- **No build-time C dependencies**: a single `cargo build` produces the
  binary. No libmp3lame-dev, no cmake, no system library hunt.
- **Predictable latency**: the encode hot path is verified allocation-free
  (regression test: `m10_zero_alloc.rs`). FFI boundaries and C allocators
  don't offer that guarantee — critical in an `AudioWorklet` callback
  where a GC pause or `malloc` stall drops audio.

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

### Rebuilding after a code change

`cargo build`/`cargo test` only recompile the library and test binaries —
if you're invoking `target/release/encorust` directly (by path, as in the
examples below), it stays stale until you rebuild the CLI explicitly:

```bash
cargo build --release -p mp3-cli   # binary lands at target/release/encorust
./target/release/encorust -o output.mp3 -b 192 input.wav
```

To sanity-check the result (waveform, spectrogram, peak/RMS levels)
against the original, use `ffmpeg`:

```bash
ffmpeg -i input.wav -af "astats=metadata=1:reset=1,ametadata=print:key=lavfi.astats.Overall.Peak_level" -f null -
ffmpeg -i output.mp3 -af "astats=metadata=1:reset=1,ametadata=print:key=lavfi.astats.Overall.Peak_level" -f null -
```

Decoding with `ffmpeg` (or any standards-compliant decoder, not this
project's own encoder logic) is important when debugging encoder bugs —
it's the only way to tell whether output that sounds wrong is a real
bitstream problem versus something specific to one decoder.

## Status

| Feature | Status |
|---|---|
| CBR encoding (`--bitrate`) | ✅ |
| ABR encoding (`--abr`) | ✅ |
| Psychoacoustic Model II with look-ahead window | ✅ |
| Block switching (long/start/short/stop) | ✅ |
| Joint stereo mid/side (MS) | ✅ |
| VBR encoding (`--vbr-quality`) | ⬜ |
| Intensity stereo | ⬜ |
| Cross-frame bit reservoir (`main_data_begin`) | ⬜ |
| Xing/LAME info header | ⬜ |
| MPEG-2 LSF (16/22.05/24 kHz) | ⬜ |

## Usage

```bash
encorust input.wav -o output.mp3 -b 192        # CBR
encorust input.wav -o output.mp3 --abr 128      # ABR
```

## License

MIT OR Apache-2.0, at your option.