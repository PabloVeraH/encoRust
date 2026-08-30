# Improvement Plan — encoRust

**Status of this document**: point-in-time review, 2026-08-23. Produced by
three parallel specialized reviews (Rust idioms/API design, memory/
performance with empirical measurement, software architecture) plus
external market research on competing encoders. All file:line references
were verified against the codebase at commit `1b24824` (branch
`fix/m2-polyphase-impulse-test`) / `2144fa0` (`main`). Re-verify line
numbers if this document is read long after that commit.

**Relationship to the roadmap**: [14-roadmap-and-milestones.md](mp3-encoder/14-roadmap-and-milestones.md)
tracks *whether the spec is implemented* (M0–M9, all done). This document
tracks *whether the implementation is as good as it should be* — memory
efficiency, Rust idiom quality, and project engineering — plus a small
number of correctness bugs this review surfaced that the roadmap's own
"known deviations" list did not yet capture. Treat Phase 0 below as
higher priority than anything in this document's later phases, and
arguably higher priority than the roadmap's own remaining feature work
(joint stereo, LSF).

---

## 1. Did the plan get fulfilled?

**Yes, structurally.** M0 through M9 are all marked done in the roadmap,
each gated by its own test suite, and the project's review culture (every
milestone re-audited, bugs found and fixed at nearly every stage) is
genuinely strong — better than most solo/small-team Rust projects get.
Zero `unsafe` in the whole workspace, a clean `no_std`-compatible core,
minimal dependencies (`libm` + `thiserror` in production), and an
unusually thorough documentation trail (the roadmap's revision log reads
like a lab notebook, not marketing copy) are real assets worth protecting
through everything below.

**But "done" already carries five self-disclosed deviations** (roadmap
rows M7–M9): no cross-frame bit reservoir, no joint stereo, no MPEG-2
LSF, no real VBR/ABR bitrate-index logic, no SIMD, no external-decoder
validation. This review found **two more that were not on that list**:

- A **real correctness bug** (§2.1) that can produce an undecodable
  bitstream on transient audio — not a documented scope cut, an actual
  defect the existing test suite doesn't catch because it doesn't
  cross-check bitstream coherence.
- **VBR/ABR are silent no-ops** (§2.2) — the CLI accepts `--vbr-quality`
  and produces bytes byte-identical to fixed 128 kbps CBR, with no
  warning.

Neither invalidates the milestone work. Both mean "M0–M9 done" should not
be read as "ready to publish as a library other people build on" without
this document's Phase 0.

---

## 2. Critical findings (fix before anything else)

### 2.1 — `block_type` is inconsistent across pipeline stages — can emit an undecodable frame

**Severity: CRITICAL — correctness, not style.**

The psychoacoustic model can decide a granule is `Start`/`Short`/`Stop`
(state machine in `psychoacoustic/model2.rs:391-435`), and that decision
is *partially* honored downstream:

| Stage | Uses the decided `block_type`? | Location |
|---|---|---|
| MDCT | **No — hardcoded to `BlockType::Long`** | `encoder.rs:318-322` |
| Quantization (band map) | Yes | `encoder.rs:344` → `quantize_granule` |
| Scalefactor encoding | Yes | `encoder.rs:353-356` |
| Huffman encoding | **Doesn't even receive the parameter** | `huffman/encode.rs:301-304`, called at `encoder.rs:364` |
| Side info | Yes — writes `window_switching_flag`, `block_type`, `subblock_gain`, and a 2-table-select layout | `encoder.rs:447` → `bitstream/side_info.rs:99-120` |

On any granule where the model detects a transient, the frame's side info
declares short blocks (different bit layout: 2 `table_select` fields
instead of 3, no `region0/1_count`) while `main_data` was Huffman-encoded
as if it were a long block over a spectrum that was MDCT'd as a long
block. A real decoder desyncs from that granule onward. `mdct::
window_for_type(BlockType::Short)` (`mdct/mod.rs:121-128`) `panic!`s —
a public-reachable leaf function panicking on a valid enum variant is the
symptom of an enum with a state that doesn't belong in that function.

The existing test `main_data_begin_never_references_untransmitted_bytes`
(`tests/m8_bitstream.rs:288-325`) uses "quiet-then-loud" content
specifically to exercise reservoir edge cases — content shaped exactly
like what would also trigger a transient/short-block decision — but its
parser (`m8_bitstream.rs:273-278`) accepts either branch without checking
`main_data` coherence against the declared layout. It is very likely
already exercising this bug without detecting it.

**Fix, in two steps:**

1. **Containment now** (same pattern already used for LSF/joint stereo):
   force `BlockType::Long` at `quantize_granule`, `encode_granule_
   scalefactors`, and `gi.block_type`, with a `debug_assert!` and a
   comment explaining why — i.e., make the psychoacoustic model's
   transient decision *advisory-only* until short blocks are fully wired.
   Emitting a bitstream that's silently wrong is strictly worse than not
   supporting short blocks yet.
2. **Structural fix**: introduce one type that every stage derives its
   layout from, instead of passing a loose `BlockType` to four places and
   hoping they agree:

   ```rust
   pub(crate) enum GranuleShape {
       Long { window: LongWindowKind },   // Long | Start | Stop
       Short { mixed: bool },             // 3 × 6-line windows
   }
   ```

   `mdct::transform_*`, `quantize_granule`, `encode_granule_
   scalefactors`, `huffman::encode_granule`, and `SideInfo::write` all
   take the same `GranuleShape` and derive their layout from it —
   forgetting a stage becomes a compile error, not a corrupted bitstream.
3. Add a coherence test that parses the emitted frame and checks the
   side-info-declared layout matches the number of regions/tables
   actually present in `main_data`.

**DoD**: no code path can emit side info declaring a different block
shape than what MDCT/quantize/Huffman actually produced; `window_for_type`
no longer has a reachable panic path; a coherence test exists and passes.

### 2.2 — VBR and ABR are silent no-ops

**Severity: CRITICAL for user trust — the CLI advertises a feature that doesn't exist.**

- `bitstream/reservoir.rs:35-40`: `RateControl::Vbr(_) => Bitrate::
  Kbps128` — VBR *is* fixed 128 kbps CBR, unconditionally.
- `RateControl::Abr` is never distinguished from `Cbr` anywhere in the
  workspace (grep confirms zero discriminating uses).
- `VbrQuality(q)` is **never read anywhere in the workspace**.
  `encorust --vbr-quality 0` and `--vbr-quality 9` produce byte-identical
  output, and both are byte-identical to `--bitrate 128`.

This is worse than "feature not implemented" — it's a feature that
*appears* to work (the flag is accepted, encoding succeeds, output plays)
while silently ignoring the user's request. Real VBR/ABR logic is
legitimate future work (§4, Phase 6), but until it exists:

**Fix now**: reject `RateControl::Vbr`/`Abr` in `Encoder::new` with a
typed error, the same pattern already used for LSF and joint stereo
(`EncodeError::UnsupportedRateControl` or similar). Update the CLI's
`--vbr-quality`/`--abr` flags to surface that error instead of silently
downgrading to CBR.

### 2.3 — Huffman table provenance unresolved + no LICENSE files in the repo

**Severity: CRITICAL — legal/publication blocker, not a code defect.**

`huffman/tables.rs`'s own header comment admits: cross-checking this
~6,000-line data block was done against FFmpeg's `mpegaudiodectab.h`
(LGPL-2.1+) *first* and minimp3 (CC0) *second* — the reverse of the
provenance procedure `docs/mp3-encoder/09-phase6-huffman-coding.md`
itself specifies, and left "not resolved as part of this pass." Separately,
**no `LICENSE`, `LICENSE-MIT`, or `LICENSE-APACHE` file exists anywhere in
the repository**, despite all three `Cargo.toml`s declaring
`license = "MIT OR Apache-2.0"`.

**Fix**: re-verify the Huffman table provenance against minimp3 (CC0) or
the standard text directly, with FFmpeg as the secondary cross-check
(matching chapter 09's own stated order), and document the final
provenance per table. Add the actual `LICENSE-MIT`/`LICENSE-APACHE` files
at the repo root. Neither of these blocks continued development, but both
block any public release/crates.io publication, which is where this
project's whole value proposition ("cleanly MIT/Apache-2.0, no LGPL
question hanging over it" — `02-standards-and-prior-art.md`) is actually
tested.

### 2.4 — The "encode_frame never allocates" guarantee is false and unverified

**Severity: CRITICAL for the project's stated real-time/embedded value proposition.**

Covered in full in §3 (it's the largest single opportunity in this
document), but stated here because it's also a documentation-accuracy
bug: `docs/mp3-encoder/01-architecture.md:107-109` and `encoder.rs:51-52`
both assert `encode_frame` is allocation-free "asserted by a test." No
such test exists (`tests/smoke.rs` only checks public types are
reachable). The claim is false — see §3 for measured allocation counts.
**Fix**: either implement §3's recommendations and add the regression
test the documentation already claims exists, or correct the
documentation to stop promising something the code doesn't deliver. Given
this project's own stated engineering values, the first option is the
right one — but until it's done, the doc comment should not claim it.

---

## 3. Memory — eliminating hot-path allocation (the core ask)

This is the section most directly aimed at "que este proyecto se mejore
en torno al uso de memoria." The numbers below are **measured**, not
estimated — a counting `GlobalAlloc` wrapper was run against the real
encode path (methodology in the box at the end of this section).

### 3.1 — Current state, quantified

| Scenario | `alloc()`/frame | heap ops/frame | bytes touched/frame |
|---|---|---|---|
| Mono, 44.1 kHz / 128 kbps | 39 | ~53.2 | ~32.0 KB |
| Stereo, 44.1 kHz / 128 kbps | 75 | ~99.75 | ~63.6 KB |

At 44.1 kHz (38.28 frames/s), that's **~2,036 heap ops/s mono, ~3,819
heap ops/s stereo** — inside the hot path of an encoder whose own design
doc promises suitability for a WASM `AudioWorklet` real-time callback,
which is precisely the environment where unpredictable allocation
latency is most punishing.

**Where it comes from:**

1. **`PsychoacousticModel::analyze_granule` — 16 `alloc()` per call, ~15.3
   KB/call, 0% of it necessary.** Called once per (granule, channel) — 4×
   per stereo frame — so **64 of the 75 stereo allocations (85%) come
   from this one function.** Breakdown (`psychoacoustic/model2.rs`):
   - `fft_analyze_long` (293-312): `re`, `im`, `mag` — 3 Vecs × 513 f32.
   - `compute_tonality` (317): 1 Vec × 513 f32.
   - `compute_partition_map` (`tables.rs:152`), `partition_bark_centers`
     (`tables.rs:179-180`), `partition_hz_centers` (`tables.rs:210-211`):
     5 Vecs, **and this is the single highest-value fix in this entire
     document**: these three functions' output depends *only* on
     `sample_rate_hz`, which never changes after `Encoder::new()`. Only 6
     sample rates exist (`SFB_LONG_COUNTS: [usize; 6]`). This is
     recomputed — including a `libm::atan()`-based Bark conversion for
     every one of 513 FFT bins — from scratch on every single call,
     thousands of times per second, for a result that is 100% constant
     for the lifetime of the `Encoder`.
   - Steps 4-6 (182-213): `part_energy`, `part_tonality`, `part_tonality_
     count`, `part_threshold`, `part_smr` — 5 Vecs, each ~48-64 elements.
   - `compute_perceptual_entropy` (358-359): 2 more Vecs.

2. **`Encoder::encode_frame` — 3 + 2×(granules × channels) Vecs per
   frame** (7/frame mono, 11/frame stereo): `new_main_data`
   (`encoder.rs:273`), `sf_buf` (348), `granule_buf` (361, both inside
   the per-granule/channel closure, doubled if the non-convergence
   fallback at 407-418 fires), `vec![0u8; frame_bytes]` (483, a known
   exact size), `si_buf` (495).

3. Everything else in the pipeline — `filterbank/polyphase.rs`, `mdct/
   mod.rs`, `quantize/loop_control.rs`, `huffman/encode.rs`, `bitstream/
   scalefactor_encode.rs`, `bitstream/side_info.rs`, `bitstream/
   reservoir.rs` — was verified **already allocation-free** on the real
   encode path (every `Vec::new()`/`vec![]` found there is inside
   `#[cfg(test)]`). `quantize/loop_control.rs` in particular already uses
   fixed-size arrays throughout despite running its inner/outer loop up
   to 512 times per (granule, channel) in the worst case — this module is
   the model to imitate for the fixes below, not a fix target itself.

4. **`mp3-wasm`** adds its own allocations on top, in exactly the one
   deployment target (`AudioWorklet`) where allocation latency is most
   costly: `crates/mp3-wasm/src/lib.rs:124, 127, 136, 161, 168, 172` — a
   fresh `Vec::new()` output buffer per `push()`/`finish()` call, and
   `self.pending.drain(..frame_samples).collect()` (line 127) allocating
   a new `Vec<f32>` per completed frame when `PcmBuffer::
   from_f32_interleaved` only needs a `&[f32]` slice.

### 3.2 — Fix plan

| # | Change | File(s) | Eliminates |
|---|---|---|---|
| M-1 | Move `compute_partition_map`/`partition_bark_centers`/`partition_hz_centers` out of `analyze_granule` into `PsychoacousticModel::new(sample_rate_hz)` (or `Encoder::new`), cached as struct fields (`partition_of_bin: [u8; 513]` — `u8` not `usize`, values never exceed the existing `[false; 64]` bound at `model2.rs:259` — `part_centers_bark`/`part_centers_hz: [f32; 64]`, `num_partitions: usize`) | `psychoacoustic/model2.rs`, `tables.rs` | 5 of 16 Vecs/call, and the redundant per-bin `atan()` work |
| M-2 | Convert the remaining 11 Vecs in `analyze_granule`/`fft_analyze_long`/`compute_tonality`/`compute_perceptual_entropy` to fixed-size arrays (`[f32; 513]`, `[f32; 64]`) — all sizes are compile-time constants; 513×4 + a handful of 64×4 buffers is ~2.3 KB, trivially stack-sized | `psychoacoustic/model2.rs` | Remaining 11 Vecs/call → **0 allocations in the psychoacoustic model** |
| M-3 | Give `Encoder` pre-allocated, reused-in-place buffers for `new_main_data`, `sf_buf`, `granule_buf`, `si_buf`, `frame_buf` — fixed capacity is computable (`frame_bytes ≤ 1440` bytes for any legal MPEG-1 CBR configuration: `144 × 320,000 / 32,000` bound), so `heapless::Vec<u8, N>` (`Vec`-compatible API, inline storage, zero heap) or a raw `[u8; N]` + length works; `.clear()` between frames instead of reallocating | `encoder.rs:273, 348, 361, 483, 495` | 7-11 Vecs/frame → **0** |
| M-4 | Hoist the MDCT window lookup out of the per-subband loop — `window_for_type(block_type)` is called 32×/(granule,channel) in `encoder.rs:317-327` but only depends on `block_type`, which is constant across all 32 subbands of a granule. Better: since only 4 window shapes exist and are purely formulaic, precompute them as `const` arrays the same way `ANALYSIS_PROTOTYPE_FILTER` already is. | `encoder.rs:317-327`, `mdct/mod.rs:51-128` | Up to 4,608 redundant `libm::sinf` calls/frame — not a heap-allocation fix, but the same "recompute a constant on every call" class of bug as M-1, and cheap to fix in the same pass |
| M-5 | Reuse buffers in `mp3-wasm`'s `WasmEncoder` (`output`, frame scratch) across `push()` calls instead of allocating fresh each time; replace `.drain(..).collect()` with building `PcmBuffer` directly from `&self.pending[..frame_samples]`, then `drain` without `.collect()` | `mp3-wasm/src/lib.rs:124-172` | Per-call allocations in the one target where they cost the most |
| M-6 | Write the regression test the architecture doc already claims exists: a `#[cfg(test)]` counting `GlobalAlloc` wrapper asserting `encode_frame` performs zero allocations after the first warm-up call | new test in `mp3-core/tests/` | Locks in M-1 through M-4 against regression |

**Net effect if M-1 through M-4 land**: from ~75 allocations/stereo-frame
to **0** on the steady-state encode path — an actual "allocation-free
after construction" guarantee instead of a documented-but-false one.

### 3.3 — Secondary memory findings

- **`QuantizationResult` is `Copy`, ~2.9 KB, and gets copied by value
  repeatedly**: embedded in `GranuleSideInfo` (`bitstream/
  side_info.rs:44`), which lives 4× inside `SideInfo` (`side_info.rs:25`,
  ~11.8 KB total). `encoder.rs:275-278` copies it 4× at initialization
  and again at `encoder.rs:450` per granule/channel — for data that side
  info doesn't actually serialize (the ~20 bytes/granule that *is*
  serialized don't need the full quantized spectrum riding along).
  **Fix**: split `GranuleSideInfo` to carry only the serialized fields;
  keep `QuantizationResult` local to the encoding closure (this also sets
  up the `GranuleCoder` decomposition in §5).
- **`Encoder` always reserves `MAX_CHANNELS` of everything** (`encoder.
  rs:87-93`, ~40 KB of state) even in mono, where half goes unused.
  Low priority, but cheap to note for anyone touching that struct.
- `partition_of_bin: Vec<usize>` uses 8 bytes/element where values never
  exceed ~64 — folded into M-1/M-2 above (`u8` instead of `usize`).

### 3.4 — Build-time memory settings

`Cargo.toml`'s `[profile.release]` (`opt-level=3`, `lto="fat"`,
`codegen-units=1`) is a good baseline but is missing:

- `panic = "abort"` for `mp3-cli`/`mp3-wasm` (neither is a library another
  process `catch_unwind`s) — smaller binary, no unwind tables.
- `strip = true` — the native binary was confirmed "not stripped" (897
  KB on disk; `.text` alone is 598 KB, 67% of the binary — the Huffman
  tables, confirmed via `nm -S` to live in `.rodata` at 56.3 KB, are
  **not** the size driver; `lto="fat"` + `codegen-units=1`'s aggressive
  inlining is).
- No `.cargo/config.toml` with `target-cpu` tuning for native (non-WASM)
  builds — LLVM auto-vectorizes more aggressively above the default
  SSE2 baseline without requiring any explicit SIMD code.
- No evaluation of `mimalloc`/`jemalloc` as `#[global_allocator]` in
  `mp3-cli` — genuinely useful given §3.1's allocation pattern (many
  small, short-lived allocations), but this is **mitigation, not a fix**:
  it should be a defense-in-depth addition *after* M-1 through M-4, never
  a substitute for them.

**Measured WASM footprint** (already good, preserve it): 120 KB raw, 44
KB gzip'd, under the project's own 150 KB budget.

<details>
<summary>Measurement methodology (for reproducibility)</summary>

A temporary harness (not committed, scratchpad-only) defined a counting
`GlobalAlloc` wrapper delegating to `System`, built all `PcmBuffer` test
inputs *outside* the measured region, ran one warm-up frame (excluded),
then measured 199 steady-state frames. `PsychoacousticModel::
analyze_granule` was additionally isolated by calling it directly,
outside `Encoder`, to attribute its allocations separately from
`encoder.rs`'s own. `alloc()` counts are exact and reproducible (fixed
struct sizes); `realloc()` counts vary slightly with audio content
(`Vec`'s amortized growth depends on how large `new_main_data`/`sf_buf`/
`granule_buf` end up before hitting their final size).
</details>

---

## 4. Performance — SIMD and the real-time-factor margin

### 4.1 — Current baseline (measured, not projected)

Via `cargo bench -p mp3-core --bench realtime_factor` (Intel Core Ultra 5
125H, release profile as configured today):

| Scenario | Time (5 s audio) | Real-time factor |
|---|---|---|
| Stereo, 128 kbps | 669.65 ms (623.86–726.87 ms CI) | **≈7.47×** |
| Mono, 128 kbps | 307.06 ms (305.24–309.34 ms CI) | **≈16.3×** |

The project's own DoD (`12-phase9-cli-and-wasm.md`) sets a ≥4× scalar
floor and a ≥15× SIMD target. **Stereo has only ~1.87× of margin over the
floor** — margin that can evaporate on weaker hardware (mobile, a
sandboxed browser WASM runtime) before SIMD exists to compensate. This is
a direct risk to the "beat LAME/Shine/other Rust encoders" goal, and it's
also good news: §3's allocation fixes and the M-4 windowing fix are pure
overhead removal (not algorithmic changes), so they're the fastest path
to widening this margin *before* touching SIMD at all — re-benchmark
after each of M-1 through M-4 lands.

### 4.2 — SIMD candidates, ranked by expected ROI

1. **`quantize_spectrum`/`dequantize_spectrum`** (`quantize/
   loop_control.rs:92-114`) — highest call count in the whole pipeline:
   up to `MAX_INNER_ITERATIONS × MAX_OUTER_ITERATIONS` = 64 × 8 = 512
   calls per (granule, channel) in the worst case, each processing all
   576 spectral lines independently (no cross-line dependency — ideal for
   `f32x8`/`f32x16`). Already allocation-free, purely a FLOPs problem.
   **Do this first.**
2. **Polyphase filterbank** (`filterbank/polyphase.rs:58-83`) — the
   classic FIR/cosine-matrix multiply-accumulate pattern LAME itself
   vectorizes (`dct64`), called 18×/(granule, channel).
3. **Psychoacoustic FFT** (`psychoacoustic/fft.rs:91-141`, radix-2,
   1024-pt) — standard SIMD target, but runs only once/(granule,channel)
   so lower total share of runtime than (1)/(2).
4. **MDCT** (`mdct/mod.rs:137-164`) — currently O(N²) by construction
   (36×18, 12×6 multiplies). An FFT-based fast MDCT would likely beat
   vectorizing the current direct form — this is an algorithmic change,
   not a SIMD one; worth a separate design note before M9's SIMD work
   reaches this stage.

**Rough estimate** (needs real validation once implemented, not a
commitment): (1) and (2) together could plausibly deliver 3-6× over the
current scalar RTF, i.e. the measured ~7.47× stereo could land in the
~25-40× range — comfortably past the ≥15× DoD target.

### 4.3 — Which SIMD approach

External research for this review confirms: `std::simd` remains
nightly-only with no stabilization timeline, which rules it out for a
project targeting stable Rust and WASM. The `wide` crate is the right
choice here — stable Rust, covers x86 (SSE/AVX), NEON, and WASM SIMD128
in one portable API, and is exactly the kind of small, focused dependency
that fits this crate's stated "stay close to dependency-free" philosophy
better than a larger framework. (`pulp`/`macerator` are reasonable
alternatives if multiversioning across CPU features at runtime becomes a
requirement later; not needed for v1 of this work.)

**Whatever SIMD path is chosen, the DoD must include a byte-identical
equivalence test between the SIMD and scalar code paths** on the same
fixtures — this is the only real defense against SIMD introducing subtle
numerical divergence that a human reviewer won't catch by inspection, and
it means the scalar path must remain compiled and callable (a runtime
fallback, not a feature-gated deletion) so the test has something to
compare against.

---

## 5. Architecture — preparing for a stable 1.0 API

### 5.1 — Public API surface is far broader than intended, and freezes exactly what's about to change

`lib.rs:24-34` exposes all nine internal modules as `pub mod`, despite
`01-architecture.md`'s own stated intent ("public API: `Encoder`,
`EncoderConfig`, `encode_frame()`"). Auditing actual usage: `mp3-cli` and
`mp3-wasm` together touch only `Encoder`, `EncoderConfig`, `Bitrate`,
`ChannelMode`, `MpegVersion`, `SampleRate`, `RateControl`, `VbrQuality`,
`PcmBuffer`, and `EncodeError`. Everything else — `filterbank`, `frame`,
`huffman`, `mdct`, `psychoacoustic`, most of `quantize`, and `bitstream::
{writer, side_info, scalefactor_encode}` — is public with zero external
consumers, including internal-only helpers like `psychoacoustic::
compute_partition_map` (only ever called from within `psychoacoustic/
model2.rs` itself).

This matters more than a typical "reduce API surface" cleanup because
**every item on that list is exactly what the remaining roadmap work
needs to change**: SIMD changes `PolyphaseFilterbank::analyze`'s
signature; LSF changes `SideInfo`'s layout; short blocks change
`huffman::encode_granule`'s signature. Publishing 1.0 with today's
surface means each of those becomes a breaking change instead of an
internal refactor.

**Fix**: reduce to `pub(crate)` everywhere except `Encoder`,
`EncoderConfig`, `EncodeError`, `types::*`, `io::PcmBuffer`, `RateControl`
(relocated — see below), `VbrQuality`. If integration tests need internal
access, use `#[doc(hidden)] pub mod internals` with an explicit
"exempt from semver" note, not blanket `pub mod`.

Related, cheap, and should happen in the same pass: **`#[non_exhaustive]`
is applied inconsistently** — only `Bitrate` (`types.rs:151`) has it.
`EncoderConfig`, `EncodeError`, `ChannelMode`, `SampleRate`, `MpegVersion`,
`RateControl` don't, and at least `EncoderConfig`/`EncodeError` are
certain to grow fields/variants as VBR/ABR, joint stereo, and LSF land.

**Relocate `RateControl`/`VbrQuality`** out of `bitstream::reservoir`
(`reservoir.rs:16-48`) into `types.rs` or a new `config` module — a
user-facing configuration type living inside the bitstream multiplexer's
internals is a bounded-context leak (the CLI today imports it as
`mp3_core::bitstream::reservoir::VbrQuality`, exposing an implementation
detail as the import path).

### 5.2 — `encode_frame` is a 345-line monolith — decompose it, but not with a generic `trait Stage`

`encoder.rs:173-517` does everything: validation, padding decision,
header construction, frame-size math, budget policy, the full per-
granule/channel DSP pipeline, the non-convergence fallback, side-info
assembly, and output serialization, all in one function. This isn't just
a style problem — it's exactly why §2.1's bug could exist undetected:
there's no unit boundary where "did every stage agree on this granule's
shape" could be tested in isolation.

**Explicit recommendation on the shape of the decomposition**: do not
introduce a `trait Stage { fn process(...) }`. The stages have
genuinely heterogeneous input/output types (`[[f32;18];32]`, `[f32;576]`,
`ScalefactorBandSmr`, `QuantizationResult`), per-channel state, and rely
on aggressive inlining for the current real-time factor — a uniform
trait would force either generics that buy nothing (nobody swaps the
MDCT implementation at runtime) or `dyn`/`Box`, which under `no_std`
means heap allocation and lost inlining, directly undermining §3's work.
Prefer concrete structs with explicit contracts:

```rust
Encoder::encode_frame(pcm, out)
  ├─ self.rate_control.plan_frame(...)          -> FramePlan { header, frame_bytes, budget }
  ├─ self.analyze(pcm)                          -> [ChannelGranuleAnalysis; N]  (filterbank+MDCT+psy)
  ├─ (future) stereo::joint_transform(&mut analyses)
  ├─ self.coder.code_granule(analysis, budget)  -> CodedGranule { side_info, bytes }
  └─ self.assembler.push_frame(plan, coded)     -> emits now, or defers (§5.3)
```

This also has a measurable quality payoff, not just a readability one:
today `MAX_SCALEFACTOR_BITS_PER_GRANULE = 156` bits (`encoder.rs:35`) is
*always* reserved because the budget is fixed before the real scalefactor
cost is known. At 64 kbps mono (748 bits/granule), that's ~21% of the
budget reserved against a typical actual cost of 70-80 bits. A
`GranuleCoder` that encodes scalefactors first and hands the *real*
remainder to Huffman recovers roughly 10% of effective bitrate at low
bitrates — a genuine quality win, not just cleanup.

### 5.3 — The `encode_frame` signature does not need to change for the future cross-frame reservoir

`pub fn encode_frame(&mut self, pcm: &PcmBuffer, out: &mut Vec<u8>) ->
Result<usize, EncodeError>` already supports "this call emits nothing"
(`Ok(0)`), and `finish()` already exists to flush. Both current consumers
tolerate it today. What **does** need to happen now, before more callers
depend on "one call = one frame":

1. Document the real contract: `encode_frame` *may* return 0 bytes;
   `finish()` *must* be called exactly once. Today's doc comment doesn't
   say this, and `finish()` (`encoder.rs:528-535`) is a silent no-op.
2. Introduce the deferred-output structure now, even as a pass-through:
   a `FrameAssembler { pending: Option<PendingFrame>, main_data_queue:
   Vec<u8> }` (which itself should use §3's fixed-capacity buffer
   approach, not a fresh `Vec`). Doing this as a small, contained change
   now is materially cheaper than doing it as a "everything in
   `encode_frame` changes at once" refactor later.
3. Add a `finished: bool` guard — today `finish()` can be called twice,
   or `encode_frame` called after `finish()`, silently. Once buffering is
   real, that produces corrupted output instead of a harmless no-op. Add
   `EncodeError::AlreadyFinished` (which is also a reason `EncodeError`
   needs `#[non_exhaustive]` now, per §5.1).
4. **Decide the Xing/Info header question now**, because it constrains
   the API: a real VBR stream needs an initial frame with duration/seek
   metadata, and with `out: &mut Vec<u8>` handed in per-call, `finish()`
   cannot go back and patch bytes already returned to the caller in a
   previous call. Either add `Encoder::reserve_header_frame(&mut self,
   out)` (placeholder now, `finish()` returns the real bytes + offset to
   patch) or explicitly document this as the caller's responsibility.
   LAME writes this header; Shine doesn't — matching or exceeding LAME
   here is part of this project's own stated bar.

### 5.4 — The psychoacoustic look-ahead window is documented but doesn't exist

`01-architecture.md:69-73` and `model2.rs:152-155`'s own doc comment
state the model needs a window *larger than one granule, with
look-ahead*. In reality, `encoder.rs:330-332` zero-pads the 1024-sample
analysis window with the current 576-sample granule plus silence — there
is no look-ahead, ever. `io/pcm.rs:33-36` explicitly documents "the ring
buffer that crosses frame boundaries lives in `Encoder`, not here" — but
`Encoder` (`encoder.rs:85-96`) has no such buffer. This degrades tonality
estimation and transient detection (which feeds directly into §2.1's
bug) on every single frame, silently.

**Fix**: add `pcm_history: [[f32; LOOKAHEAD_SAMPLES]; MAX_CHANNELS]` to
`Encoder`, feed the model `[history | current granule]`. This is also the
natural place to implement the one-frame output delay §5.3 needs — one
buffer, two purposes.

### 5.5 — Type design: two illegal states worth closing before 1.0

| Illegal state | Where it fails today | Fix |
|---|---|---|
| `ScalefactorBandSmr { bands: [f32; 22] }` structurally cannot represent short-block SMR (would need 12 bands × 3 windows = 36 slots) | Never fails loudly — `encoder.rs:408` just substitutes a flat `[1.0; 22]` | This is the type-level reason short blocks can't be finished without a breaking change — fix as part of §2.1's structural work |
| `ChannelMode::JointStereoMs`/`Intensity` model joint stereo as a *stream-level* config, but real MP3 `mode_extension` (MS on/off) is a *per-frame* decision based on that frame's L/R correlation | `types.rs:294-341` derives `mode_extension` statically from the stream config | Collapse to `ChannelMode::JointStereo` as user intent; add a per-frame `mode_extension: ModeExtension` decided by the stereo stage — and note this requires restructuring `encode_frame`'s loop from `for granule { for channel { full pipeline } }` to `analyze all channels → joint-transform → code all channels`, which is cheap to do now (before joint stereo exists) and expensive to retrofit later |
| `EncoderConfig` (sample_rate + bitrate) can express e.g. 44.1 kHz + 144 kbps (LSF-only bitrate on an MPEG-1 rate) | Runtime-only, in `Encoder::new` (`encoder.rs:135-140`) — already correctly rejected, just not preventable at the type level | Lower priority than the two above; a `Mpeg1Config`/`LsfConfig` split (or a validating constructor) would move this from a runtime `Result` to unrepresentable, but the current runtime check is at least correct and tested |

### 5.6 — Should encoRust adopt a `symphonia`-style trait-based encoder registry?

**No — explicit recommendation, not a trade-off to weigh.** Symphonia's
registry exists because a *decoder* must dispatch across N codecs
discovered by probing an unknown container at runtime. An *encoder*'s
codec is chosen by the caller at compile time. Keep `Encoder` concrete
and monomorphic. The decisive argument is one-directional compatibility:
adding a trait later (`impl AudioEncoder for Encoder`, or a thin adapter
crate) never breaks anything; removing generics or `dyn` after
publishing does. When in doubt, prefer the option that doesn't foreclose
the other one — and here that's staying concrete now.

---

## 6. Testing and CI — the verification gap that let §2.1 through

- **No external-decoder validation exists**, and the documented blocker
  ("ffmpeg isn't installed in this environment") has a fix that needs
  neither ffmpeg nor network access: add `symphonia` (pure-Rust MP3
  decoder) as a **dev-dependency of `mp3-cli`** (never `mp3-core`, to
  keep `no_std` intact), and write a differential test —
  encode → decode → compare SNR/correlation against the original PCM.
  This is precisely the test that would have caught §2.1 on day one.
- **No CI/CD at all** — no `.github/workflows`, confirmed by direct
  search. All verification today is `verify.sh` run by hand. Minimum
  viable pipeline: `cargo fmt --check` + `cargo clippy --all-targets -- -D
  warnings` + `cargo test --workspace` + `cargo test --no-default-
  features` + `cargo build -p mp3-core --target wasm32-unknown-unknown
  --no-default-features` + the Symphonia differential test + an MSRV
  (1.82) pin check + `cargo-deny` (license/advisory compliance — doubly
  important given §2.3). GitHub Actions runners have `ffmpeg`
  pre-installed or trivially installable, so this also lets a stronger
  external check run in CI even without adding it to the local dev loop.
- **No `rust-toolchain.toml`.** The roadmap's revision log documents the
  *same* system-rustc-vs-rustup-toolchain conflict corrupting builds
  across M4, M6, and M9 — a 3-line file eliminates the whole class of
  failure that `verify.sh`'s PATH-resolution workaround exists to
  compensate for.
- **No `clippy.toml` with `disallowed-methods`.** The `f32::sin`/`cos`/
  `sqrt`/`round` (std-only, silently broken under `--no-default-features`)
  regression hit M2/M3, M4, M7, and M8 — four separate times, per the
  roadmap's own log. A `disallowed-methods` lint pointing at a
  `crate::math` facade (native under `std`, `libm` under `no_std`) turns
  this into a compile-time lint failure on every build, not something
  that only surfaces when someone remembers to run the WASM check. As a
  side benefit, gating on `std` lets native builds use the platform's
  faster intrinsics instead of unconditionally paying `libm`'s cost.
- **Property-based testing is concentrated in `huffman/encode.rs` only.**
  `quantize/loop_control.rs` has clear invariants (monotonic in `step`,
  sign preserved, `ix >= 0` always) currently covered only by hand-picked
  cases — a `proptest` generating random spectra (including extreme
  magnitudes) asserting "never panics, `ix` always non-negative" would
  likely find edge cases the current suite doesn't.
- **No per-stage benchmarks** — only end-to-end `realtime_factor.rs`
  exists. Add `criterion` benches per stage (filterbank, MDCT, FFT,
  quantize, Huffman) before starting §4's SIMD work, so vectorization
  priority is driven by measurement, not intuition.
- **`mp3-cli/tests/cli.rs` doesn't clean up on test failure** — manual
  `remove_file()` calls are skipped if an earlier `assert!` panics.
  Low-priority, but a one-line fix (`tempfile::TempDir`/`NamedTempFile`,
  cleaned via `Drop` even on panic).

---

## 7. Rust idiom and code-quality cleanup (medium/low priority, batch together)

These don't block anything above but are worth a dedicated pass, roughly
in this order:

1. **`mp3-cli/src/main.rs`**: replace the ~13 repeated
   `.unwrap_or_else(|e| { eprintln!(...); exit(1); })` blocks (two with
   literally duplicated error messages) with `fn run(args) ->
   anyhow::Result<()>` + `?`, a single error-handling point in `main`,
   and differentiated exit codes (sysexits-style: 64 usage, 66 input
   file, 70 internal). While there, validate `WavSpec::sample_format`/
   `bits_per_sample` up front instead of failing sample-by-sample on
   unsupported formats (float32/24-bit WAV today produces a generic
   per-sample error instead of a clear rejection at the boundary).
2. **`huffman/tables.rs`** (6,051 lines, the only file far over this
   project's own 800-line ceiling): split into `huffman/tables/{vlc.rs,
   count1.rs, selection.rs}` re-exported from `mod.rs` — pure file
   reorganization, no data or type changes, keeps `no_std`/zero-deps
   intact. While there, replace `BIG_VALUES_TABLES: [Option<(usize,
   u8)>; 32]`'s anonymous tuples with a named `struct BigValuesEntry {
   vlc_table_index: usize, linbits: u8 }`.
3. **`huffman/encode.rs:338`**: `BIG_VALUES_TABLES[table_id as
   usize].unwrap()` is safe today (traced: `table_id` only ever comes
   from `choose_table_and_cost`, which only sets it when the lookup
   returned `Some`) but has no `// INVARIANT:` comment, unlike nearly
   everywhere else in this codebase where invariants are meticulously
   documented. Add the comment, or convert to `.unwrap_or((0,0))` with an
   explicit `debug_assert!`.
4. Dead code cleanup: `FFT_SIZE_SHORT`/`FFT_BINS_SHORT`/`struct FftBin`
   (`model2.rs:37,41,78`) are genuinely unused — either finish wiring
   short-block psychoacoustic support (real quality improvement on
   transients, and needed for §2.1/§5.5's short-block work anyway) or
   remove them with an issue reference; a permanent `#[allow(dead_code)]`
   citing milestones that are now closed (`tables.rs:10`) is stale.
5. `#![warn(missing_docs)]` → `#![deny(missing_docs)]` in `lib.rs` — stop
   relying on `clippy -D warnings` always being the invocation used.
6. Narrow the three module-wide `#![allow(clippy::needless_range_loop)]`
   (`mdct/mod.rs:19`, `psychoacoustic/fft.rs:9`, `quantize/
   loop_control.rs:6`) to the specific loops that need it, with a comment
   — `encoder.rs:302` already does this correctly and can be the
   template.
7. Remove or gate the `simd` feature flag (`mp3-core/Cargo.toml`) — today
   enabling it silently changes nothing, with no compiler warning to say
   so.

---

## 8. Project hygiene / publication readiness

Quick-hit items, cheap now and expensive to retrofit after a public
release:

- Add `LICENSE-MIT`/`LICENSE-APACHE` (blocking, see §2.3).
- `Cargo.toml`'s `repository = "https://github.com/REPLACE_ME/encoRust"`
  is a literal placeholder that would publish as-is. Fix, and add the
  `keywords`, `categories`, `readme`, `documentation`, `homepage`, and
  `[package.metadata.docs.rs]` fields all three crates currently lack.
- `lib.rs:11-12`'s doc comment still reads "This crate is a scaffold:
  every DSP-heavy function is a documented `todo!()`" — false since M8,
  and would be the front page on docs.rs.
- Crate names (`mp3-core`, `mp3-cli`, `mp3-wasm`) are generic enough to
  likely collide on crates.io. For a project positioning itself against
  `shine-rs`/`rusty_mp3`/`oxideav-mp3`, consider `encorust-core`/
  `encorust`/`encorust-wasm` (binary name `encorust` is already correct)
  — free to rename now, a deprecation cycle later.
- Add `CHANGELOG.md` and document the MSRV policy (the project already
  pins `rust-version = 1.82`, which is more diligence than most
  competitors show — make it visible).
- README's status banner ("under development, does NOT produce valid MP3
  files") is now stale relative to the roadmap (M0-M9 done) — update
  before using the README to attract contributors or comparisons against
  LAME/Shine, since right now it undersells the project's actual state.

---

## 9. Where this leaves encoRust vs. the competition

Context gathered for this review, useful for calibrating how much of the
above actually matters competitively:

- **`shine-rs`** (LGPL-2.0, pure-Rust port of Shine): its own published
  benchmarks show **114.1× real-time** vs. Shine-C's **130.4×** — a 14%
  gap from a heap-allocating, fixed-point-arithmetic implementation.
  Shine is a deliberately minimal CBR-only encoder without a full
  Psychoacoustic Model II, so this isn't an apples-to-apples comparison
  with encoRust's feature scope — but it's concrete evidence that a Rust
  MP3 encoder can get within touching distance of hand-tuned C without
  needing to eliminate every allocation, *if* the allocation count is
  already low and the arithmetic is simple. encoRust's current ~7.47×
  stereo (full Model II, floating point, before any of this document's
  fixes) has real room to close that gap — §3 and §4 are the concrete
  path there, not a hand-wave.
- **`rusty_mp3`** (Apache-2.0) claims exactly the three features encoRust
  has explicitly deferred — real VBR, joint stereo, cross-frame
  reservoir. Worth a structural read (never a code source, per this
  project's own provenance rules) once Phase 6 below starts on any of
  those three.
- **LAME** remains the quality/feature benchmark. Nothing in this
  document changes that target — it sharpens the path to it.

The honest positioning today: encoRust's differentiator is **provenance
and correctness rigor** (every constant traced to Annex B, every
milestone independently re-reviewed) more than raw performance — and
§2.1's finding shows that rigor still has a real gap. Closing Phase 0
and Phase 1-2 of this document is what turns "provenance-rigorous" into
"provenance-rigorous *and* competitive," which is the actual claim the
README wants to be able to make.

---

## 10. Priority matrix and suggested order

| Phase | Focus | Depends on | Est. relative effort |
|---|---|---|---|
| **0** | §2.1 block_type containment, §2.2 VBR/ABR rejection, §2.3 license/provenance | — | Small, do first |
| **1** | §3 memory — M-1 through M-6 | Phase 0 (touches `encoder.rs`/`model2.rs` either way — do together) | Medium |
| **2** | §6 CI + Symphonia differential test | Phase 0 | Small — and should land *before* Phase 3-4 so those refactors are safety-netted |
| **3** | §5.1 API surface + `#[non_exhaustive]`, §6 `rust-toolchain.toml`/`clippy.toml` | — | Small, cheap now/expensive later |
| **4** | §5.2 `encode_frame` decomposition + §3.3 struct-size cleanup | Phase 1-2 (tests must exist first) | Medium-large |
| **5** | §4 SIMD | Phase 1 (fix overhead before vectorizing it), §6 per-stage benchmarks | Large |
| **6** | §5.3 real reservoir/`FrameAssembler`, §5.4 look-ahead, §5.5 short blocks (closes §2.1 structurally), real VBR/ABR, joint stereo, LSF | Phase 3-4 | Large, and each sub-item is roughly its own milestone in the existing roadmap's style |
| — | §7 idiom cleanup, §8 publication hygiene | None — can interleave anywhere | Small, batch opportunistically |

This mirrors the priority ordering the architecture review converged on
independently: fix what's silently wrong, make it verifiable, make the
API safe to freeze, *then* invest in performance and new features — in
that order, not the reverse.

---

## 11. Gain/corruption investigation and a still-open, more fundamental
## fidelity bug (2026-08-26 through 2026-08-29)

**Status of this section**: a running log from a multi-session debugging
investigation, kept in append-only order (most recent problem at the
bottom). Start a fresh session on this by reading this whole section
first, then jumping straight to "Where the next session should start."

### What was reported

Real-recording MP3 output (`chickens_16bit.wav`, a ~22s stereo 44.1kHz
recording of chicken/rooster vocalizations) sounded wrong: initially
reported as "+6.9 dB peak vs -15.8 dB original, clipping"; after the
first round of fixes, as "still sounds like a kind of echo, noisy."

### Fixes landed this investigation (all verified, all still valid)

Each bullet is one commit on `feat/post-m9-scope-limitations`, oldest
first. All 141 `mp3-core` unit tests and the full workspace suite pass
as of the last commit below; none of these were reverted.

1. **`0c1721a`** — `main_data` was byte-aligned per scalefactor/Huffman
   section instead of packed as one continuous bit stream per frame.
   ffmpeg overreads on the real recording: 1043 → 75.
2. **`e406ae3`** — Long-block `region0_count`/`region1_count` used an
   internally-consistent but decoder-incompatible split (verified
   against an external reference: `region0_end = band_index[region0_count+1]`,
   etc. — real decoders derive the boundary *only* from these two
   fields, this encoder's actual Huffman-table switch point didn't
   match). Overreads: 75 → 0.
3. **`e1647aa`** — The rate loop only ever searched `step` upward from a
   fixed 0 (never finer/negative), which is incompatible with this
   encoder's PCM normalized to `[-1.0, 1.0]` (`io/pcm.rs`) — real
   spectra are often small enough that `step=0` already quantizes
   everything to zero, with no escape since the loop could only
   coarsen. Replaced with a binary search over the *entire*
   representable range (global_gain 0..=255). Required a paired fix:
   the rate loop's own bit-cost estimator was a crude, disconnected
   heuristic (`crate::huffman::estimate_bits` — the *accurate*,
   already-existing estimator built for this exact contract — is used
   now instead), and `quantize_spectrum` needed to saturate `ix` to
   8206 (`MAX_REPRESENTABLE_IX`, the largest value any Huffman
   big_values table can represent) since the wider search can otherwise
   produce genuinely unrepresentable values that `write_bits` would
   silently truncate instead of erroring.
4. **`0127039`** — The forward MDCT (`mdct_36`/`mdct_12`) had no `2/N`
   normalization, verified missing against a real, independent decoder
   implementation (FlorisCreyf/mp3-decoder's `imdct()`: a raw,
   unnormalized cosine sum). Encoding without it and decoding with any
   compliant decoder reconstructs audio `N/2` (9x, +19dB for long
   blocks) too loud. This alone regressed a passing test (moderate
   content decoding to silence) until paired with fix 3 above — the
   two were diagnosed and must ship together.
5. **`e74a26a`** — The absolute threshold of hearing (`ath_from_db`) was
   never re-anchored from dB SPL to the model's 0 dBFS energy
   convention (missing a ~96 dB offset), pinning masking thresholds to
   the SMR floor for nearly all real (non-full-scale) content and
   starving the outer loop's masking-driven precision allocation.
   Visible in a spectrogram as a uniform noise wash filling quiet gaps
   that should be dark/clean. This had been tried and reverted once
   already earlier in the investigation because it appeared to have
   zero effect and broke a test — both true only because bug #1-2's
   corruption dominated every measurement at the time.
6. **`ba67e8e`** — `compute_perceptual_entropy` (drives short-block
   transient detection) computed its own "threshold" as the *same
   bin's own energy*, scaled by a fixed 1e-6 — i.e. a constant,
   content-independent ratio, not a real distortion measure. Result: 0
   of 3360 granules on the real recording ever used anything but
   `BlockType::Long`, even across its sharpest attacks — every
   transient was encoded with a full 26ms window (textbook MP3
   pre-echo). Fixed to use the real, already-computed masking
   threshold; also fixed a mis-scaled/mis-named attack threshold
   constant and a warm-up-period bug in the tonality predictor
   (needed 2 granules of real history, not 1).
7. **`611e2cb`** — Once short blocks actually started triggering (via
   fix #6), a *previously dead* code path lit up for the first time and
   immediately corrupted the bitstream (3 new overreads): pure short
   blocks' big_values region split must be the *fixed* line 36/576
   split (verified against the same external decoder reference as fix
   #2), not a content-dependent one — region0_count/region1_count
   aren't transmitted at all for `window_switching_flag=1`.
8. **`8189212`** — The outer loop could keep incrementing a band's
   scalefactor past the point where `ix` saturates at
   `MAX_REPRESENTABLE_IX` (introduced by fix #3) — a hard ceiling
   violation, not graceful precision loss. Fixed to stop retrying
   already-saturated bands.

### The bug none of the above actually fixed

After all 8 fixes above, the *reported symptom* (something between
"noisy" and "echo") persisted, only modestly improved. Peak/RMS levels
on the real recording were finally in a normal, unremarkable range
(-14.5 dB decoded vs -16.2 dB original — ordinary lossy-encoding
variation), which is what made this section's investigation possible:
with gain no longer 20+ dB off, a *precision* metric became meaningful
for the first time.

**Method**: cross-correlation between the original WAV and the
ffmpeg-decoded MP3, scanned over a wide lag range (±3000 samples ≈
±68ms at 44.1kHz), on a raw (no windowing, no spectral tricks)
time-domain signal. A good encoder should show one dominant, sharply-
decaying correlation peak near whatever fixed encoder+decoder algorithmic
delay applies.

```python
import numpy as np, wave
def load_mono(path):
    w = wave.open(path, 'rb'); n = w.getnframes(); ch = w.getnchannels()
    s = np.frombuffer(w.readframes(n), dtype=np.int16).astype(np.float64)
    return (s.reshape(-1, 2).mean(axis=1) if ch == 2 else s), w.getframerate()

orig, sr = load_mono('original.wav')
dec, _ = load_mono('decoded.wav')  # ffmpeg -i out.mp3 decoded.wav
start, seg_len = int(1.0 * sr), int(0.2 * sr)  # adjust window to taste
o = orig[start:start+seg_len]
best = []
for lag in range(-3000, 3001):
    d = dec[start+lag:start+lag+seg_len]
    if len(d) == len(o):
        best.append((lag, np.corrcoef(o, d)[0, 1]))
best.sort(key=lambda x: -x[1])
print(best[:10])
```

**Reference point (this method is sound)**: `ffmpeg -codec:a libmp3lame
-b:a 192k` on the exact same recording, same segment, same script:
**0.9999 correlation at lag 0**, decaying smoothly (0.958, 0.855, 0.733,
0.618, 0.517 at ±1, ±2, ±3, ±4, ±5 samples) — the expected shape for a
well-aligned, high-fidelity encode.

**encoRust, same recording, same segment, current code (post fix #8)**:
best correlation found anywhere in the ±3000-sample range was **~0.06**,
with no single dominant peak — many similar-magnitude candidates
scattered across the whole range. Repeated on a *synthetic, pure 1kHz
sine tone at 320kbps* (the simplest possible signal to encode well,
generous bitrate) instead of the real recording: best correlation
**~0.04-0.15** depending on exact fix state, again no dominant peak.
This is not "encoded with some quantization noise" — it's essentially
uncorrelated with the source at the sample level, while still *looking*
roughly right in a spectrogram/waveform-envelope view (which is why this
went unnoticed until a correlation check specifically was run).

**FFT of the decoded pure tone** (4096-point, Hann window, same segment):
energy is smeared across several adjacent bins around 1kHz (990.5,
1001.3, 1012.1 Hz cluster; a second 1065.9-1098.2 Hz cluster; an outlier
at 1152.0 Hz) instead of one sharp peak, and the peak magnitude is ~23x
smaller than the original's. The two clusters are roughly 65-75 Hz
apart, in the same ballpark as the granule rate (576 samples / 44100 Hz
≈ 76.6 Hz) — suggestive of amplitude modulation at roughly the granule
or frame rate, though not confirmed as the root cause (fix #8, which
directly targeted granule-to-granule gain instability, made no
measurable difference to the correlation numbers above).

**Confirmed pre-existing, not a regression from this investigation**: a
disposable `git worktree` was built from commit `f392781` (the last
commit before this entire investigation started) and given the exact
same pure-tone test. Result: **~0.019** correlation, same "no dominant
peak" shape, *slightly worse* if anything. This bug has been present
since at least that commit — it is not something fixes #1-8 introduced,
and it is very likely present in every MP3 this project has ever
produced, undetected because:
- The only differential-decode test in the suite
  (`crates/mp3-cli/tests/symphonia_diff.rs`) explicitly does not check
  correlation/SNR ("SNR/correlation targets belong in the roadmap's
  future milestones" — see that file's own comment); it only asserts
  non-silent, finite, roughly-expected-length output.
- `long_mdct_perfect_reconstruction`/`short_mdct_perfect_reconstruction`
  (`mdct/mod.rs`) verify the MDCT stage in isolation (forward + a
  test-only inverse cancel out), which cannot catch a bug in how
  granules chain together across calls (`mdct_prev_tail` state) or in
  the polyphase filterbank feeding it — exactly the surface a real,
  continuous multi-granule encode exercises that an isolated single-call
  unit test cannot.

**Ruled out this session** (both tested directly against the pure-tone
correlation check, no measurable effect either way):
- The anti-aliasing butterfly (`mdct::antialias_butterfly`, wired into
  `encoder.rs` this same investigation window) — disabling it entirely
  produced numerically near-identical correlation numbers. Its rotation
  coefficients are close to identity (`cs` ∈ [0.857, 0.9999], `ca` ∈
  [0.0037, 0.514]) so this is not surprising in hindsight, but it was
  the first, cheapest hypothesis to eliminate.
- Fix #8 (saturation-aware outer loop) — real bug, correctly fixed, but
  did not move the correlation numbers, so granule-to-granule
  scalefactor/global_gain instability via *that specific mechanism* is
  not the (or not the whole) root cause.

### Session 3 (2026-08-29, continued): quantizer and `mdct_prev_tail`
### chaining both ruled out with direct measurements

Followed the previous session's step 1 and step 3 exactly. Both produced
clean, decisive negative results — the bug is **not** in either of the
two most-suspected areas.

**Step 1 — bypass the quantizer's search entirely.** Added a
`#[cfg(test)]`-gated bypass to `quantize_granule`
(`quantize::loop_control::set_debug_fixed_step`): when armed, it skips
the inner (rate) and outer (distortion) loops completely and quantizes
at a caller-chosen fixed `step` with `scalefac` forced to all-zero — no
search, no SMR-driven amplification. Compiles to nothing outside
`mp3-core`'s own test binary (verified absent from release builds; it's
behind `#[cfg(test)]`, not a runtime flag).

Used it in a new test, `Encoder::tests::diag_bypass_quantizer_pure_tone`
(`#[ignore]`d — writes `diag_tone.mp3`/`diag_tone_original.wav` to
`$DIAG_OUT_DIR`, default `/tmp`, as a side effect; run explicitly with
`cargo test -p mp3-core --lib diag_bypass_quantizer_pure_tone -- --ignored --nocapture`).
It runs the *real* `Encoder::encode_frame` pipeline (real filterbank,
real psychoacoustic model, real MDCT) for a 2.1s, 1kHz, mono, 320kbps
tone, with only `quantize_granule`'s search bypassed. The fixed step is
chosen by first probing the real spectrum's peak magnitude on a
throwaway encoder instance, then solving for the step that lands the
peak line's `ix` near 1000 — nowhere near the 8206
(`MAX_REPRESENTABLE_IX`) ceiling that steady-state real encodes were
observed hitting before fix #8.

**Result: correlation stayed at ~0.03** (peak found by scanning ±3000
samples, same methodology as the rest of this section) — no
improvement over the un-bypassed encoder's ~0.04-0.15. The FFT
signature is *identical* to previous sessions' unbypassed measurements
too: decoded peak still lands at ~1076.7 Hz against a ~1001.3 Hz
original (same ~7.5% pitch shift, same smeared multi-bin cluster).
Per the previous session's own decision rule: **this rules out the
quantizer/rate-loop interaction as the root cause** — the corruption is
already present in the spectrum *before* `quantize_granule` ever runs.
(Separately: the rate loop's "binary-search for the single finest step
that fits the whole granule's bit budget" design is still worth
revisiting later — it's what drove real encodes to `ix=8206` saturation
for sparse/tonal spectra in the first place, since a generous bitrate
budget applied to a granule with only 1-2 significant lines has no
mechanism to stop precision short of the hard `ix` ceiling. That's a
real design smell, and probably audible on sparse/tonal real-world
content, but it is not *this* bug.)

**Step 3 — `mdct_prev_tail` chaining, isolated from filterbank/model.**
Added `mdct::tests::transform_long_multi_granule_chaining_reconstructs`:
drives the same `transform_long` call and tail-update pattern
`Encoder::mdct_stage` uses, for 6 consecutive granules, on a synthetic
subband-domain signal (no filterbank, no psychoacoustic model — pure
MDCT-domain chaining). Overlap-add reconstruction (via the module's own
test-only `imdct_36`) is checked against the original signal at every
granule boundary.

First version of this test asserted the wrong "expected" slice (compared
granule `gr`'s overlap against granule `gr`'s own input instead of
granule `gr-1`'s — an off-by-one-granule mistake in the *test*, not the
encoder: `transform_long`'s returned tail is `*input`, so the raw data
shared between granule `g` and `g+1`'s windowed blocks is granule `g`'s
input, not granule `g+1`'s). That version failed loudly (reconstructed
≈0 vs expected -0.73), which is a useful cautionary note for whoever
revisits this: a shifted-by-one-granule comparison in a *test* produces
exactly the kind of "looks completely broken" failure a real bug would
also produce — verify the indexing algebra by hand before trusting a
red multi-granule test here. With the comparison corrected, **the test
passes cleanly** (reconstruction within `1e-3` at every one of the 5
granule boundaries checked). **This rules out the tail-passing mechanism
itself** as the bug — `mdct_prev_tail`'s "raw, unwindowed `*input`"
convention is mathematically sound for chained long blocks.

**Where this leaves the investigation**: both of the two
highest-suspicion areas from the previous session are now eliminated
with direct evidence, not just "fix X didn't move the needle" inference.
What's left, per the original priority list's step 4 plus one new
candidate this session's evidence points at:

- **The polyphase filterbank's history/cosine-matrix convention**
  (original step 4) — still unverified against an independent numeric
  reference (its own tests tolerate "significant leakage," see that
  step's original note below).
- **How `Encoder::analyze_pre_mdct`/`mdct_stage` assemble the filterbank's
  32×18 output into MDCT input and the flat 576-line spectrum** — the
  tail-chaining mechanism was tested in isolation with a hand-built
  synthetic subband signal, which does *not* exercise whether
  `subband_samples[sb][fbc]` (built from 18 consecutive
  `PolyphaseFilterbank::analyze` calls in `analyze_pre_mdct`) or the
  `mdct_out[sb][k] → spectrum[sb*18+k]` flattening in `mdct_stage`
  preserve the *correct* time/frequency ordering the filterbank
  actually produces. A consistent, wrong ordering convention here
  (rather than a chaining bug) would explain a *stable-frequency-band,
  wrong-fine-structure* signature just as well as a tail bug would, and
  wouldn't be caught by either of this session's isolated tests.
- A quick, likely-unreliable side observation from this session, **not**
  to be treated as evidence: an ad-hoc probe that logged which flat
  spectral line held peak energy per granule showed it alternating
  between two adjacent lines most granules (normal MDCT leakage for a
  tone not exactly on a bin center) but jumping further away for two
  granules in a row before returning. That probe reused
  `analyze_pre_mdct` directly without replicating `encode_frame`'s real
  `pcm_history` bookkeeping (it always fed zeros as look-back), which
  plausibly triggers spurious transient/short-block detection in the
  psychoacoustic model independent of any real encoder bug — this was
  *not* investigated further and should not be treated as a lead without
  redoing it with correct history.

### Where the next session should start

Both prior hypotheses (quantizer/rate-loop, `mdct_prev_tail` chaining)
are now ruled out with passing/failing tests, not inference — don't
re-open them without new evidence. Start here instead:

1. **Verify the filterbank → MDCT input/output ordering convention**,
   isolated from chaining: extend
   `diag_bypass_quantizer_pure_tone`-style probing (or a new, cleaner
   test) to check, for a *stable* granule (steady-state tone, several
   granules in), whether the *same* flat spectral line index holds peak
   energy every granule — not just "which subband" (already confirmed
   stable and analytically correct: subband 1 for a 1kHz tone at
   44.1kHz, matching `floor(1000 / (44100/2/32))`), but "which of the 18
   lines within that subband," with **correct** `pcm_history` chaining
   this time (reuse `Encoder::encode_frame`'s real bookkeeping, or drive
   the probe through a sequence of real `encode_frame` calls and hook in
   at `mdct_stage` via the same kind of instrumentation used this
   session — cleaned up before committing either way). A jumping/unstable
   peak line for a *stationary* tone, several granules past start-up, is
   the smoking gun this step is looking for.
2. **Suspect the polyphase filterbank's history/cosine-matrix
   convention** (original step 4, unchanged) — its own unit tests
   (`impulse_response_matches_filter_shape`,
   `sine_energy_concentrates_in_expected_bin`) tolerate "significant
   leakage to adjacent subbands... corrected by the MDCT/anti-alias
   butterfly in chapter 06" per their own comments, which means those
   tests would not catch a filterbank-side bug that produces technically
   plausible-looking but subtly wrong subband values. The matrixing
   formula (`filterbank/polyphase.rs`'s `analyze`,
   `M[k][i] = cos((2k+1)(i-16)π/64)`) was re-derived by hand this
   session from the modulated-filter-bank definition and matches both
   the project's own doc citation
   (`docs/mp3-encoder/05-phase2-polyphase-filterbank.md` §1 step 4) and
   the standard formula — **not** re-flagged as a suspect on formula
   grounds, but the *values* (prototype filter table, history
   shift/insertion order) are still only self-consistency-tested, never
   checked against an independent numeric reference — see that chapter's
   own "table provenance" requirement, which was never actually done.
3. **Write the test that should already exist** (carried over,
   unchanged): a granule-chaining integration test that feeds a
   multi-granule *continuous* tone through the real `encode_frame` path,
   decodes with `symphonia` (already a dev-dependency, see
   `symphonia_diff.rs`), and asserts correlation/SNR above a real
   threshold. `diag_bypass_quantizer_pure_tone` (this session, `#[cfg]`
   `#[ignore]`d, shells out to `ffmpeg` + Python) is a stopgap for manual
   runs, not this — the permanent regression guard should decode
   in-process with `symphonia`, assert a real threshold, and run by
   default.

### Session 3, part 2 (2026-08-29, same day): root cause found and fixed
### — missing polyphase-filterbank frequency-inversion compensation

Per the plan above, cross-referenced two independent, real, working
open-source encoders for the filterbank→MDCT assembly convention (the
user's suggestion this session, and a good one): **Shine**
(`toots/shine`, `src/lib/l3subband.c` + `l3mdct.c` — already this
project's own cited source for `ANALYSIS_PROTOTYPE_FILTER`) and
**LAME** (`libmp3lame/newmdct.c`, `mdct_sub48`). Both independently
confirmed the polyphase analysis filterbank's own cosine-modulation
matrix (`M[k][i] = cos((2k+1)(i-16)π/64)` — re-verified by hand this
session too, and it does match both the standard and this project's
`filterbank/polyphase.rs`) has an inherent, unavoidable property: for
every ODD-numbered subband, every OTHER time sample (of the 18 per
granule) comes out with its sign flipped relative to the subband's true
baseband signal. Left uncorrected, the MDCT that follows transforms an
effectively frequency-mirrored signal for every odd subband — corrupting
fine spectral structure while leaving gross per-subband *energy*
untouched (a sign flip disappears under squaring), which is exactly why
this survived every prior check: the filterbank's own unit tests, the
spectrogram comparisons, and even this session's own "which subband gets
the energy" probe all only look at *energy*, never sign/fine-structure.

Both reference encoders apply the same fix at the same point in the
pipeline, with matching source comments:
- Shine, `l3mdct.c`, `shine_mdct_sub`: `/* Compensate for inversion in
  the analysis filter (every odd index of band AND k) */` —
  `for (band = 1; band < 32; band += 2) sample[k+1][band] *= -1;`
  (applied only at the *odd* one of each pair of filterbank calls).
- LAME, `newmdct.c`, `mdct_sub48`: `/* Compensate for inversion in the
  analysis filter */` — the identical `band` odd / `k` odd negation, at
  the identical stage (immediately after the polyphase filterbank,
  immediately before MDCT).

This project's `Encoder::analyze_pre_mdct` had no equivalent step at
all — `subband_samples[sb][fbc]` was stored directly from the
filterbank's output with no correction. Fixed by adding the same
negation (`crates/mp3-core/src/encoder.rs`, right after the
`subband_samples` assembly loop, before it's returned for MDCT):
`for sb in odd subbands, for fbc in odd time-indices: negate`.

**Verified**: the previously-observed pure-1kHz-tone corruption pattern
(decoded FFT peak at 1076.7 Hz, ~23× amplitude loss, energy smeared
across a 990-1098 Hz cluster) that survived commits #1-8 *and* this
session's own quantizer-bypass and `mdct_prev_tail`-chaining tests
(neither of which touch this code path — both ran through the *real*
filterbank, since the bug is upstream of both) is now, with this one
fix and nothing else changed:
- **Decoded FFT peak lands exactly on 1001.3 Hz** (matching the
  original), with the two dominant bins (990.5/1001.3 Hz) in the same
  relative magnitude order as the original — no more pitch shift, no
  more smearing into a wrong cluster.
- **Cross-correlation on the same pure tone**: still not at LAME's
  0.9999, but the *shape* of the result changed qualitatively, not just
  its peak value — before this fix, the best-correlation search found
  many similar-magnitude candidates scattered across the lag range with
  no single dominant peak (see this section's earlier notes); after,
  peaks appear at multiples of the tone's own period (~44.1 samples at
  1kHz/44.1kHz) with a clean, symmetric decay shape around each — the
  self-similarity expected from correlating a near-periodic signal
  against itself, which also makes a pure sine tone a weaker
  discriminator for this specific check than initially assumed (any
  near-period-multiple lag scores well by construction, regardless of
  fine-structure fidelity) — see the "still open" note below for why the
  real recording is the more trustworthy signal here.
- **Cross-correlation on the real recording**
  (`chickens_16bit.wav`, 192kbps, same file/method as every number
  earlier in this section): **0.14-0.30** across four spot-checked
  segments (was ~0.02-0.15 before any of commits #1-8, ~0.06 after all
  8). Nowhere near LAME's 0.9999 yet, but — critically — the
  correlation-vs-lag *shape* at the best lag is now a clean, single,
  sharply-decaying peak (checked ±6 samples around the best lag at the
  5.0s mark: 0.20 → 0.30 → 0.20 over ±6 samples, smoothly symmetric),
  qualitatively matching the *shape* of LAME's reference decay pattern
  documented earlier in this section, just at a much lower magnitude —
  not the "no dominant peak, many similar scattered candidates" shape
  every previous measurement in this section reported. This is a
  structural change, not just a number improving: the bitstream now
  carries fine-grained spectral structure a decoder can actually lock
  onto, where before it carried none.
- **Zero ffmpeg overreads** on both the pure tone and the real recording
  after this fix (was already 0 after fix #2, unaffected either way —
  noted for completeness).
- Full workspace test suite (166 tests + this session's new coverage)
  stays green with no changes needed elsewhere; this fix required no
  paired changes to the quantizer, Huffman coding, or bitstream
  assembly, consistent with the bug being purely upstream of
  quantization exactly as step 1's diagnostic (this same session, run
  *before* this fix) had already established.

**Still open**: correlation is unambiguously, structurally much
healthier than before, but 0.14-0.30 is still far from LAME's 0.9999.
The clean single-peak shape suggests what's left is closer to "real but
ordinary quantization noise / precision loss" than "structurally
scrambled," which points back at the *already-documented, separate*
rate-loop finding from step 1's diagnostic this same session: the inner
loop's binary search maximizes precision to fill the entire bit budget
with no distortion-based ceiling, which was independently observed
driving `ix` to the `MAX_REPRESENTABLE_IX=8206` saturation ceiling on
sparse/tonal spectra at generous bitrates. That was ruled out as *this*
session's dominant bug (bypassing it entirely didn't fix the pitch-shift
signature), but it was never ruled out as *a* remaining contributor to
ordinary quantization noise, and is the most likely next place to look
if 0.14-0.30 isn't good enough. Re-run this section's correlation
methodology on both the pure tone and a real recording *after* any
further fix, using the reproduction commands below, to keep this number
current.

### Where the next session should start (updated)

The dominant, structural bug is fixed and verified — don't re-open the
quantizer/rate-loop, `mdct_prev_tail` chaining, or filterbank
cosine-matrix-formula hypotheses without new evidence; all three were
directly tested and ruled out or fixed this session. Next:

1. **Re-run the correlation methodology** (reproduction commands below)
   on a fresh real recording and confirm the 0.14-0.30 range holds or
   improves; if it's still far from LAME, profile *where* the remaining
   distortion concentrates (still-sparse tonal content? transients/short
   blocks specifically? a particular subband range?) before guessing at
   a fix.
2. **Revisit the rate loop's "maximize precision to fill the entire bit
   budget" design** (`quantize::loop_control::inner_loop`) — flagged as
   a real design smell in this session's step-1 diagnostic notes above,
   independent of the bug just fixed. Real encoders (per the same Shine/
   LAME source now available locally in the cargo registry cache, see
   below) drive the rate loop from an SMR/energy-informed starting
   estimate and only coarsen from there, rather than binary-searching
   the entire range for the theoretical finest fit — worth reading
   `quantize.c`/`quantize_pvt.c`'s `outer_loop`/`inner_loop` in LAME for
   the actual reference algorithm shape before redesigning ours.
3. **Write the permanent regression test** (carried over from the
   original list, still not done): an in-process, `symphonia`-decoding,
   correlation/SNR-asserting integration test, run by default — not the
   `#[ignore]`d, file-writing, `ffmpeg`-shelling-out diagnostic this
   session added as a stopgap.

**Where to find reference encoder source**: Shine (`toots/shine` on
GitHub, `src/lib/`) and LAME (mirrored at `zlargon/lame` and
`rhishi/lame` on GitHub — the relevant file is `libmp3lame/newmdct.c`,
*not* `mdct.c`) are both plain C, small enough to read directly over
`curl`/`WebFetch` (no local checkout needed; `WebFetch` alone
summarized instead of returning exact source in this session's own
attempt, so prefer `curl <raw.githubusercontent.com URL>` when the exact
code, not a description of it, is what's needed). Separately,
`symphonia-bundle-mp3` — already a dev-dependency of `mp3-cli`, so
already vendored locally at
`~/.cargo/registry/src/index.crates.io-*/symphonia-bundle-mp3-0.5.5/` —
is a good independent-**decoder** cross-check for any future
decode-side-convention question: its `src/layer3/hybrid_synthesis.rs`
has the decode-side analog of the fix in this section
(`frequency_inversion`, applied to reconstructed time-domain subband
samples *after* IMDCT rather than *before* the forward MDCT — same
underlying cosine-modulated-filterbank property, corrected on whichever
side of the transform needs it), and its `src/synthesis.rs` carries the
same 512-tap prototype-filter table (`SYNTHESIS_D`, exactly `32×` this
project's own `ANALYSIS_PROTOTYPE_FILTER` at every spot-checked index —
consistent with the standard's known analysis/synthesis table
relationship, not re-flagged as a suspect).

Reproduction commands (adjust paths):

```bash
# Run this session's quantizer-bypass diagnostic (writes files, not run
# by default):
DIAG_OUT_DIR=/some/dir/under/$HOME cargo test -p mp3-core --lib \
  diag_bypass_quantizer_pure_tone -- --ignored --nocapture
ffmpeg -y -i /some/dir/under/$HOME/diag_tone.mp3 /some/dir/under/$HOME/diag_tone_dec.wav
# then run the correlation script above on diag_tone_original.wav /
# diag_tone_dec.wav. Note: a sandboxed (snap) ffmpeg may reject /tmp
# outright ("No such file or directory" despite the file existing) --
# use a directory under $HOME instead.

# Or, full CLI round-trip on a hand-built tone:
cargo build --release -p mp3-cli
python3 -c "
import wave, struct, math
sr=44100; amp=8000; freq=1000.0; n=int(sr*2)
frames=[]
for i in range(n):
    v=int(round(amp*math.sin(2*math.pi*freq*i/sr))); frames += [v, v]
w=wave.open('tone.wav','wb'); w.setnchannels(2); w.setsampwidth(2); w.setframerate(sr)
w.writeframes(struct.pack('<%dh'%(2*n), *frames)); w.close()
"
./target/release/encorust -b 320 -o tone.mp3 tone.wav
ffmpeg -y -i tone.mp3 tone_dec.wav
# then run the correlation script above on tone.wav / tone_dec.wav
```

### Session 4 (2026-08-29, later the same day): short-block SMR fix
### landed; a long elimination pass on the remaining Long-block
### distortion narrows the search space a lot but does not yet find it

**Context**: picks up from this section's own "Where the next session
should start (updated)" pointer above -- its item 1 (short-block SMR grid
mismatch) and item 2 (rate-loop redesign, revisited and deprioritized
below after direct inspection against the reference).

Also landed this session, orthogonal to the DSP investigation:
`compare_audio.sh` was rewritten and `scripts/audio_fidelity.py` added --
fixes a real bug in the old script (the loudness section's `grep` only
ever matched the "Integrated loudness:" header line, never the `I:`/
`LRA:`/`Peak:` value lines after it, so it always printed blank) and adds
the lag-aligned correlation + per-band profile section that most of this
session's numbers below come from. See that script's own header comment
for usage; `SKIP_IMAGES=1` skips the waveform/spectrogram PNGs for faster
iteration.

#### Part 1: the short-block SMR fix -- landed, tested, confirmed
correct, but currently dormant on real content

`ScalefactorBandSmr` gained a `short_bands: [f32; 13]` field
(`crates/mp3-core/src/psychoacoustic/model2.rs`), computed in
`analyze_granule`'s new "Step 7b" by mapping the *same* per-partition SMR
values already computed for the long-block `bands` field onto
`SFB_SHORT_BOUNDARIES`/`SFB_SHORT_COUNTS` instead, using the correct
line->Hz scale for a 192-line short window (`/192`, not `/576`).
`quantize::loop_control::quantize_granule`'s outer loop now reads
`smr.short_bands[b]` instead of `smr.bands[b]` when
`block_type == Short`. Three new tests guard this:
`smr_short_bands_peak_falls_in_the_tones_own_frequency_range` (line->Hz
conversion correctness), `smr_short_bands_tone_produces_high_smr_near_own_frequency`
(parity with the existing long-block tone test), and
`outer_loop_short_block_uses_short_bands_not_long_bands` (a deliberately
contradictory `bands`/`short_bands` setup that can only pass if the outer
loop reads the *short* grid for `Short` granules -- fails against the
pre-fix code). All 145 mp3-core unit tests plus the full workspace suite
pass.

**Empirically confirmed dormant on `chickens_16bit.wav`**: instrumented
`Encoder::analyze_pre_mdct` (temporarily, since removed) to count
`block_type` across the whole file -- **3360 of 3360 granules are
`BlockType::Long`**. Zero short blocks ever trigger on this recording, so
this fix changes nothing for it (confirmed: re-encoding after the fix
produces a byte-identical MP3 to before it -- the fidelity-scan numbers
below match to 4 decimal places what a pre-fix build produced). The fix
is real and tested, just not exercised by this particular test file. This
also means the "why doesn't a percussive recording ever trigger a short
block" question (item 3 in the previous "next session" list) is now the
more relevant of the two remaining psychoacoustic-model threads, but was
not investigated this session -- see "Where the next session should
start" below.

#### Part 2: chasing the remaining Long-block distortion (2-8kHz,
correlation ~0.34 on real content / ~0.04 on synthetic broadband noise)

**Control experiments** (established the symptom is real and
content-dependent, not a measurement artifact):
- LAME encoding the *same* `chickens_16bit.wav` at the same bitrate: 0.9999
  correlation, all 5 frequency bands >0.99 -- confirms the fidelity script
  itself is trustworthy.
- encoRust without `--joint-stereo` (discrete stereo): same bad pattern as
  with `--joint-stereo` (avg 0.336 vs 0.340) -- rules out joint-stereo/MS
  as the cause; the bug lives inside single-channel processing.
- Synthetic broadband white noise (removes tonal self-periodicity
  ambiguity that made a pure-tone test a weak discriminator earlier in
  this investigation): encoRust **0.042** average correlation, ~0.03-0.06
  in every frequency band, flat/scattered decay shape (no dominant lag) --
  vs. **LAME 0.946** on the *identical* noise file (decode peak -0.83
  dBFS, normal). encoRust's decode peak on the same noise file: **+12.15
  dBFS** (~4x full scale -- genuine amplitude overflow; independently
  confirmed via `wave`/numpy that many decoded samples hard-clip to
  -32768).
- Disabling `mdct::antialias_butterfly` entirely on the noise file: no
  measurable change (0.042 -> 0.042 average correlation). Its formula was
  also hand-verified against ISO §2.4.3.4.9.4 and matches exactly. Ruled
  out.
- Debug-build capacity/budget invariants
  (`Encoder::encode_frame`'s frame-level `main_data` capacity
  `debug_assert!`, and the finer per-granule/channel budget
  `debug_assert!`): neither ever fires on the noise file, even in a debug
  build where `debug_assert!` is live. Ruled out silent truncation in
  `--release` (the `emit_len = ....min(main_data_capacity)` clamp at the
  end of `encode_frame` never actually engages here).

**Byte-level verification on the actual encoded bitstream** (frame 0 of
the noise file, both granules -- wrote a throwaway Python bit-parser
against `bitstream/side_info.rs`'s own documented 59-bit-per-granule
layout, not committed to the repo):
- `part2_3_length`, `big_values`, `global_gain`, `scalefac_compress`,
  `region0_count`/`region1_count`, `table_select`, `preflag`,
  `scalefac_scale`, `count1table_select`, and the raw transmitted
  scalefactor values all matched the encoder's own intended values
  *exactly*, for both granules of frame 0.
- Decoded granule 1's entire `big_values` region (576 lines, table 12
  throughout, since `table_select=[12,12,12]` for that granule) by hand
  using the project's own `VLC_TABLE_12` data: reconstructed values
  matched the encoder's intent exactly (max abs value 7), consuming
  exactly the declared bit count (2175 bits, zero drift).

**False lead, caught and corrected before being reported as fact**: a
diff against LAME's `libmp3lame/tables.c` initially showed ~63/64 bit-
length mismatches in *every* VLC table checked (1,2,3,5-13,15,16,24) --
looked like a crate-wide Huffman table corruption, a very plausible-
looking root cause given everything else upstream had just checked out.
Before reporting it, verified two ways: (1) the project's own table
satisfies the Kraft equality exactly (sums to 1.0) and is provably
prefix-free (0 conflicts across all 64 codewords) -- the signature of a
valid, complete, unambiguous code, which a genuinely corrupted table
would be very unlikely to produce by accident; (2) cross-checked tables 1
and 12 against **Symphonia**
(`symphonia-bundle-mp3-0.5.5/src/layer3/codebooks.rs`'s `MPEG_CODES_*`/
`MPEG_BITS_*` constants, MPL-2.0, already a dev-dependency of `mp3-cli` --
found locally under `~/.cargo/registry/src/*/symphonia-bundle-mp3-0.5.5/`)
-- both match the project's tables *exactly*, code and bit-length, every
entry. Conclusion: the LAME extraction was misread this session (that
source file likely uses a different internal indexing/derivation
convention for its `t*l` "hlen" arrays than a flat per-symbol bit count --
not chased further since Symphonia settles the actual question) --
**the Huffman tables are correct**. Recorded here specifically so a
future session doesn't re-open the same LAME-source comparison and
rediscover the same false alarm.

**Still unresolved -- and a targeting bug in this session's own
methodology, found late**: this session located the exact decoded PCM
sample that hard-clips (sample index 1072 in the decoded noise file,
~24.3ms in) and assumed it falls inside the *encoder's* granule 1
(encoder samples 576-1151) via naive `sample_index / 576` arithmetic.
That ignores the encoder+decoder's own combined algorithmic delay --
already measured earlier this same investigation at **~918 samples
(20.8ms)** on the real recording (see this section's "root cause found
and fixed" write-up above). Net: this session spent real, valid effort
verifying granule 1's bitstream representation bit-perfectly correct
(previous paragraph), but almost certainly audited the *wrong* granule
relative to where the clipped sample actually originates once the ~918-
sample decoder delay is subtracted out. **Don't repeat the granule-1
byte-level audit** -- it's already clean, and the same manual-bit-parser
approach is now validated and reusable. Re-locate the true source granule
first (see below).

### Where the next session should start (updated again)

1. **Re-locate the actual source granule before auditing further.**
   Encode the reproduction noise file (script below), decode it, find the
   clipped/anomalous sample index (`numpy.argmax(np.abs(decoded))`), then
   **subtract the encoder+decoder delay** (re-measure it for this exact
   build/bitrate via the pure-tone or impulse method already used earlier
   in this section -- don't reuse the ~918-sample number blind, it may
   depend on config) before dividing by 576 to get the real granule
   index. A clean way to calibrate the delay precisely: encode a signal
   that's silence except for one full-scale single-sample impulse at a
   *known* sample index, decode, and find where that impulse's energy
   lands in the output.
2. **Repeat the byte-level audit on the correctly-identified granule**,
   reusing this session's now-validated method: hand-parse
   `side_info.rs`'s 59-bits/granule layout, then hand-decode the
   `big_values`/`count1` region against the (now Symphonia-cross-checked,
   trusted) tables in `huffman/tables.rs`. If it *also* checks out
   perfectly, the bug likely isn't in any single granule's own
   representation at all -- move to item 3.
3. **If item 2 comes back clean too**, look at cross-granule/cross-frame
   state specifically under content that keeps every subband busy at
   once: `mdct_prev_tail` chaining (already has a *synthetic* multi-
   granule regression test, `transform_long_multi_granule_chaining_reconstructs`,
   but never exercised with the real filterbank + real psychoacoustic
   model + real quantizer together on dense content) and the bit
   reservoir's `main_data_begin: 0` hardcode (real gap, confirmed in this
   session's own reading of `encoder.rs` -- `reservoir.record_frame_usage`
   is called but its output never feeds back into what's written, so a
   frame that genuinely needs more bits than its nominal share has no
   escape valve; plausible quality-limiting factor for dense content in
   general, though not yet tied to the specific clipping symptom).
4. **Separately, and lower priority**: investigate why
   `chickens_16bit.wav` -- a percussive recording -- produces zero
   `BlockType::Short` granules out of 3360 (Part 1 above). Worth checking
   whether `PE_ATTACK_THRESHOLD_DB`/the tonality predictor's behavior
   changed since the last time this was measured firing, or whether it
   simply never applied to this specific file's dynamics.

Reproduction (broadband noise, the signal that most clearly exposes this
bug -- much more clearly than the real recording or a pure tone):

```bash
python3 -c "
import wave, struct, random
sr=44100; n=int(sr*6)
random.seed(42)
frames=[int(random.uniform(-0.6,0.6)*32767) for _ in range(n)]
w=wave.open('noise.wav','wb'); w.setnchannels(1); w.setsampwidth(2); w.setframerate(sr)
w.writeframes(struct.pack('<%dh'%n, *frames)); w.close()
"
cargo build --release -p mp3-cli
./target/release/encorust -b 192 -o noise.mp3 noise.wav
SKIP_IMAGES=1 ./compare_audio.sh noise.wav noise.mp3   # should show ~0.04 avg correlation
ffmpeg -y -i noise.mp3 -ac 1 -ar 44100 -c:a pcm_s16le noise_dec.wav
python3 -c "
import wave, numpy as np
w = wave.open('noise_dec.wav','rb'); n = w.getnframes()
data = np.frombuffer(w.readframes(n), dtype=np.int16).astype(np.float64)
print('peak idx', np.argmax(np.abs(data)), 'value', data[np.argmax(np.abs(data))])
"
```

### Session 5 (2026-08-30): delay recalibrated, granule correctly
### re-located, and it's *also* bit-perfect -- the bug isn't in the
### bitstream at all

Picked up this section's own item 1: re-locate the true source granule
before auditing further, since Session 4 had almost certainly audited
the wrong one.

**Delay recalibration**: the ~918-sample number quoted earlier in this
section came from a correlation-lag *search* on real content, which can
land near but not exactly on the true delay if quantization noise pulls
the optimum slightly. Measured the actual delay directly and
unambiguously instead: encoded a file that's silence except for one
near-full-scale single-sample impulse at a known index (50000), decoded
it, and found where the impulse's (window-spread) energy peaks in the
output. Result: **992 samples**, not 918 -- a real, measurable difference
between the two methods; 992 is the trustworthy one (calibrated against a
known input, not inferred from a correlation search over lossy content).

Applying the corrected delay to the noise file's clipped sample (decoded
index 1072): `1072 - 992 = 80`, i.e. **encoder sample 80, granule 0** --
not granule 1 as Session 4 assumed and spent real effort auditing (that
audit was real, valid work, just on the wrong granule).

**Audited granule 0 the same way** (hand-decode its `big_values` region,
which spans all three region tables `[12, 15, 12]` here, unlike granule
1's uniform `[12,12,12]`): first attempt read region boundaries directly
off the raw `SFB_LONG_BOUNDARIES` table using `region0_count`/
`region1_count` as plain indices, which **overran the declared bit
budget by 24 bits** (2248 decoded vs. 2224 declared) -- briefly looked
like a second, real, off-by-one bug in `RegionSplit::compute`'s
`r0_end`/`r1_end` derivation. **Caught before being reported**: traced
the actual Rust source's `huffman::encode::sf_band_end()` and found it
builds its `band_end` array as `SFB_LONG_BOUNDARIES[0][1..=22]` -- already
shifted by one relative to the raw boundary table. `band_end[region0_count]`
in the real code therefore already equals `SFB_LONG_BOUNDARIES[region0_count+1]`,
exactly matching the "correct" formula this same function's own doc
comment states. The bug was in this session's *reverse-engineering
script* (it read the unshifted table directly, missing that pre-existing
shift), not in the encoder. Re-decoding with the shift accounted for:
**2224 bits consumed, exactly matching the declared length**, max
decoded value 8 (matching the encoder's own intended `max_ix=8` for this
granule from Session 4's diagnostic dump). Granule 0 is bit-perfect too.

**Where this leaves things**: for frame 0 of the noise file, *both*
granules are now proven correct across every layer checked this
investigation -- side info fields, scalefactor values, region-boundary
derivation, Huffman table data (cross-checked against Symphonia), and the
actual `big_values` bit-decode (round-trips exactly, using the exact
declared bit count, for both granules). There is no remaining evidence of
a bitstream-encoding bug in this specific, representative granule. Two
false alarms were chased and correctly ruled out this investigation (the
Huffman-table LAME diff in Session 4, and this session's region-boundary
off-by-one) -- both looked like solid findings at first glance and both
turned out to be bugs in the hand-written verification scripts, not the
codebase. Worth remembering next time something "obviously" explains the
symptom: verify it against the actual running code's helper functions
(like `sf_band_end()`'s shift here), not just the data tables in
isolation.

**Where the next session should start (superseding the "delay-corrected
re-locate" item above, now done):**

1. **The bug is very likely in the decode-side reconstruction, not the
   encoded bitstream.** Everything checkable by hand-decoding the
   bitstream has checked out. The next productive step is almost
   certainly building (or reusing) a real IMDCT + overlap-add + synthesis
   polyphase filterbank reference implementation to trace *this exact
   granule's* `ix`/`scalefac`/`global_gain` values all the way through to
   PCM samples, and compare against what ffmpeg actually outputs at the
   same (delay-corrected) position -- rather than continuing to audit
   bitstream fields that have already checked out three times over.
   Symphonia (already vendored locally, MPL-2.0, already used for the
   Huffman-table cross-check above) is the natural source for that
   reference: its `layer3/requantize.rs` and `synthesis.rs` are real,
   independent, working implementations of exactly this chain.
2. **Alternative, cheaper first move**: instrument *Symphonia's own*
   decode of this exact MP3 (add temporary logging inside a local
   Cargo-patched copy, or drive it via its public API and inspect
   intermediate buffers where exposed) to see whether *it* also produces
   an anomalous/clipped sample at the same position. If Symphonia agrees
   with ffmpeg (independent confirmation, different codebase entirely),
   that's strong evidence the fault really is in what this project's
   *values* imply once correctly reconstructed (i.e. a genuine DSP/gain
   issue in how `global_gain`/`scalefac` combine, not a decoder quirk) --
   even though the granule's own recorded intent already looked
   reasonable in isolation. If Symphonia decodes it cleanly, the fault
   is decoder-side/ffmpeg-specific, which would be a very different and
   much smaller problem.
3. Items 3-4 from the previous "next session" list (cross-granule
   `mdct_prev_tail` state under dense content; the `main_data_begin: 0`
   reservoir bypass; why `chickens_16bit.wav` never triggers
   `BlockType::Short`) are all still open and unaffected by this
   session's work.

Delay recalibration reproduction (run this before trusting any future
"which granule" claim -- the number can depend on build/config, don't
reuse 992 blind either):

```bash
python3 -c "
import wave, struct
sr = 44100; n = sr * 3; impulse_idx = 50000
samples = [0] * n; samples[impulse_idx] = 32000
w = wave.open('impulse.wav', 'wb')
w.setnchannels(1); w.setsampwidth(2); w.setframerate(sr)
w.writeframes(struct.pack('<%dh' % n, *samples)); w.close()
"
./target/release/encorust -b 192 -o impulse.mp3 impulse.wav
ffmpeg -y -i impulse.mp3 -ac 1 -ar 44100 -c:a pcm_s16le impulse_dec.wav
python3 -c "
import wave, numpy as np
w = wave.open('impulse_dec.wav','rb'); n = w.getnframes()
data = np.frombuffer(w.readframes(n), dtype=np.int16).astype(np.float64)
peak = int(np.argmax(np.abs(data)))
print('delay =', peak - 50000, 'samples')
"
```

### Session 5, continued (same day): Symphonia cross-check — confirmed
### real, not an ffmpeg quirk

Implemented "where the next session should start" item 2 above. Added
`diag_noise_peak_cross_check` to `crates/mp3-cli/tests/symphonia_diff.rs`
(`#[ignore]`d, run explicitly): encodes ~6s of dense broadband noise
(xorshift32, 60% of full scale — same shape of signal as Session 4/5's
Python repro, generated in-process this time) via `mp3-core` directly,
then decodes the *identical* bytes two ways — in-process via Symphonia
(already a dev-dependency, reusing this file's existing
`decode_with_symphonia` helper) and out-of-process via `ffmpeg` (shelled
out, same as every other measurement this investigation) — and reports
both.

**Result: both independent decoders agree.** Symphonia's own peak lands
at exactly 1.0 (0 samples exceed it) while ffmpeg's int16-converted peak
also saturates at exactly ±1.0 (matching int16 clipping) — superficially
identical, but not informative by itself (both are just hitting each
decoder's own output-range ceiling). The real signal is in the *counts*
of how far reconstruction pushes samples beyond the source's actual 0.6
amplitude, which neither decoder's output format ceiling can hide:
**Symphonia: 8492 of 263808 samples exceed 0.6; ffmpeg: 8501 of 263825.**
Essentially identical (the ~9-sample difference is consistent with the
two decoders' independently-measured algorithmic delays not being
pixel-identical, not a real discrepancy). Symphonia's `f32` sample
conversion path was checked directly in its source
(`symphonia-core-0.5.5/src/conv.rs`,
`impl_convert!(f32, f32, s, s)`) and confirmed to be a bare identity with
no clamping, so this isn't Symphonia quietly hiding the same problem
behind a safety clamp -- it's a real, independent measurement.

**Conclusion: this is not a decoder quirk.** Two independent, unrelated
MP3 decoder implementations (ffmpeg's mp3float and Symphonia's pure-Rust
decoder) reconstruct essentially the same ~3.2% of samples in this file
too loud, from the same bytes. The fault is in what encoRust's bitstream
*means* once correctly reconstructed -- not in how any particular decoder
reads it. Combined with Session 5's earlier finding (the bitstream fields
themselves are transmitted exactly as intended, for both granules
checked), the remaining candidate is a **DSP/scaling issue in how the
encoder's chosen `global_gain`/quantization step relates to genuine
reconstructed energy specifically when most/all 576 lines in a granule
are simultaneously active** -- a regime a single dominant tone (which
the earlier MDCT-normalization fix, `0127039`, was validated against)
never exercises, but broadband noise (and to a lesser extent real
percussive/complex content) does constantly.

**Where the next session should start:**

1. **Compare encoRust's chosen `global_gain` against LAME's for the
   *same* dense content.** If LAME picks a visibly more conservative
   (coarser/higher) `global_gain` for equally-dense spectra, the bug is
   in the rate-loop/psychoacoustic model being too aggressive
   specifically when many bands compete at once (a tuning/algorithm
   issue in `quantize::loop_control` or the SMR feeding it) -- not a
   formula bug. If the gains are actually comparable, look at (2)
   instead.
2. **Re-derive, by hand, the expected reconstructed amplitude for one
   specific granule** (this session's already-audited granule 0 of frame
   0 is a good candidate -- `global_gain=173`, `scalefac` all zero,
   `ix` up to 8) all the way through IMDCT + overlap-add + the synthesis
   polyphase filterbank, using the *dequantization formula this crate
   documents* (`docs/mp3-encoder/08-phase5-quantization-loop.md` §2) and
   compare the hand-derived PCM magnitude against what Symphonia/ffmpeg
   actually produce for that exact time range. If the hand-derived value
   is already too large, the bug is a genuine formula/normalization
   mismatch (the same bug class as the already-fixed missing `2/N` MDCT
   factor, `0127039`) -- if it's *not* too large, the discrepancy is
   introduced somewhere between quantization and final PCM that this
   session never modeled by hand (the synthesis filterbank specifically,
   which no fix this whole investigation has touched).

Reproduction: `cargo test -p mp3-cli --release --test symphonia_diff diag_noise_peak_cross_check -- --ignored --nocapture`

### Session 5, continued again (same day): `global_gain` comparison vs
### LAME on the same content — not a magnitude problem, a *shaping* one

Implemented "where the next session should start" item 1 above: hand-
parsed side info across every frame (not just frame 0) of both
encoRust's and LAME's encode of the same noise file, comparing
`global_gain`/`scalefac_compress` distributions (230 frames / 460
granules for encoRust, 231/462 for LAME -- LAME's file has one extra
frame's worth from its Xing/info header, skipped during parsing).

**Average `global_gain` is comparable — this is not "encoRust picks a
finer/more aggressive gain than LAME":**
- encoRust: min=173, max=180, **avg=179.0**, and essentially constant
  (457 of 460 granules land on exactly 179; only 3 outliers).
- LAME: min=158, max=210, **avg=176.2**, spread across a 52-unit range
  with no single value dominating (top value, 175, is only 92 of 462).

**The real, well-evidenced difference is in *shaping*, not average
magnitude:**
- `scalefac_compress` (a proxy for how much per-band distortion shaping
  actually happened -- higher values mean wider scalefactor fields were
  needed, i.e. bigger differences between bands' scalefactors):
  encoRust averages **2.32**, LAME averages **5.43** -- more than double.
- encoRust picks essentially **one fixed `global_gain`, applied
  uniformly, for the entire file** on this content. LAME's gain
  *adapts frame-to-frame* (the 52-unit range) and shapes distortion
  *within* each granule far more (the compress average).

**Why this connects to the overshoot, and to something already flagged
early in this investigation**: `quantize::loop_control::quantize_granule`'s
outer loop only ever amplifies a band's scalefactor `if distort_e[b] >
allowed`, and `allowed = f32::MAX` (no ceiling at all, i.e. "anything
goes") whenever `smr.bands[b] <= 1.0`. Broadband, flat-spectrum noise is
exactly the content where a masking-ratio-based per-band criterion has
the least to say -- no band stands out tonally against its neighbors, so
SMR legitimately hovers near 1.0 across most/all bands, and the outer
loop's per-band shaping mechanism goes essentially dormant (matching the
`scalefac_compress=2.32` average -- barely any real shaping happened).
What's left running the show is `inner_loop` alone: a binary search for
the single finest *global* step that fits the entire nominal per-granule
bit budget, applied with zero per-band adaptation and zero margin,
identically, granule after granule, for as long as the input stays
statistically stationary (matching the `global_gain` staying pinned at
179 for the whole file). This is the same design property this
section's own much earlier notes flagged and then deprioritized
("revisit the rate loop's 'maximize precision to fill the entire bit
budget' design... real encoders drive the rate loop from an SMR/energy-
informed estimate and only coarsen from there") -- now with much
stronger, direct evidence specifically tying it to *this* symptom rather
than a general suspicion.

**Where the next session should start:**

1. **This is the strongest lead so far and the most actionable.** The
   `smr.bands[b] <= 1.0 -> allowed = f32::MAX` branch in
   `quantize_granule`'s outer loop effectively disables per-band
   distortion shaping for exactly the content that fails worst. Consider
   giving it a real (non-infinite) floor even when SMR sits at/under 1.0
   -- e.g. a small but finite allowed-distortion ceiling derived from the
   band's own signal energy, so broadband content still gets *some*
   per-band differentiation instead of one uniform global step repeated
   unchanged across the entire file. Verify against this session's own
   `diag_noise_peak_cross_check` (the "exceeds source amplitude" counts
   should drop) and the real-recording correlation numbers in
   `compare_audio.sh`, not just against a single metric.
2. If that alone doesn't fully resolve it, item 2 from the previous
   entry (hand-deriving the expected reconstructed amplitude through
   IMDCT + overlap-add + synthesis filterbank for one granule) is still
   open and would confirm whether *uniform* quantization noise
   specifically -- as opposed to shaped/textured noise -- is what
   produces the overshoot on reconstruction, which would explain
   *why* this particular gap matters mechanically, not just
   correlationally.

### Session 6 (2026-08-30): implemented item 1's fix — landed, tests
### pass, but empirically it changes *nothing* on the diagnostic file.
### Two hypotheses now disproven with direct evidence; the mechanism is
### neither the threshold formula nor the iteration budget

Implemented the fix proposed above: removed the `smr_value <= 1.0 ->
f32::MAX` special case in `quantize_granule`'s outer loop, replacing it
with the plain `allowed = (signal_e[b] + EPS) / smr_value` formula
unconditionally (safe unconditionally: the psychoacoustic model clamps
every SMR value to `[1.0, 1e6]` and default-inits to `1.0`, so
`smr_value` can never be `<= 0`). Full workspace test suite (145
mp3-core unit tests + everything else) stays green.

**Empirically: byte-identical output on the diagnostic noise file.**
Re-ran `diag_noise_peak_cross_check` before/after -- every number matches
exactly (8492/8501 samples exceeding 0.6, same peak, same everything).
Investigated why with a temporary diagnostic (dumped `smr_value`,
`signal_e`, `distort_e`, `allowed`, and their ratio, per band, per outer
iteration, for real noise-file granules; removed after use): **SMR never
actually sits at the clamp floor for this content** -- observed values
ranged from ~1.1 to over 40 across the first granule alone. The old
`> 1.0` branch was *already* taking the finite-formula path essentially
always; the special case this fix removed was already dead code for
this specific symptom. The hypothesis was reasonable but wrong about
the mechanism -- kept the fix anyway (it's still a real correctness
improvement: the formula now matches the standard unconditionally, with
no behavioral risk, given the guaranteed-safe SMR range), but it does
not explain or resolve the reported symptom.

**Second hypothesis, also tested and also disproven**: the same
diagnostic dump showed the outer loop's retry condition *does* trigger
sometimes (97 of ~3234 logged band/iteration combinations had
`distort_e > allowed`, up to 8x over in one case) -- so scalefactor
amplification isn't structurally inert. Hypothesized this might be an
iteration-budget problem: 22 bands competing simultaneously (unlike
sparse content, where only 1-2 ever compete) might need more than
`MAX_OUTER_ITERATIONS = 8` rounds to reach a well-shaped equilibrium,
especially since each amplified band forces `inner_loop` to pick a
*coarser* shared global step next round, which can itself push
previously-fine bands over their own threshold. Tested directly: bumped
`MAX_OUTER_ITERATIONS` to 60 (temporarily, reverted after the test) and
re-ran the same diagnostic -- **byte-identical output again.** The outer
loop reaches a genuine, non-iteration-limited fixed point well within 8
rounds; more rounds available doesn't change anything because there's
nothing left for them to do.

**Where this leaves things**: the outer loop's *control flow* (when to
retry, how many rounds it gets) is not the bug -- confirmed twice, by
direct experiment, not just by reasoning about the code. Whatever makes
`global_gain` land on one fixed value and `scalefac_compress` stay low
for this content is either (a) a genuine property of the psychoacoustic
model's *actual computed SMR/masking-threshold values* being too
permissive for broadband content specifically (as opposed to a control-
flow bug around them), or (b) entirely downstream of quantization, in
the DSP reconstruction chain (IMDCT + overlap-add + synthesis
filterbank) -- which nothing in this whole investigation has directly
exercised or verified by hand yet.

**Where the next session should start:**

1. **Hand-derive the expected reconstructed amplitude for one granule
   through the full chain** (IMDCT + overlap-add + synthesis polyphase
   filterbank), using the dequantization formula this crate documents
   (`docs/mp3-encoder/08-phase5-quantization-loop.md` §2), for granule 0
   of frame 0 (already fully characterized: `global_gain=173`, `scalefac`
   all zero, `ix` up to 8 -- see Session 5). Compare the hand-derived PCM
   magnitude against what Symphonia/ffmpeg actually produce for that
   time range. This is the same item 2 flagged at the end of Session 5's
   first entry, now the most direct remaining path since both control-
   flow hypotheses in the outer loop are ruled out.
2. **Alternatively**: instrument `PsychoacousticModel::analyze_granule`
   directly (not the quantizer) to check whether the *masking thresholds
   themselves* (`part_threshold`, before SMR division) are reasonable for
   broadband noise, or whether something in the spreading-function
   convolution or ATH floor produces an over-generous threshold
   specifically when energy is spread flat across every partition
   (unlike a tonal peak, which concentrates energy and produces a
   sharper, more restrictive threshold profile by construction).
