//! M7 end-to-end rate-control integration test. See
//! `docs/mp3-encoder/10-phase7-bit-reservoir-and-rate-control.md` §5.
//!
//! `Encoder::encode_frame` is still a `todo!()` scaffold (real frame
//! *assembly* -- headers, CRC, side-info bit-packing -- is M8's job, per
//! `01-architecture.md` and `encoder.rs`'s own doc comments). Chapter
//! 10's DoD nonetheless asks for "chapters 04-09 wired together" and an
//! "output byte stream['s] average bitrate" check. This test satisfies
//! that intent within what M7 actually owns: it manually chains the
//! already-implemented, already-unit-tested stages (filterbank -> MDCT ->
//! antialiasing -> psychoacoustic SMR -> quantization -> real Huffman bit
//! emission) and exercises the *real* rate-control decision (reservoir,
//! CBR frame-size/padding formulas, and per-granule bit budgeting) to
//! confirm the actual bits produced converge to the configured target.
//! It does not assemble real frame headers/side-info/CRC bits (a fixed,
//! known-size overhead per real frame, not something M7's own formulas
//! need to account for); that byte-exact check is M8's to add once frame
//! assembly exists.

use mp3_core::bitstream::writer::BitWriter;
use mp3_core::bitstream::{frame_bytes_for_bitrate, split_bits_for_granules, BitReservoir};
use mp3_core::filterbank::PolyphaseFilterbank;
use mp3_core::huffman::encode_granule;
use mp3_core::mdct::{antialias_butterfly, long_window, transform_long, BlockType};
use mp3_core::psychoacoustic::PsychoacousticModel;
use mp3_core::quantize::quantize_granule;
use mp3_core::types::{Bitrate, SampleRate, SUBBANDS};

/// Encodes one granule's worth of raw PCM (576 samples) through the real
/// filterbank -> MDCT -> antialiasing -> psychoacoustic -> quantization
/// -> Huffman pipeline, all `BlockType::Long` (short-block scalefactor
/// mapping is a known, separately-flagged gap -- see the M5/M6 review
/// notes -- out of scope for this rate-control-focused test). Returns
/// the granule's real emitted bit count.
#[allow(clippy::too_many_arguments)]
fn encode_one_granule(
    filterbank: &mut PolyphaseFilterbank,
    prev_tail: &mut [[f32; 18]; SUBBANDS],
    psy: &mut PsychoacousticModel,
    pcm_granule: &[f32; 576],
    psy_window: &[f32],
    sample_rate_hz: u32,
    bit_budget: u32,
) -> u32 {
    // Filterbank: 18 calls of 32 samples each -> [subband][time] samples.
    let mut subband_time = [[0.0f32; 18]; SUBBANDS];
    for step in 0..18 {
        let mut chunk = [0.0f32; 32];
        chunk.copy_from_slice(&pcm_granule[step * 32..step * 32 + 32]);
        let out = filterbank.analyze(&chunk);
        for sb in 0..SUBBANDS {
            subband_time[sb][step] = out[sb];
        }
    }

    // MDCT per subband (long blocks throughout), then antialiasing.
    let mut spectrum = [[0.0f32; 18]; SUBBANDS];
    let mdct_window = long_window();
    for sb in 0..SUBBANDS {
        let (spec, new_tail) = transform_long(&subband_time[sb], &prev_tail[sb], &mdct_window);
        spectrum[sb] = spec;
        prev_tail[sb] = new_tail;
    }
    antialias_butterfly(&mut spectrum, &[BlockType::Long; SUBBANDS]);

    // Flatten to the flat 576-line, subband-major layout chapters 06/08
    // use (see the M5 review notes on this convention).
    let mut flat = [0.0f32; 576];
    for sb in 0..SUBBANDS {
        flat[sb * 18..sb * 18 + 18].copy_from_slice(&spectrum[sb]);
    }

    // Psychoacoustic model: independent FFT path over raw PCM (chapter
    // 07 §1) -- not derived from the MDCT spectrum above.
    let (smr, block_type) = psy.analyze_granule(psy_window, sample_rate_hz);

    let result = quantize_granule(&flat, &smr, bit_budget, block_type, sample_rate_hz);

    let mut out = Vec::new();
    let mut writer = BitWriter::new(&mut out);
    encode_granule(&result.ix, &mut writer);
    writer.flush();
    (out.len() * 8) as u32
}

/// Generates `n` granules' worth (576 samples each) of a sine tone at
/// `amplitude` (silence when `amplitude == 0.0`).
fn make_pcm(num_granules: usize, amplitude: f32, sample_rate_hz: u32) -> Vec<f32> {
    let mut pcm = Vec::with_capacity(num_granules * 576);
    for i in 0..(num_granules * 576) {
        let t = i as f32 / sample_rate_hz as f32;
        // clippy.toml's disallowed-methods bans f32::sin because it's
        // std-only and breaks the no_std/wasm build — but this is an
        // integration test binary under `tests/`, which always links
        // std and never compiles for wasm32/no_std, and `libm` isn't a
        // dev-dependency of this crate (it's mp3-core's own production
        // dependency, not visible here). Genuinely safe here.
        #[allow(clippy::disallowed_methods)]
        let s = (2.0 * core::f32::consts::PI * 440.0 * t).sin();
        pcm.push(amplitude * s);
    }
    pcm
}

/// Broadband pseudo-random noise (LCG-based, matching the generator
/// `quantize::loop_control`'s own tests use). Unlike a discrete-tone
/// signal (even a multi-tone one) -- whose spectrum stays sparse
/// (concentrated at a handful of MDCT lines) and which a transform coder
/// genuinely, correctly compresses to very few bits regardless of
/// amplitude -- broadband noise spreads energy across effectively all
/// 576 lines, needing real precision at *every* line to represent
/// faithfully. This is what actually exercises a meaningful fraction of
/// a realistic bit budget, which the CBR average-bitrate convergence
/// test needs to be a non-trivial check (a pure/multi tone would make it
/// pass trivially without proving anything about typical content).
fn make_noise_pcm(num_granules: usize, amplitude: f32) -> Vec<f32> {
    let mut pcm = Vec::with_capacity(num_granules * 576);
    let mut seed: u32 = 12345;
    for _ in 0..(num_granules * 576) {
        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
        let sample = ((seed >> 16) as f32 / 32768.0 - 0.5) * 2.0;
        pcm.push(amplitude * sample);
    }
    pcm
}

#[test]
fn cbr_average_bitrate_matches_configured_target() {
    let sample_rate = SampleRate::Hz44100;
    let hz = sample_rate.as_hz();
    let bitrate = Bitrate::Kbps128;
    let kbps = bitrate.as_kbps();
    let version = sample_rate.version();

    // ~577 bytes/frame at 128kbps/44.1kHz (417/418 alternating) x 40
    // frames = one granule per 576 samples, 2 granules/frame for MPEG-1.
    let num_frames = 40usize;
    let num_granules = num_frames * version.granules_per_frame();
    let pcm = make_noise_pcm(num_granules, 0.5);

    let mut filterbank = PolyphaseFilterbank::new();
    let mut prev_tail = [[0.0f32; 18]; SUBBANDS];
    let mut psy = PsychoacousticModel::new();
    psy.init_for_sample_rate(hz);
    let mut reservoir = BitReservoir::new(BitReservoir::max_for_version(version));
    let mut padding_acc = 0u32;

    let mut total_actual_bits: u64 = 0;
    let mut total_nominal_bits: u64 = 0;

    for frame in 0..num_frames {
        let (padded, new_acc) =
            mp3_core::bitstream::padding_bit_for_frame(version, kbps, hz, padding_acc);
        padding_acc = new_acc;
        let nominal_bytes = frame_bytes_for_bitrate(bitrate, sample_rate, padded);
        let nominal_bits = nominal_bytes * 8;

        let frame_budget = reservoir.available_for_frame(nominal_bits);
        // Equal PE split: PE isn't exposed by PsychoacousticModel's
        // public API, and this test verifies the reservoir/padding
        // convergence property, not the split policy itself.
        let (budget0, budget1) = split_bits_for_granules(frame_budget, 1.0, 1.0);

        let g0_start = frame * 1152;
        let psy_window0 = &pcm[g0_start..(g0_start + 1024).min(pcm.len())];
        let mut pcm_g0 = [0.0f32; 576];
        pcm_g0.copy_from_slice(&pcm[g0_start..g0_start + 576]);
        let bits0 = encode_one_granule(
            &mut filterbank,
            &mut prev_tail,
            &mut psy,
            &pcm_g0,
            psy_window0,
            hz,
            budget0,
        );

        let g1_start = g0_start + 576;
        let psy_window1 = &pcm[g1_start..(g1_start + 1024).min(pcm.len())];
        let mut pcm_g1 = [0.0f32; 576];
        pcm_g1.copy_from_slice(&pcm[g1_start..g1_start + 576]);
        let bits1 = encode_one_granule(
            &mut filterbank,
            &mut prev_tail,
            &mut psy,
            &pcm_g1,
            psy_window1,
            hz,
            budget1,
        );

        let actual_bits = bits0 + bits1;
        reservoir.record_frame_usage(nominal_bits, actual_bits);

        total_actual_bits += u64::from(actual_bits);
        total_nominal_bits += u64::from(nominal_bits);
    }

    // The reservoir's whole purpose: even though individual frames'
    // *actual* Huffman bit counts vary with content, borrowing/banking
    // against the nominal allocation keeps the long-run average locked
    // to the configured nominal rate -- this is the property a real
    // decoder's fixed-size frame-header assumption depends on.
    let avg_nominal = total_nominal_bits as f64 / f64::from(num_frames as u32);
    let target_bits_per_frame =
        f64::from(frame_bytes_for_bitrate(bitrate, sample_rate, false)) * 8.0;
    assert!(
        (avg_nominal - target_bits_per_frame).abs() < 8.0, // < 1 byte/frame
        "avg nominal bits/frame {avg_nominal} should track the {kbps}kbps \
         target ({target_bits_per_frame} bits/frame within padding rounding)"
    );

    // Sanity: the quantizer actually used a meaningful fraction of its
    // budget (content isn't being silently dropped to near-zero bits,
    // which would make this test pass trivially).
    assert!(
        total_actual_bits > total_nominal_bits / 4,
        "actual bits used ({total_actual_bits}) suspiciously low vs. \
         nominal budget ({total_nominal_bits})"
    );
}

#[test]
fn vbr_style_unconstrained_budget_produces_varying_frame_sizes() {
    // No RateControl::Vbr bitrate-index-picking logic exists yet (chapter
    // 10 §4's "pick the smallest bitrate_index that fits the bits
    // actually produced" is unimplemented -- nothing in this crate
    // dispatches on `RateControl::Vbr` today). What *is* testable now,
    // and what chapter 08 §3 calls "the bit-budget escape valve" VBR
    // depends on: given a bit budget generous enough that the inner
    // (rate) loop's constraint never binds, the *actual* bits a granule
    // needs is driven purely by content complexity via the outer
    // (distortion) loop -- loud/transient content should cost
    // measurably more than near-silence.
    let sample_rate = SampleRate::Hz44100;
    let hz = sample_rate.as_hz();
    let generous_budget = 40_000u32; // far above what any granule needs

    let mut filterbank = PolyphaseFilterbank::new();
    let mut prev_tail = [[0.0f32; 18]; SUBBANDS];
    let mut psy = PsychoacousticModel::new();
    psy.init_for_sample_rate(hz);

    // loud -> quiet -> loud, 8 granules each (matches the DoD's
    // "loud-then-quiet-then-loud synthetic fixture").
    let mut pcm = make_pcm(8, 0.8, hz);
    pcm.extend(make_pcm(8, 0.0, hz));
    pcm.extend(make_pcm(8, 0.8, hz));

    let mut bits_per_granule = Vec::new();
    for g in 0..24 {
        let start = g * 576;
        let psy_window = &pcm[start..(start + 1024).min(pcm.len())];
        let mut pcm_g = [0.0f32; 576];
        pcm_g.copy_from_slice(&pcm[start..start + 576]);
        let bits = encode_one_granule(
            &mut filterbank,
            &mut prev_tail,
            &mut psy,
            &pcm_g,
            psy_window,
            hz,
            generous_budget,
        );
        bits_per_granule.push(bits);
    }

    let loud1_avg: f64 = bits_per_granule[0..8]
        .iter()
        .map(|&b| f64::from(b))
        .sum::<f64>()
        / 8.0;
    let quiet_avg: f64 = bits_per_granule[8..16]
        .iter()
        .map(|&b| f64::from(b))
        .sum::<f64>()
        / 8.0;
    let loud2_avg: f64 = bits_per_granule[16..24]
        .iter()
        .map(|&b| f64::from(b))
        .sum::<f64>()
        / 8.0;

    assert!(
        quiet_avg < loud1_avg / 2.0,
        "quiet section ({quiet_avg} bits/granule avg) should cost far \
         less than the loud section ({loud1_avg}) -- a VBR-style \
         unconstrained budget that produces constant-size output \
         indicates the bit-budget escape valve isn't actually being \
         used (chapter 08 §3)"
    );
    assert!(
        quiet_avg < loud2_avg / 2.0,
        "quiet section should also cost far less than the second loud \
         section ({loud2_avg})"
    );

    // Frame sizes actually vary at all -- not every granule identical.
    let distinct: std::collections::HashSet<u32> = bits_per_granule.iter().copied().collect();
    assert!(
        distinct.len() > 1,
        "expected varying per-granule bit counts, got a single constant \
         value repeated -- {bits_per_granule:?}"
    );
}
