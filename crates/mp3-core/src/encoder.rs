//! Top-level pipeline orchestration — composes every other module into
//! the public `Encoder` API. See
//! `docs/mp3-encoder/01-architecture.md` §3 and §5.

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
use crate::mdct::antialias_butterfly;
use crate::mdct::long_window;
use crate::mdct::long_window_for_kind;
use crate::mdct::reorder_short;
use crate::mdct::transform_long;
use crate::mdct::transform_short;
use crate::mdct::BlockType;
use crate::mdct::GranuleShape;
use crate::mdct::LongWindowKind;
use crate::psychoacoustic::{
    scalefactor_sample_rate_index, PsychoacousticModel, ScalefactorBandSmr, SFB_SHORT_BOUNDARIES,
    SFB_SHORT_COUNTS,
};
use crate::quantize::{quantize_granule, ScaleFactors};
use crate::types::{ChannelMode, MpegVersion, SampleRate, MAX_CHANNELS, SUBBANDS};

/// Worst-case scalefactor bits a single granule/channel can need.
const MAX_SCALEFACTOR_BITS_PER_GRANULE: u32 = 39 * 4;

/// Maximum frame size in bytes for any legal MPEG-1 CBR configuration:
/// `144 × 320000 / 32000 + 1` = 1441. Used to size pre-allocated buffers
/// so `encode_frame` never allocates after construction.
/// See `docs/mejoras.md` §3.2, M-3.
const MAX_FRAME_BYTES: usize = 1441;

/// Maximum per-granule-channel main_data in bytes: `MAX_FRAME_BYTES / 2`
/// (one granule's share, mono) is ~720; 1024 is generous headroom.
const MAX_GRANULE_BUF_BYTES: usize = 1024;

/// Number of look-back PCM samples fed to the psychoacoustic model's
/// 1024-point FFT for granule 0 of each frame, so the model sees a
/// continuous window of signal instead of zeros. 1024 - 576 = 448.
const PCM_HISTORY_SAMPLES: usize = 1024 - crate::types::SAMPLES_PER_GRANULE;

/// Configuration for a new [`Encoder`]. See
/// `docs/mp3-encoder/01-architecture.md` §5.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct EncoderConfig {
    /// Input/output sample rate (this encoder does not resample).
    pub sample_rate: SampleRate,
    /// Stereo/joint/dual/mono coding mode.
    pub channel_mode: ChannelMode,
    /// CBR, ABR, or VBR — see
    /// `docs/mp3-encoder/10-phase7-bit-reservoir-and-rate-control.md` §4.
    pub rate_control: RateControl,
}

impl EncoderConfig {
    /// Creates a new encoder configuration.
    #[must_use]
    pub const fn new(
        sample_rate: SampleRate,
        channel_mode: ChannelMode,
        rate_control: RateControl,
    ) -> Self {
        Self {
            sample_rate,
            channel_mode,
            rate_control,
        }
    }
}

/// Output of `filterbank → MDCT → psychoacoustic model` for one
/// granule/channel — the "analysis" half of the pipeline. Separated from
/// the "coding" half so joint stereo (M12) can inspect both channels'
/// spectra before quantizing either one.
#[derive(Debug, Clone)]
struct GranuleAnalysis {
    /// Flattened MDCT spectrum: subband-major, 32 × 18 = 576 lines
    /// (long) or interleaved short-block layout.
    spectrum: [f32; 576],
    /// Signal-to-mask ratio per scalefactor band (energy ratio, not dB).
    smr: ScalefactorBandSmr,
    /// Unified per-granule window shape, replacing bare `BlockType`.
    shape: GranuleShape,
}

/// Output of the *stateful, unrepeatable* half of granule analysis: the
/// psychoacoustic model's transient decision (which advances its own
/// internal state machine) and the polyphase filterbank's output (which
/// advances its sliding-history state). Neither can be safely re-run for
/// the same input. This is deliberately separate from the MDCT stage
/// ([`Encoder::mdct_stage`]), which — given this struct's fields — is a
/// pure function of `subband_samples` and `block_type` and so *can* be
/// re-run with a different (reconciled) `block_type`, which joint stereo
/// (M12) needs: see the block-type reconciliation step in
/// `Encoder::encode_frame`.
struct PreMdctAnalysis {
    ch: usize,
    subband_samples: [[f32; 18]; SUBBANDS],
    smr: ScalefactorBandSmr,
    block_type: BlockType,
}

/// A pure-Rust MP3 encoder. All working buffers are pre-allocated at
/// construction — [`Self::encode_frame`] performs no heap allocations.
///
/// # Known scope limitations
///
/// - **MPEG-2 LSF** sample rates are rejected outright (`Encoder::new`
///   returns [`EncodeError::UnsupportedSampleRate`]).
/// - **Intensity stereo** is rejected outright
///   (`EncodeError::UnsupportedChannelMode`). Mid/side (MS) joint stereo
///   is implemented and active for `ChannelMode::JointStereoMs`, with a
///   per-granule reconciliation step forcing both channels to share the
///   same window shape before the MS transform is applied (see
///   `reconcile_block_type` — mixing spectra transformed with different
///   shapes would otherwise combine physically unrelated spectral
///   lines). The reconciliation heuristic is a deliberate simplification,
///   not a jointly-optimized stereo transient decision — see
///   `docs/plus.md`'s review notes.
/// - **VBR and ABR** are both rejected outright
///   (`EncodeError::UnsupportedRateControl`) — `RateControl::Abr`'s
///   `nominal_bitrate()` is currently identical to `Cbr`'s, so accepting
///   it would silently produce fixed-bitrate output while claiming to
///   honor an average-bitrate target. Only CBR is implemented.
/// - **Bit reservoir** doesn't smooth across frames yet — every frame is
///   self-contained (`main_data_begin == 0` always).  `self.reservoir`'s
///   bookkeeping is still updated every frame so it's ready once the
///   output-buffering architecture is in place.
/// - **Window switching** (short blocks) is wired end-to-end — the
///   psychoacoustic model's transient detection drives MDCT window
///   selection (`Long`/`Start`/`Short`/`Stop`), including correct
///   overlap-add bookkeeping across block-type transitions and the
///   ISO/IEC 11172-3 §2.4.3.4.9 reorder step. **Not yet verified against
///   an external decoder**: the Huffman big_values region-0/1 boundary
///   used for `Start`/`Short`/`Stop` granules is internally consistent
///   but its exact split point has not been cross-checked against
///   Annex B's fixed rule for `window_switching_flag == 1` (a real
///   decoder derives that boundary independently, since it isn't
///   transmitted). Separately, the psychoacoustic model does not yet
///   compute per-window SMR for short blocks — quantization for
///   `Short`-block granules is still guided by an SMR profile computed
///   against the long-block scalefactor-band grid, a known mismatch
///   documented on `quantize::loop_control::build_band_map`. Neither gap
///   corrupts the bitstream's own internal consistency (all existing
///   tests pass), but both should be closed — the first via a
///   differential decode test on transient content, the second via
///   short-block-aware SMR — before treating short blocks as
///   production-ready. See `docs/plus.md` M11.2/M11.6/M11.7.
pub struct Encoder {
    /// Input configuration (sample rate, channels, rate control).
    config: EncoderConfig,
    /// Polyphase analysis filterbank, one per channel.
    filterbanks: [PolyphaseFilterbank; MAX_CHANNELS],
    /// Psychoacoustic Model II instance, one per channel.
    psychoacoustic: [PsychoacousticModel; MAX_CHANNELS],
    /// Bit reservoir for cross-frame bitrate smoothing.
    reservoir: BitReservoir,
    /// MDCT overlap history per channel, per subband.
    mdct_prev_tail: [[[f32; 18]; SUBBANDS]; MAX_CHANNELS],
    /// Running fractional-sample accumulator for the padding decision.
    padding_accumulator: u32,

    /// Accumulates per-granule scalefactor + Huffman bytes within a
    /// frame. Cleared at the start of each `encode_frame` call.
    main_data_buf: Vec<u8>,
    /// Scratch buffer for scalefactor encoding of one granule/channel.
    sf_buf: Vec<u8>,
    /// Scratch buffer for Huffman encoding of one granule/channel.
    granule_buf: Vec<u8>,
    /// Full frame assembly buffer.
    frame_buf: Vec<u8>,
    /// Number of look-back PCM samples fed to the psychoacoustic model's
    /// 1024-point FFT for granule 0 of each frame, so the model sees a
    /// continuous window of real signal instead of a truncated window
    /// padded with zeros. 1024 - 576 = 448 samples.
    /// Side-info bit-level serialization buffer.
    si_buf: Vec<u8>,

    /// PCM history (per channel) from the end of the previous frame, used
    /// as the first `PCM_HISTORY_SAMPLES` samples of granule 0's 1024-
    /// sample psychoacoustic analysis window. Initialized to all zeros
    /// (first frame has no real history — same as the previous behavior).
    pcm_history: [[f32; PCM_HISTORY_SAMPLES]; MAX_CHANNELS],

    /// Pre-allocated per-granule/channel analysis buffer. Filled in
    /// Phase 1 of `encode_frame`, consumed in Phase 2, then cleared.
    /// Sized for the worst case: MPEG-1 stereo (2 granules × 2 channels).
    analysis_buf: Vec<GranuleAnalysis>,
}

impl Encoder {
    /// Creates a new encoder. Pre-allocates every working buffer up
    /// front so [`Self::encode_frame`] stays allocation-free.
    ///
    /// # Errors
    ///
    /// Returns [`EncodeError`] if `config` requests an unsupported sample
    /// rate, channel mode, rate-control variant, or bitrate.
    pub fn new(config: EncoderConfig) -> Result<Self, EncodeError> {
        let version = config.sample_rate.version();
        if version != MpegVersion::Mpeg1 {
            return Err(EncodeError::UnsupportedSampleRate);
        }
        if matches!(config.channel_mode, ChannelMode::JointStereoIntensity) {
            return Err(EncodeError::UnsupportedChannelMode {
                mode: config.channel_mode,
            });
        }
        // ABR and VBR are both rejected outright, not just VBR: `Abr`'s
        // `nominal_bitrate()` currently returns the exact same fixed
        // per-frame bitrate as `Cbr` (see `bitstream/reservoir.rs`), and
        // nothing downstream in `encode_frame` distinguishes them either
        // — accepting `Abr` here would silently produce byte-identical
        // CBR output while claiming to honor an average-bitrate target,
        // exactly the "feature that appears to work while silently
        // ignoring the user's request" anti-pattern `docs/mejoras.md`
        // §2.2 already fixed once for both variants. Re-enabling `Abr`
        // needs the real averaging controller from `docs/plus.md` M13.4
        // first, not just removing this check.
        if matches!(
            config.rate_control,
            RateControl::Abr(_) | RateControl::Vbr(_)
        ) {
            return Err(EncodeError::UnsupportedRateControl {
                variant: match config.rate_control {
                    RateControl::Abr(_) => "Abr",
                    RateControl::Vbr(_) => "Vbr",
                    RateControl::Cbr(_) => unreachable!(),
                },
            });
        }
        let bitrate = config.rate_control.nominal_bitrate();
        if bitrate.header_index(version).is_none() {
            return Err(EncodeError::InvalidBitrate {
                kbps: bitrate.as_kbps(),
            });
        }

        let reservoir = BitReservoir::new(BitReservoir::max_for_version(version));

        let filterbanks = [const { PolyphaseFilterbank::new() }; MAX_CHANNELS];
        let mut psychoacoustic = [const { PsychoacousticModel::new() }; MAX_CHANNELS];
        for psy in &mut psychoacoustic {
            psy.init_for_sample_rate(config.sample_rate.as_hz());
        }
        let mdct_prev_tail = [[[0.0f32; 18]; SUBBANDS]; MAX_CHANNELS];
        let pcm_history = [[0.0f32; PCM_HISTORY_SAMPLES]; MAX_CHANNELS];

        Ok(Self {
            config,
            filterbanks,
            psychoacoustic,
            reservoir,
            mdct_prev_tail,
            padding_accumulator: 0,
            pcm_history,
            main_data_buf: Vec::with_capacity(MAX_FRAME_BYTES),
            sf_buf: Vec::with_capacity(64),
            granule_buf: Vec::with_capacity(MAX_GRANULE_BUF_BYTES),
            frame_buf: Vec::with_capacity(MAX_FRAME_BYTES),
            si_buf: Vec::with_capacity(40),
            analysis_buf: Vec::with_capacity(4),
        })
    }

    /// Encodes exactly one MPEG frame's worth of PCM and appends the
    /// resulting bytes to `out`. Returns the number of bytes written.
    ///
    /// Pipeline: validate → plan frame header → analyze all
    /// granules/channels → code each → assemble frame → emit.
    ///
    /// # Errors
    ///
    /// Returns [`EncodeError::BufferLengthMismatch`] if `pcm`'s channel
    /// count doesn't match this encoder's configuration.
    pub fn encode_frame(
        &mut self,
        pcm: &PcmBuffer,
        out: &mut Vec<u8>,
    ) -> Result<usize, EncodeError> {
        let version = self.config.sample_rate.version();
        let channel_mode = self.config.channel_mode;
        let n_channels = channel_mode.channel_count();
        let granules_per_frame = version.granules_per_frame();

        if pcm.samples_per_channel() != version.samples_per_frame() {
            return Err(EncodeError::BufferLengthMismatch {
                expected: n_channels * version.samples_per_frame(),
                got: pcm.samples_per_channel() * n_channels,
            });
        }

        let out_start = out.len();

        // --- Frame plan: header + budget ---
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

        let frame_bytes = frame_bytes_for_bitrate(bitrate, self.config.sample_rate, padding);
        let side_info_bytes: usize = if channel_mode.is_stereo() { 32 } else { 17 };
        let main_data_capacity = frame_bytes as usize - 4 - side_info_bytes;
        let nominal_bits = main_data_capacity as u32 * 8;
        let frame_bit_budget = nominal_bits;

        let (granule0_bits, granule1_bits) = split_bits_for_granules(frame_bit_budget, 0.0, 0.0);

        // --- Phase 1: Analyze all granules/channels ---
        self.analysis_buf.clear();
        let samples_per_granule = crate::types::SAMPLES_PER_GRANULE;

        // Phase 1a: psychoacoustic model + polyphase filterbank (stateful
        // — must run exactly once per granule/channel). MDCT is deferred
        // to Phase 1c so a joint-stereo pair's independently-decided
        // block types can be reconciled first (Phase 1b); see
        // `PreMdctAnalysis`'s doc comment for why MDCT can't just be
        // redone after the fact once it's run.
        let mut pre: [Option<PreMdctAnalysis>; MAX_CHANNELS * 2] = [None, None, None, None];
        let mut n_pre = 0usize;

        for gr in 0..granules_per_frame {
            for ch in 0..n_channels {
                let pcm_ch = pcm.channel(ch);
                let gr_offset = gr * samples_per_granule;

                // Build 1024-sample psychoacoustic window with real
                // look-back/look-ahead context instead of zero-padding.
                // Granule 0: prefix the current granule with the previous
                //   frame's last PCM_HISTORY_SAMPLES samples (from pcm_history).
                // Granule 1: prefix with granule 0's last PCM_HISTORY_SAMPLES.
                let mut pcm_window = [0.0f32; 1024];
                if gr == 0 {
                    pcm_window[..PCM_HISTORY_SAMPLES].copy_from_slice(&self.pcm_history[ch]);
                } else {
                    let hist_start = gr_offset - PCM_HISTORY_SAMPLES;
                    pcm_window[..PCM_HISTORY_SAMPLES]
                        .copy_from_slice(&pcm_ch[hist_start..gr_offset]);
                }
                let copy_len = samples_per_granule.min(pcm_ch.len() - gr_offset);
                pcm_window[PCM_HISTORY_SAMPLES..PCM_HISTORY_SAMPLES + copy_len]
                    .copy_from_slice(&pcm_ch[gr_offset..gr_offset + copy_len]);

                pre[n_pre] =
                    Some(self.analyze_pre_mdct(ch, gr, pcm_ch, samples_per_granule, &pcm_window));
                n_pre += 1;
            }
        }

        // Save granule 1's last PCM_HISTORY_SAMPLES for the next frame's
        // granule-0 look-ahead window.
        if granules_per_frame > 1 {
            for ch in 0..n_channels {
                let pcm_ch = pcm.channel(ch);
                let hist_start = pcm_ch.len() - PCM_HISTORY_SAMPLES;
                self.pcm_history[ch].copy_from_slice(&pcm_ch[hist_start..]);
            }
        }

        // Phase 1b: reconcile block_type across a JointStereoMs pair's
        // two channels *before* MDCT runs. Mid/side mixes same-index
        // spectral lines from both channels below — that's only
        // meaningful when both were transformed with the same window
        // shape; without this, a granule where the two channels'
        // independent transient decisions disagree (e.g. one detects a
        // transient, the other doesn't — routine for real stereo
        // content) would mix a Short-block spectral line from one
        // channel with a physically unrelated Long-block line from the
        // other. This does not touch either channel's own psychoacoustic
        // model state, so each model's *own* future decisions stay
        // consistent with its own transient history — only the shape
        // used for *this* granule's MDCT is overridden. See
        // `reconcile_block_type` and `docs/plus.md`'s review notes.
        if channel_mode == ChannelMode::JointStereoMs {
            let mut i = 0;
            while i + 1 < n_pre {
                let bt0 = pre[i].as_ref().expect("filled in phase 1a").block_type;
                let bt1 = pre[i + 1].as_ref().expect("filled in phase 1a").block_type;
                if bt0 != bt1 {
                    let reconciled = reconcile_block_type(bt0, bt1);
                    pre[i].as_mut().expect("filled in phase 1a").block_type = reconciled;
                    pre[i + 1].as_mut().expect("filled in phase 1a").block_type = reconciled;
                }
                i += 2;
            }
        }

        // Phase 1c: MDCT, using each granule/channel's final block_type.
        for slot in pre.iter_mut().take(n_pre) {
            let p = slot.take().expect("filled in phase 1a");
            let spectrum = self.mdct_stage(p.ch, &p.subband_samples, p.block_type);
            self.analysis_buf.push(GranuleAnalysis {
                spectrum,
                smr: p.smr,
                shape: GranuleShape::from_block_type(p.block_type, false),
            });
        }

        // --- Stereo transform (M12: mid/side for JointStereoMs) ---
        if channel_mode == ChannelMode::JointStereoMs {
            debug_assert_eq!(n_channels, 2, "JointStereoMs requires 2 channels");
            // Apply MS transform to each granule's spectrum pair.
            // mid = (L + R) / sqrt(2), side = (L - R) / sqrt(2)
            let inv_sqrt2 = 1.0 / core::f32::consts::SQRT_2;
            let total_analyses = granules_per_frame * n_channels;
            let mut ch0_idx = 0usize;
            while ch0_idx < total_analyses {
                let ch1_idx = ch0_idx + 1;
                let (left, right) = if ch0_idx < total_analyses && ch1_idx < total_analyses {
                    // Split the analysis buffer to get mutable access to both
                    let (left_part, right_part) = self.analysis_buf.split_at_mut(ch1_idx);
                    (&mut left_part[ch0_idx], &mut right_part[0])
                } else {
                    break;
                };
                for i in 0..576 {
                    let l = left.spectrum[i];
                    let r = right.spectrum[i];
                    left.spectrum[i] = (l + r) * inv_sqrt2;
                    right.spectrum[i] = (l - r) * inv_sqrt2;
                }
                ch0_idx += 2;
            }
        }

        // --- Phase 2: Code each analysis ---
        self.main_data_buf.clear();

        let mut gi_gr0_ch0 = default_granule_side_info();
        let mut gi_gr0_ch1 = gi_gr0_ch0;
        let mut gi_gr1_ch0 = gi_gr0_ch0;
        let mut gi_gr1_ch1 = gi_gr0_ch0;

        let mut idx = 0usize;
        for gr in 0..granules_per_frame {
            let granule_bits = if gr == 0 {
                granule0_bits
            } else {
                granule1_bits
            };
            let per_channel_bits = granule_bits / n_channels as u32;
            let huffman_budget = per_channel_bits.saturating_sub(MAX_SCALEFACTOR_BITS_PER_GRANULE);
            let sample_rate_hz = self.config.sample_rate.as_hz();

            for ch in 0..n_channels {
                let analysis = &self.analysis_buf[idx];
                idx += 1;

                let (
                    quant_result,
                    scalefac_compress,
                    scalefac_bits,
                    sf_len,
                    huffman_info,
                    huffman_bits,
                    granule_len,
                ) = Self::code_granule(
                    &mut self.sf_buf,
                    &mut self.granule_buf,
                    &analysis.spectrum,
                    &analysis.smr,
                    huffman_budget,
                    analysis.shape,
                    per_channel_bits,
                    sample_rate_hz,
                );

                debug_assert!(
                    scalefac_bits + huffman_bits <= per_channel_bits
                        || per_channel_bits < MAX_SCALEFACTOR_BITS_PER_GRANULE,
                    "granule/channel main_data overflow"
                );

                self.main_data_buf.extend_from_slice(&self.sf_buf[..sf_len]);
                self.main_data_buf
                    .extend_from_slice(&self.granule_buf[..granule_len]);

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
                gi.block_type = analysis.shape.block_type();
                gi.mixed_block_flag = matches!(analysis.shape, GranuleShape::Short { mixed: true });
                gi.scalefac_compress = scalefac_compress;
                gi.quant = quant_result;
                gi.huffman = huffman_info;
            }
        }

        // --- Reservoir bookkeeping ---
        let actual_bits_used = self.main_data_buf.len() as u32 * 8;
        self.reservoir
            .record_frame_usage(nominal_bits, actual_bits_used);

        debug_assert!(
            self.main_data_buf.len() <= main_data_capacity,
            "frame main_data exceeded physical capacity"
        );

        // --- Build frame in pre-allocated buffer ---
        self.frame_buf.clear();
        self.frame_buf.resize(frame_bytes as usize, 0);
        self.frame_buf[..4].copy_from_slice(&header_bits.to_be_bytes());
        let side_info_start = 4;
        let main_data_offset = 4 + side_info_bytes;

        // --- Write side info ---
        {
            let side_info = SideInfo {
                main_data_begin: 0,
                scfsi: [[false; 4]; 2],
                granules: [[gi_gr0_ch0, gi_gr0_ch1], [gi_gr1_ch0, gi_gr1_ch1]],
            };
            self.si_buf.clear();
            {
                let mut writer = BitWriter::new(&mut self.si_buf);
                side_info.write(&mut writer, channel_mode);
                writer.flush();
            }
            let si_len = side_info_bytes.min(self.si_buf.len());
            self.frame_buf[side_info_start..side_info_start + si_len]
                .copy_from_slice(&self.si_buf[..si_len]);
        }

        let emit_len = self.main_data_buf.len().min(main_data_capacity);
        self.frame_buf[main_data_offset..main_data_offset + emit_len]
            .copy_from_slice(&self.main_data_buf[..emit_len]);

        out.extend_from_slice(&self.frame_buf);

        Ok(out.len() - out_start)
    }

    /// First half of the pipeline for one granule/channel: psychoacoustic
    /// model + polyphase filterbank. MDCT is deferred to
    /// [`Self::mdct_stage`] so a joint-stereo pair's block-type
    /// disagreement can be reconciled before either channel's MDCT runs.
    ///
    /// `pcm_window` is a 1024-sample window pre-built by the caller with
    /// real look-back context (instead of zero-padding), fed directly to
    /// the psychoacoustic model's FFT.
    fn analyze_pre_mdct(
        &mut self,
        ch: usize,
        gr: usize,
        pcm_ch: &[f32],
        samples_per_granule: usize,
        pcm_window: &[f32; 1024],
    ) -> PreMdctAnalysis {
        let gr_offset = gr * samples_per_granule;

        // --- Psychoacoustic model (runs first so block_type is known
        //     before MDCT window selection) ---
        let (smr, psy_block_type) =
            self.psychoacoustic[ch].analyze_granule(pcm_window, self.config.sample_rate.as_hz());

        debug_assert!(
            matches!(
                psy_block_type,
                BlockType::Long | BlockType::Start | BlockType::Short | BlockType::Stop
            ),
            "unexpected block_type variant"
        );
        let block_type = psy_block_type;

        // --- Polyphase filterbank (needs all 18 × 32 samples) ---
        let mut subband_samples = [[0.0f32; 18]; SUBBANDS];
        #[allow(clippy::needless_range_loop)]
        for fbc in 0..18 {
            let pcm_offset = gr_offset + fbc * 32;
            let mut pcm_chunk = [0.0f32; 32];
            let copy_len = 32.min(pcm_ch.len() - pcm_offset);
            pcm_chunk[..copy_len].copy_from_slice(&pcm_ch[pcm_offset..pcm_offset + copy_len]);
            let subband = self.filterbanks[ch].analyze(&pcm_chunk);
            for sb in 0..SUBBANDS {
                subband_samples[sb][fbc] = subband[sb];
            }
        }

        PreMdctAnalysis {
            ch,
            subband_samples,
            smr,
            block_type,
        }
    }

    /// Second half of granule analysis: MDCT, given the (possibly
    /// stereo-reconciled) `block_type`. Pure in terms of its inputs plus
    /// `self.mdct_prev_tail[ch]` — safe to call exactly once per
    /// granule/channel *after* block-type reconciliation, never before
    /// (it mutates `self.mdct_prev_tail[ch]`, so calling it twice for the
    /// same granule would corrupt overlap state for the next one).
    fn mdct_stage(
        &mut self,
        ch: usize,
        subband_samples: &[[f32; 18]; SUBBANDS],
        block_type: BlockType,
    ) -> [f32; 576] {
        let mut mdct_out = [[0.0f32; 18]; SUBBANDS];
        let mut block_types = [BlockType::Long; SUBBANDS];

        for sb in 0..SUBBANDS {
            block_types[sb] = block_type;
            match block_type {
                BlockType::Long | BlockType::Start | BlockType::Stop => {
                    let window = match block_type {
                        BlockType::Long => long_window(),
                        BlockType::Start => long_window_for_kind(LongWindowKind::Start),
                        BlockType::Stop => long_window_for_kind(LongWindowKind::Stop),
                        _ => unreachable!(),
                    };
                    let (spec, new_tail) =
                        transform_long(&subband_samples[sb], &self.mdct_prev_tail[ch][sb], &window);
                    self.mdct_prev_tail[ch][sb] = new_tail;
                    mdct_out[sb] = spec;
                }
                BlockType::Short => {
                    let tail = self.mdct_prev_tail[ch][sb];
                    let new = subband_samples[sb];
                    let mut windows = [[0.0f32; 12]; 3];
                    windows[0][..6].copy_from_slice(&tail[12..18]);
                    windows[0][6..12].copy_from_slice(&new[0..6]);
                    windows[1][..6].copy_from_slice(&new[0..6]);
                    windows[1][6..12].copy_from_slice(&new[6..12]);
                    windows[2][..6].copy_from_slice(&new[6..12]);
                    windows[2][6..12].copy_from_slice(&new[12..18]);

                    let spec_blocks = transform_short(&windows);
                    for wi in 0..3 {
                        for k in 0..6 {
                            mdct_out[sb][wi * 6 + k] = spec_blocks[wi][k];
                        }
                    }
                    self.mdct_prev_tail[ch][sb] = new;
                }
            }
        }

        // Anti-aliasing butterfly across adjacent subbands (ISO §2.4.3.4.9.4)
        antialias_butterfly(&mut mdct_out, &block_types);

        // Flatten to 576-line layout
        let mut spectrum = [0.0f32; 576];
        for sb in 0..SUBBANDS {
            for k in 0..18 {
                spectrum[sb * 18 + k] = mdct_out[sb][k];
            }
        }

        spectrum
    }

    /// Second half of the pipeline for one granule/channel: quantize +
    /// scalefactor-encode + Huffman-encode. Returns all results needed
    /// for side-info assembly and main_data accumulation.
    #[allow(clippy::too_many_arguments)]
    fn code_granule(
        sf_buf: &mut Vec<u8>,
        granule_buf: &mut Vec<u8>,
        spectrum: &[f32; 576],
        smr: &ScalefactorBandSmr,
        huffman_budget: u32,
        shape: GranuleShape,
        per_channel_bits: u32,
        sample_rate_hz: u32,
    ) -> (
        crate::quantize::QuantizationResult,
        u8,
        u32,
        usize,
        crate::huffman::encode::HuffmanSideInfo,
        u32,
        usize,
    ) {
        let block_type = shape.block_type();

        // For short blocks: reorder spectrum from natural layout to the
        // interleaved scalefactor-band layout the quantizer expects.
        let maybe_reordered: [f32; 576];
        let actual_spectrum: &[f32; 576] = if matches!(shape, GranuleShape::Short { .. }) {
            let sfb_idx = scalefactor_sample_rate_index(sample_rate_hz);
            let bounds = &SFB_SHORT_BOUNDARIES[sfb_idx];
            let count = SFB_SHORT_COUNTS[sfb_idx];
            maybe_reordered = reorder_short(spectrum, bounds, count);
            &maybe_reordered
        } else {
            spectrum
        };

        let (
            mut quant_result,
            mut scalefac_compress,
            mut scalefac_bits,
            mut sf_len,
            mut huffman_info,
            mut huffman_bits,
            mut granule_len,
        ) = encode_granule_inner(
            sf_buf,
            granule_buf,
            actual_spectrum,
            smr,
            huffman_budget,
            block_type,
            sample_rate_hz,
        );

        if !quant_result.converged && scalefac_bits + huffman_bits > per_channel_bits {
            let flat_smr = ScalefactorBandSmr { bands: [1.0; 22] };
            (
                quant_result,
                scalefac_compress,
                scalefac_bits,
                sf_len,
                huffman_info,
                huffman_bits,
                granule_len,
            ) = encode_granule_inner(
                sf_buf,
                granule_buf,
                actual_spectrum,
                &flat_smr,
                huffman_budget,
                block_type,
                sample_rate_hz,
            );
        }

        (
            quant_result,
            scalefac_compress,
            scalefac_bits,
            sf_len,
            huffman_info,
            huffman_bits,
            granule_len,
        )
    }

    /// Flushes the bit reservoir and any buffered look-ahead samples at
    /// end of stream. Call exactly once, after the last `encode_frame`.
    #[allow(clippy::ptr_arg)]
    pub fn finish(&mut self, _out: &mut Vec<u8>) -> Result<usize, EncodeError> {
        Ok(0)
    }
}

/// Deterministic tie-break for a `JointStereoMs` granule whose two
/// channels' psychoacoustic models independently decided different
/// block types. Priority: `Short` (a real transient was detected in at
/// least one channel — resolving it with a short window matters more
/// than avoiding a forced switch on the other channel) over `Start`/
/// `Stop` (arbitrary but deterministic — both are long-shaped transition
/// windows) over `Long`. Both channels get whichever shape ranks higher.
///
/// This is a simplification, not a full stereo-aware psychoacoustic
/// model: it only prevents mixing incompatible window shapes under MS,
/// it doesn't try to jointly optimize *which* shape is chosen the way a
/// combined mid-signal transient analysis would. See `docs/plus.md`'s
/// review notes for the follow-up (deciding block_type once, from the
/// mid signal, when `JointStereoMs` is active) this is standing in for.
fn reconcile_block_type(a: BlockType, b: BlockType) -> BlockType {
    const fn rank(bt: BlockType) -> u8 {
        match bt {
            BlockType::Long => 0,
            BlockType::Stop => 1,
            BlockType::Start => 2,
            BlockType::Short => 3,
        }
    }
    if rank(a) >= rank(b) {
        a
    } else {
        b
    }
}

/// Encodes one granule/channel into the provided scratch buffers.
/// Returns quant/scalefac/huffman results and the byte-lengths used
/// within `sf_buf` and `granule_buf`.
#[allow(clippy::too_many_arguments)]
fn encode_granule_inner(
    sf_buf: &mut Vec<u8>,
    granule_buf: &mut Vec<u8>,
    spectrum: &[f32; 576],
    smr: &ScalefactorBandSmr,
    huffman_budget: u32,
    block_type: BlockType,
    sample_rate_hz: u32,
) -> (
    crate::quantize::QuantizationResult,
    u8,
    u32,
    usize,
    crate::huffman::encode::HuffmanSideInfo,
    u32,
    usize,
) {
    let quant_result = quantize_granule(spectrum, smr, huffman_budget, block_type, sample_rate_hz);

    sf_buf.clear();
    let (scalefac_compress, scalefac_bits) = {
        let mut sf_writer = BitWriter::new(sf_buf);
        let r = encode_granule_scalefactors(
            &quant_result.scalefac.values,
            block_type,
            sample_rate_hz,
            &mut sf_writer,
        );
        sf_writer.flush();
        r
    };
    let sf_len = sf_buf.len();

    granule_buf.clear();
    let huffman_info = {
        let mut writer = BitWriter::new(granule_buf);
        let info = encode_granule(&quant_result.ix, block_type, &mut writer);
        writer.flush();
        info
    };
    let huffman_bits = granule_buf.len() as u32 * 8;
    let granule_len = granule_buf.len();

    (
        quant_result,
        scalefac_compress,
        scalefac_bits,
        sf_len,
        huffman_info,
        huffman_bits,
        granule_len,
    )
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
    // integration tests.
}
