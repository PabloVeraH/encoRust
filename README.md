# encoRust

A pure-Rust MPEG-1/2 Layer III (MP3) encoder — no C dependencies, no FFI —
designed for real-time use and WebAssembly targets.

**What this project actually is**: an experimental, from-scratch lab
effort to reimplement LAME's approach — the psychoacoustic model, the
quantization loop, the bitstream format — natively in Rust, to explore
how far a memory-safe, dependency-free encoder can get against a
decades-old, heavily tuned C reference. It's a research project, not
(yet) a polished, production-ready alternative to LAME. See
[Status](#status) below for what's actually implemented versus still in
progress, and `docs/investigation-log.md` for the ongoing investigation into known
audio-quality gaps.

## Why

The Rust ecosystem has several pure-Rust MP3 *decoders* (Symphonia,
puremp3, nanomp3), but every MP3 *encoder* available today is an FFI
binding to a C library (LAME, Shine). encoRust fills that gap with a
codec written natively in Rust:

- **Real-time friendly — a design goal, partially verified**: the encode
  path targets zero heap allocations after construction, for predictable
  worst-case latency in live audio (e.g. encoding a microphone feed). A
  counting-allocator regression test
  (`crates/mp3-core/tests/m10_zero_alloc.rs`) confirms this holds for the
  narrowest configuration it exercises — mono, CBR, long-block-only
  content — but does **not** yet cover stereo/joint-stereo, ABR/VBR, or
  transient (short-block) encoding. Read "zero allocations" as verified
  for that one path today, not as a blanket guarantee across every mode.
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
- **Predictable latency — for the one path this is actually verified
  on today**: the encode hot path is allocation-free in the mono/CBR/
  long-block configuration `m10_zero_alloc.rs` exercises. FFI boundaries
  and C allocators don't offer even that much of a guarantee — critical
  in an `AudioWorklet` callback where a GC pause or `malloc` stall drops
  audio — but encoRust hasn't yet proven the same holds for stereo,
  ABR/VBR, or short-block paths.

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

## Other Rust Audio Codecs

MP3 isn't always the right codec for the job. If you're looking at this
project but actually need something else, these are worth a look:

- [`opus-pure`](https://docs.rs/opus-pure/latest/opus_pure/) — a
  pure-Rust Opus codec (RFC 6716), encoder and decoder, no C
  dependencies.
- [`opus`](https://docs.rs/opus/latest/opus/) — high-level Rust bindings
  to libopus (the C reference implementation), encoder and decoder.
- [`fdk-aac-sys`](https://docs.rs/fdk-aac-sys/latest/fdk_aac_sys/) — raw
  FFI bindings to Fraunhofer's FDK AAC library, encoder and decoder.

## License

MIT OR Apache-2.0, at your option.