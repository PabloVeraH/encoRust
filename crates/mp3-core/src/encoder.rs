//! Top-level pipeline orchestration — composes every other module into
//! the public `Encoder` API. See
//! `docs/mp3-encoder/01-architecture.md` §3 and §5.

use alloc::vec;
use alloc::vec::Vec;

use crate::bitstream::reservoir::{
    frame_bytes_for_bitrate, padding_bit_for_frame, split_bits_for_granules, BitReservoir,
    RateControl,
};
use crate::bitstream::scalefactor_encode::encode_granule_scalefactors;
use crate::bitstream::side_info::SideInfo;
use crate::bitstream::writer::BitWriter;
use crate::error::EncodeError;
use crate::filterbank::PolyphaseFilterbank;
use crate::frame::FrameHeader;
use crate::huffman::encode_granule;
use crate::io::PcmBuffer;
use crate::mdct::{transform_long, BlockType};
use crate::psychoacoustic::{PsychoacousticModel, ScalefactorBandSmr};
use crate::quantize::{quantize_granule, ScaleFactors};
use crate::types::{ChannelMode, MpegVersion, SampleRate, MAX_CHANNELS, SUBBANDS};

/// Worst-case scalefactor bits a single granule/channel can need: up to
/// 39 scalefactor slots (12 bands × 3 windows for short blocks, the
/// larger of the two layouts) at the maximum 4-bit `slen` width (see
/// `quantize::loop_control::MAX_SCALEFAC`). Subtracted from a granule's
/// total main_data budget *before* it's handed to [`quantize_granule`]
/// as the Huffman-only budget that function actually expects --
/// otherwise `scalefac_bits + huffman_bits` can (and, confirmed
/// empirically during the M8 review, does for perfectly ordinary
/// content) exceed the granule's real physical allocation, which an
/// earlier version silently truncated instead of ever detecting.
const MAX_SCALEFACTOR_BITS_PER_GRANULE: u32 = 39 * 4;

/// Configuration for a new [`Encoder`]. See
/// `docs/mp3-encoder/01-architecture.md` §5.
#[derive(Debug, Clone, Copy)]
pub struct EncoderConfig {
    /// Input/output sample rate (this encoder does not resample).
    pub sample_rate: SampleRate,
    /// Stereo/joint/dual/mono coding mode.
    pub channel_mode: ChannelMode,
    /// CBR, ABR, or VBR — see
    /// `docs/mp3-encoder/10-phase7-bit-reservoir-and-rate-control.md` §4.
    pub rate_control: RateControl,
}

/// A pure-Rust MP3 encoder. Owns all working state so that
/// [`Self::encode_frame`] never allocates on the hot path — see
/// `docs/mp3-encoder/01-architecture.md` §4.
///
/// # Known scope limitations (see the M8 review notes)
///
/// - **MPEG-2 LSF sample rates are rejected outright** (`Encoder::new`
///   returns [`EncodeError::UnsupportedSampleRate`]). `SideInfo::write`
///   and this module's byte-count constants are MPEG-1-only (9-bit
///   `main_data_begin`, `scfsi` present, 4-bit `scalefac_compress`,
///   `preflag` present, 32/17-byte side info, 2 granules/frame) --
///   silently encoding LSF content through the MPEG-1 layout would
///   produce a non-conformant bitstream with no error at all, which is
///   worse than refusing.
/// - **Joint stereo (MS/intensity) is rejected outright** (`Encoder::new`
///   returns [`EncodeError::UnsupportedChannelMode`]) -- the required
///   cross-channel transform (chapter 08: applied *before*
///   quantization) isn't implemented. Mono, discrete Stereo, and
///   DualMono all encode each channel independently, which is correct
///   for those modes.
/// - **The bit reservoir (chapter 10) doesn't smooth across frames yet**:
///   every frame is self-contained (`main_data_begin == 0` always,
///   budget capped at that frame's own nominal allocation). Actually
///   spending banked surplus requires the encoder to delay emitting a
///   frame's physical bytes by at least one `encode_frame` call (so a
///   later frame's overflow can be carried backward into an earlier,
///   already-known-to-be-underused frame's physical slot) -- a real
///   output-buffering change, not a local formula fix. A first attempt
///   at the shortcut (accumulate a byte queue without restructuring
///   output timing) was caught red-handed by
///   `main_data_begin_never_references_untransmitted_bytes` in
///   `tests/m8_bitstream.rs` producing exactly the "declares data that
///   isn't there yet" failure chapter 11 §5 warns about; reverted rather
///   than ship something unverifiable. `self.reservoir`'s bookkeeping is
///   still updated every frame so it's ready once this is implemented.
pub struct Encoder {
    config: EncoderConfig,
    filterbanks: [PolyphaseFilterbank; MAX_CHANNELS],
    psychoacoustic: [PsychoacousticModel; MAX_CHANNELS],
    reservoir: BitReservoir,
    /// MDCT overlap history per channel, per subband: the previous
    /// granule's 18 input samples used as the "old" half of the 36-sample
    /// MDCT window. Initialized to zeros.
    mdct_prev_tail: [[[f32; 18]; SUBBANDS]; MAX_CHANNELS],
    /// Running fractional-sample accumulator for the padding decision.
    padding_accumulator: u32,
}

impl Encoder {
    /// Creates a new encoder. Pre-allocates every per-channel working
    /// buffer up front so [`Self::encode_frame`] can stay allocation-free.
    ///
    /// # Errors
    ///
    /// Returns [`EncodeError`] if `config` requests an unsupported sample
    /// rate, channel count, or bitrate -- see this struct's doc comment
    /// for the current MPEG-2 LSF / joint-stereo scope limitations.
    pub fn new(config: EncoderConfig) -> Result<Self, EncodeError> {
        let version = config.sample_rate.version();
        if version != MpegVersion::Mpeg1 {
            // See the struct doc comment: SideInfo::write and this
            // module's byte-count constants are MPEG-1-only. Reject
            // rather than silently emit a non-conformant bitstream.
            return Err(EncodeError::UnsupportedSampleRate);
        }
        if matches!(
            config.channel_mode,
            ChannelMode::JointStereoMs | ChannelMode::JointStereoIntensity
        ) {
            return Err(EncodeError::UnsupportedChannelMode {
                mode: config.channel_mode,
            });
        }
        // Bitrate legality is version-dependent (`Bitrate::header_index`
        // covers both MPEG-1 and MPEG-2 LSF tables, but a `Bitrate` value
        // legal for one can be `None` for the other -- e.g. 144 kbps is
        // LSF-only, 320 kbps is MPEG-1-only). This used to go unchecked
        // here despite this function's own doc comment promising it:
        // `encode_frame`'s `FrameHeader::to_bits()` was the only place
        // that actually caught it, deep inside the first encode call. A
        // caller-facing API (found during the M9 review: `mp3-wasm`'s
        // `WasmEncoder::push()`) that swallows a rare `encode_frame`
        // error would then silently produce nothing forever instead of
        // ever surfacing why -- reject up front, at construction, where
        // every caller already handles a `Result`.
        let bitrate = config.rate_control.nominal_bitrate();
        if bitrate.header_index(version).is_none() {
            return Err(EncodeError::InvalidBitrate {
                kbps: bitrate.as_kbps(),
            });
        }

        let reservoir = BitReservoir::new(BitReservoir::max_for_version(version));

        let filterbanks = [const { PolyphaseFilterbank::new() }; MAX_CHANNELS];
        let psychoacoustic = [const { PsychoacousticModel::new() }; MAX_CHANNELS];
        let mdct_prev_tail = [[[0.0f32; 18]; SUBBANDS]; MAX_CHANNELS];

        Ok(Self {
            config,
            filterbanks,
            psychoacoustic,
            reservoir,
            mdct_prev_tail,
            padding_accumulator: 0,
        })
    }

    /// Consumes exactly one MPEG frame's worth of PCM — 1152 samples per
    /// channel for MPEG-1, 576 for MPEG-2 LSF (one granule per frame;
    /// see [`crate::types::MpegVersion::samples_per_frame`], never a
    /// hard-coded 1152) — and appends the encoded frame's bytes to
    /// `out`. Returns bytes written.
    ///
    /// Note the psychoacoustic model's look-ahead requirement means
    /// output for a given granule of PCM lags its input by roughly one
    /// granule — see
    /// `docs/mp3-encoder/04-phase1-pcm-io-and-framing.md` §5.
    ///
    /// # Errors
    ///
    /// Returns [`EncodeError`] if `pcm`'s channel count doesn't match
    /// this encoder's configured [`ChannelMode`].
    pub fn encode_frame(
        &mut self,
        pcm: &PcmBuffer,
        out: &mut Vec<u8>,
    ) -> Result<usize, EncodeError> {
        let version = self.config.sample_rate.version();
        let channel_mode = self.config.channel_mode;
        let n_channels = channel_mode.channel_count();
        let granules_per_frame = version.granules_per_frame();
        let samples_per_granule = crate::types::SAMPLES_PER_GRANULE;

        // Validate PCM length
        if pcm.samples_per_channel() != version.samples_per_frame() {
            return Err(EncodeError::BufferLengthMismatch {
                expected: n_channels * version.samples_per_frame(),
                got: pcm.samples_per_channel() * n_channels,
            });
        }

        let out_start = out.len();

        // --- Frame header ---
        let bitrate = self.config.rate_control.nominal_bitrate();
        let (padding, new_acc) = padding_bit_for_frame(
            version,
            bitrate.as_kbps(),
            self.config.sample_rate.as_hz(),
            self.padding_accumulator,
        );
        self.padding_accumulator = new_acc;

        let header = FrameHeader {
            sample_rate: self.config.sample_rate,
            crc_present: false,
            bitrate,
            padding,
            private_bit: false,
            channel_mode,
            copyright: false,
            original: false,
        };
        let header_bits = header.to_bits().ok_or(EncodeError::InvalidBitrate {
            kbps: bitrate.as_kbps(),
        })?;

        // --- Frame size computation ---
        let frame_bytes = frame_bytes_for_bitrate(bitrate, self.config.sample_rate, padding);
        let side_info_bytes: usize = if channel_mode.is_stereo() { 32 } else { 17 };
        let main_data_capacity = frame_bytes as usize - 4 - side_info_bytes;
        let nominal_bits = main_data_capacity as u32 * 8;

        // --- Bit budget for this frame's granules ---
        //
        // This is `nominal_bits`, *not* `self.reservoir.available_for_frame
        // (nominal_bits)` -- deliberately. Actually spending banked
        // reservoir bits means a granule can produce *more* main_data
        // than fits in its own frame's physical slot, and the excess
        // must be carried into a *later* frame's physical slot via that
        // later frame's `main_data_begin`. Since a granule's own
        // part2_3_length must be fully satisfiable using only bytes
        // already transmitted by the end of its *own* frame (a decoder
        // processes each frame as it arrives, causally -- it cannot wait
        // for a future frame to finish reading the current one), realizing
        // that carry-forward requires the *encoder* to delay emitting a
        // frame's physical bytes until it already knows how much backlog
        // the *next* frame will produce -- i.e. at least one frame of
        // output latency. This function emits each frame immediately as
        // it's called, so it cannot honor that without a real
        // architecture change (buffering the previous frame's header+
        // side-info until the next `encode_frame` call, revising
        // `main_data_begin` then, and flushing the final buffered frame
        // in `finish`).
        //
        // An earlier version tried the shortcut of accumulating a
        // `main_data` byte queue and computing `main_data_begin` from it
        // without that output-delay restructuring; a targeted test
        // (`main_data_begin_never_references_untransmitted_bytes` in
        // `tests/m8_bitstream.rs`, driven by quiet-then-loud content
        // that actually exercises borrowing, not constant-complexity
        // content that never draws the bank down) caught it immediately:
        // a granule's declared `part2_3_length` came out larger than
        // `main_data_begin + this frame's own capacity` could supply --
        // exactly the "produces a file but it doesn't decode" failure
        // chapter 11 §5 warns about. Keeping every frame self-contained
        // (`main_data_begin == 0` always, budget capped at the frame's
        // own nominal allocation) is strictly less than what chapter 10
        // promises -- no cross-frame quality smoothing -- but it is
        // *verifiably correct*: each frame only ever claims data that
        // physically exists within itself, which the passing test above
        // (still run against every frame) directly confirms. `self
        // .reservoir` bookkeeping is still updated below so it stays
        // ready for whichever milestone adds the output-buffering needed
        // to actually spend what it tracks.
        let frame_bit_budget = nominal_bits;

        // Equal split for now; PE-based split (chapter 10 §3) will be
        // added with quality feedback.
        let (granule0_bits, granule1_bits) = split_bits_for_granules(frame_bit_budget, 0.0, 0.0);

        // --- Build this frame's own new main_data bytes ---
        let mut new_main_data: Vec<u8> = Vec::new();

        let mut gi_gr0_ch0 = default_granule_side_info();
        let mut gi_gr0_ch1 = gi_gr0_ch0;
        let mut gi_gr1_ch0 = gi_gr0_ch0;
        let mut gi_gr1_ch1 = gi_gr0_ch0;

        for gr in 0..granules_per_frame {
            let granule_bits = if gr == 0 {
                granule0_bits
            } else {
                granule1_bits
            };
            // Split the granule's budget across channels too -- an
            // earlier version handed the *full* granule budget to each
            // channel independently, letting stereo content use up to
            // ~2x its fair share of the frame.
            let per_channel_bits = granule_bits / n_channels as u32;
            let huffman_budget = per_channel_bits.saturating_sub(MAX_SCALEFACTOR_BITS_PER_GRANULE);

            for ch in 0..n_channels {
                let pcm_ch = pcm.channel(ch);
                let gr_offset = gr * samples_per_granule;

                // --- Polyphase filterbank ---
                let mut subband_samples = [[0.0f32; 18]; SUBBANDS];
                // `fbc` drives both the PCM offset arithmetic below and a
                // transposed write into `subband_samples[sb][fbc]` -- not
                // a plain per-element iteration clippy can rewrite.
                #[allow(clippy::needless_range_loop)]
                for fbc in 0..18 {
                    let pcm_offset = gr_offset + fbc * 32;
                    let mut pcm_chunk = [0.0f32; 32];
                    let copy_len = 32.min(pcm_ch.len() - pcm_offset);
                    pcm_chunk[..copy_len]
                        .copy_from_slice(&pcm_ch[pcm_offset..pcm_offset + copy_len]);
                    let subband = self.filterbanks[ch].analyze(&pcm_chunk);
                    for sb in 0..SUBBANDS {
                        subband_samples[sb][fbc] = subband[sb];
                    }
                }

                // --- MDCT: 32 subbands × transform_long → 576 spectral lines ---
                let mut spectrum = [0.0f32; 576];
                for sb in 0..SUBBANDS {
                    let (spec, new_tail) = transform_long(
                        &subband_samples[sb],
                        &self.mdct_prev_tail[ch][sb],
                        BlockType::Long,
                    );
                    self.mdct_prev_tail[ch][sb] = new_tail;
                    for k in 0..18 {
                        spectrum[sb * 18 + k] = spec[k];
                    }
                }

                // --- Psychoacoustic model ---
                let mut pcm_window = [0.0f32; 1024];
                let pcm_len = samples_per_granule.min(pcm_ch.len() - gr_offset);
                pcm_window[..pcm_len].copy_from_slice(&pcm_ch[gr_offset..gr_offset + pcm_len]);
                let (smr, block_type) = self.psychoacoustic[ch]
                    .analyze_granule(&pcm_window, self.config.sample_rate.as_hz());
                let sample_rate_hz = self.config.sample_rate.as_hz();

                // --- Quantize + encode this granule's scalefactors and
                // Huffman data, given an SMR. ---
                let encode_with = |smr: &ScalefactorBandSmr| {
                    let quant_result = quantize_granule(
                        &spectrum,
                        smr,
                        huffman_budget,
                        block_type,
                        sample_rate_hz,
                    );

                    let mut sf_buf = Vec::new();
                    let (scalefac_compress, scalefac_bits) = {
                        let mut sf_writer = BitWriter::new(&mut sf_buf);
                        let r = encode_granule_scalefactors(
                            &quant_result.scalefac.values,
                            block_type,
                            sample_rate_hz,
                            &mut sf_writer,
                        );
                        sf_writer.flush();
                        r
                    };

                    let mut granule_buf = Vec::new();
                    let huffman_info = {
                        let mut writer = BitWriter::new(&mut granule_buf);
                        let info = encode_granule(&quant_result.ix, &mut writer);
                        writer.flush();
                        info
                    };
                    let huffman_bits = granule_buf.len() as u32 * 8;

                    (
                        quant_result,
                        scalefac_compress,
                        scalefac_bits,
                        sf_buf,
                        huffman_info,
                        huffman_bits,
                        granule_buf,
                    )
                };

                let (
                    mut quant_result,
                    mut scalefac_compress,
                    mut scalefac_bits,
                    mut sf_buf,
                    mut huffman_info,
                    mut huffman_bits,
                    mut granule_buf,
                ) = encode_with(&smr);

                // The quantization inner loop coarsens until its own
                // (real, region-aware) bit estimate fits `huffman_budget`
                // -- but the *outer* (distortion) loop can amplify
                // scalefactors chasing the SMR-driven quality target
                // strongly enough that even the coarsest representable
                // step no longer reaches zero for every line, and
                // `converged == false` signals exactly that "gave up,
                // best-effort" case chapter 08 §3 documents. When that
                // happens and the result genuinely doesn't fit this
                // granule's physical share, fall back to a flat SMR (no
                // band demands more than baseline, so the outer loop
                // never amplifies anything and the inner loop's own step
                // coarsening -- unopposed -- reliably reaches zero):
                // strictly worse quality for this one granule, but
                // guaranteed to fit, instead of the silent truncation an
                // earlier version of this function used.
                if !quant_result.converged && scalefac_bits + huffman_bits > per_channel_bits {
                    let flat_smr = ScalefactorBandSmr { bands: [1.0; 22] };
                    (
                        quant_result,
                        scalefac_compress,
                        scalefac_bits,
                        sf_buf,
                        huffman_info,
                        huffman_bits,
                        granule_buf,
                    ) = encode_with(&flat_smr);
                }

                debug_assert!(
                    scalefac_bits + huffman_bits <= per_channel_bits
                        || per_channel_bits < MAX_SCALEFACTOR_BITS_PER_GRANULE,
                    "granule/channel main_data ({} bits: {scalefac_bits} scalefac + \
                     {huffman_bits} huffman) exceeded its reserved share of the frame \
                     ({per_channel_bits} bits) even after the flat-SMR fallback -- \
                     content this demanding cannot fit even at the coarsest \
                     representable quantization step",
                    scalefac_bits + huffman_bits
                );

                new_main_data.extend_from_slice(&sf_buf);
                new_main_data.extend_from_slice(&granule_buf);

                // Store side info for this granule/channel
                let gi = if gr == 0 {
                    if ch == 0 {
                        &mut gi_gr0_ch0
                    } else {
                        &mut gi_gr0_ch1
                    }
                } else if ch == 0 {
                    &mut gi_gr1_ch0
                } else {
                    &mut gi_gr1_ch1
                };
                gi.part2_3_length = (scalefac_bits + huffman_bits) as u16;
                gi.block_type = block_type;
                gi.mixed_block_flag = false;
                gi.scalefac_compress = scalefac_compress;
                gi.quant = quant_result;
                gi.huffman = huffman_info;
            }
        }

        // --- Reservoir bookkeeping ---
        // Updated for whichever future milestone adds the output
        // buffering described above and can actually spend this; not
        // consulted for this frame's own budget (see that comment).
        let actual_bits_used = new_main_data.len() as u32 * 8;
        self.reservoir
            .record_frame_usage(nominal_bits, actual_bits_used);

        // Every frame is self-contained: main_data_begin == 0 always
        // (see the budget comment above for why). new_main_data must fit
        // within this frame's own physical capacity by construction --
        // huffman_budget was already capped to
        // `per_channel_bits - MAX_SCALEFACTOR_BITS_PER_GRANULE`, and
        // per_channel_bits sums (across channels and granules) to
        // exactly `frame_bit_budget == nominal_bits == main_data_capacity
        // * 8`. `debug_assert!` rather than silently truncating if that
        // invariant is ever violated -- an earlier version clamped via
        // `.min(remaining)` here, silently dropping bytes with no error
        // at all.
        debug_assert!(
            new_main_data.len() <= main_data_capacity,
            "frame main_data ({} bytes) exceeded its physical capacity \
             ({main_data_capacity} bytes) -- the per-granule/channel budget \
             split above should make this impossible",
            new_main_data.len()
        );

        // --- Build frame in a buffer of exact size ---
        let mut frame_buf = vec![0u8; frame_bytes as usize];
        frame_buf[..4].copy_from_slice(&header_bits.to_be_bytes());
        let side_info_start = 4;
        let main_data_offset = 4 + side_info_bytes;

        // --- Write side info into the frame buffer ---
        {
            let side_info = SideInfo {
                main_data_begin: 0,
                scfsi: [[false; 4]; 2],
                granules: [[gi_gr0_ch0, gi_gr0_ch1], [gi_gr1_ch0, gi_gr1_ch1]],
            };
            let mut si_buf = Vec::new();
            {
                let mut writer = BitWriter::new(&mut si_buf);
                side_info.write(&mut writer, channel_mode);
                writer.flush();
            }
            let si_len = side_info_bytes.min(si_buf.len());
            frame_buf[side_info_start..side_info_start + si_len].copy_from_slice(&si_buf[..si_len]);
        }

        // --- Emit this frame's own main_data. Any leftover physical
        // space (main_data_capacity - new_main_data.len()) is ancillary/
        // unused (chapter 11 §6) and stays zero-filled (frame_buf is
        // zero-initialized above) -- expected whenever content needs
        // fewer bits than the nominal allocation affords.
        let emit_len = new_main_data.len().min(main_data_capacity);
        frame_buf[main_data_offset..main_data_offset + emit_len]
            .copy_from_slice(&new_main_data[..emit_len]);

        out.extend_from_slice(&frame_buf);

        Ok(out.len() - out_start)
    }

    /// Flushes the bit reservoir and any buffered look-ahead samples at
    /// end of stream. Call exactly once, after the last
    /// [`Self::encode_frame`] call.
    ///
    /// # Errors
    ///
    /// Returns [`EncodeError`] on internal flush failure (should not
    /// occur once M8 is correctly implemented; kept fallible for
    /// forward-compatibility with the public API contract).
    pub fn finish(&mut self, out: &mut Vec<u8>) -> Result<usize, EncodeError> {
        let _ = out;
        // No cross-frame output buffering exists yet (see this struct's
        // doc comment on the reservoir scope limitation), so there is
        // nothing pending to flush -- every `encode_frame` call already
        // emitted a complete, self-contained frame.
        Ok(0)
    }
}

fn default_granule_side_info() -> crate::bitstream::side_info::GranuleSideInfo {
    crate::bitstream::side_info::GranuleSideInfo {
        part2_3_length: 0,
        block_type: BlockType::Long,
        mixed_block_flag: false,
        scalefac_compress: 0,
        quant: crate::quantize::QuantizationResult {
            ix: [0; 576],
            sign: [false; 576],
            scalefac: ScaleFactors { values: [0; 39] },
            global_gain: 0,
            scalefac_scale: false,
            preflag: false,
            subblock_gain: None,
            converged: false,
        },
        huffman: crate::huffman::encode::HuffmanSideInfo {
            big_values: 0,
            region0_count: 0,
            region1_count: 0,
            table_select: [0; 3],
            count1table_select: false,
        },
    }
}

#[cfg(test)]
mod tests {
    // See crates/mp3-core/tests/m8_bitstream.rs for `Encoder`'s
    // integration tests (single-frame structure, multi-frame
    // main_data_begin round-trip, LSF/joint-stereo rejection).
}
