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
use crate::mdct::long_window;
use crate::mdct::transform_long;
use crate::mdct::BlockType;
use crate::psychoacoustic::{PsychoacousticModel, ScalefactorBandSmr};
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

/// A pure-Rust MP3 encoder. All working buffers are pre-allocated at
/// construction — [`Self::encode_frame`] performs no heap allocations.
///
/// # Known scope limitations
///
/// - **MPEG-2 LSF** sample rates are rejected outright (`Encoder::new`
///   returns [`EncodeError::UnsupportedSampleRate`]).
/// - **Joint stereo** (MS/intensity) is rejected outright
///   (`EncodeError::UnsupportedChannelMode`).
/// - **VBR/ABR** are rejected outright (`EncodeError::UnsupportedRateControl`).
/// - **Bit reservoir** doesn't smooth across frames yet — every frame is
///   self-contained (`main_data_begin == 0` always).  `self.reservoir`'s
///   bookkeeping is still updated every frame so it's ready once the
///   output-buffering architecture is in place.
/// - **Short blocks** are not yet wired — the psychoacoustic model's
///   transient detection is advisory-only.
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
    /// Side-info bit-level serialization buffer.
    si_buf: Vec<u8>,
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
        if matches!(
            config.channel_mode,
            ChannelMode::JointStereoMs | ChannelMode::JointStereoIntensity
        ) {
            return Err(EncodeError::UnsupportedChannelMode {
                mode: config.channel_mode,
            });
        }
        if matches!(
            config.rate_control,
            RateControl::Abr(_) | RateControl::Vbr(_)
        ) {
            return Err(EncodeError::UnsupportedRateControl {
                variant: match config.rate_control {
                    RateControl::Abr(_) => "Abr",
                    RateControl::Vbr(_) => "Vbr",
                    _ => unreachable!(),
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

        Ok(Self {
            config,
            filterbanks,
            psychoacoustic,
            reservoir,
            mdct_prev_tail,
            padding_accumulator: 0,
            main_data_buf: Vec::with_capacity(MAX_FRAME_BYTES),
            sf_buf: Vec::with_capacity(64),
            granule_buf: Vec::with_capacity(MAX_GRANULE_BUF_BYTES),
            frame_buf: Vec::with_capacity(MAX_FRAME_BYTES),
            si_buf: Vec::with_capacity(40),
        })
    }

    /// Encodes exactly one MPEG frame's worth of PCM and appends the
    /// resulting bytes to `out`. Returns the number of bytes written.
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
        let samples_per_granule = crate::types::SAMPLES_PER_GRANULE;

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
        let frame_bit_budget = nominal_bits;

        let (granule0_bits, granule1_bits) = split_bits_for_granules(frame_bit_budget, 0.0, 0.0);

        // --- Reuse pre-allocated main_data accumulator ---
        self.main_data_buf.clear();

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
            let per_channel_bits = granule_bits / n_channels as u32;
            let huffman_budget = per_channel_bits.saturating_sub(MAX_SCALEFACTOR_BITS_PER_GRANULE);

            for ch in 0..n_channels {
                let pcm_ch = pcm.channel(ch);
                let gr_offset = gr * samples_per_granule;

                // --- Polyphase filterbank ---
                let mut subband_samples = [[0.0f32; 18]; SUBBANDS];
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

                // --- MDCT ---
                let mdct_window = long_window();
                let mut spectrum = [0.0f32; 576];
                for sb in 0..SUBBANDS {
                    let (spec, new_tail) = transform_long(
                        &subband_samples[sb],
                        &self.mdct_prev_tail[ch][sb],
                        &mdct_window,
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
                let (smr, psy_block_type) = self.psychoacoustic[ch]
                    .analyze_granule(&pcm_window, self.config.sample_rate.as_hz());
                // FIXME: short blocks aren't fully wired yet — forcing
                // Long until every stage honors the model's decision.
                // See docs/mejoras.md §2.1.
                debug_assert!(
                    matches!(
                        psy_block_type,
                        BlockType::Long | BlockType::Start | BlockType::Short | BlockType::Stop
                    ),
                    "unexpected block_type variant"
                );
                let block_type = BlockType::Long;
                let sample_rate_hz = self.config.sample_rate.as_hz();

                // --- Quantize + encode (inline to reuse buffers) ---
                let (
                    mut quant_result,
                    mut scalefac_compress,
                    mut scalefac_bits,
                    mut sf_len,
                    mut huffman_info,
                    mut huffman_bits,
                    mut granule_len,
                ) = Self::encode_granule_inner(
                    &mut self.sf_buf,
                    &mut self.granule_buf,
                    &spectrum,
                    &smr,
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
                    ) = Self::encode_granule_inner(
                        &mut self.sf_buf,
                        &mut self.granule_buf,
                        &spectrum,
                        &flat_smr,
                        huffman_budget,
                        block_type,
                        sample_rate_hz,
                    );
                }

                debug_assert!(
                    scalefac_bits + huffman_bits <= per_channel_bits
                        || per_channel_bits < MAX_SCALEFACTOR_BITS_PER_GRANULE,
                    "granule/channel main_data overflow"
                );

                self.main_data_buf.extend_from_slice(&self.sf_buf[..sf_len]);
                self.main_data_buf
                    .extend_from_slice(&self.granule_buf[..granule_len]);

                // Store side info
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
        u8,    // scalefac_compress
        u32,   // scalefac_bits
        usize, // sf_buf used length
        crate::huffman::encode::HuffmanSideInfo,
        u32,   // huffman_bits
        usize, // granule_buf used length
    ) {
        let quant_result =
            quantize_granule(spectrum, smr, huffman_budget, block_type, sample_rate_hz);

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
            let info = encode_granule(&quant_result.ix, &mut writer);
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

    /// Flushes the bit reservoir and any buffered look-ahead samples at
    /// end of stream. Call exactly once, after the last `encode_frame`.
    // `&mut Vec<u8>`, not `&mut [u8]`: matches `encode_frame`'s output
    // parameter, and once cross-frame reservoir buffering lands
    // (docs/mejoras.md §5.3) this will need to *grow* `out` with the
    // final buffered frame's bytes via `extend_from_slice`, which a
    // fixed-size slice can't do. `_out` is unused only because that
    // buffering doesn't exist yet.
    #[allow(clippy::ptr_arg)]
    pub fn finish(&mut self, _out: &mut Vec<u8>) -> Result<usize, EncodeError> {
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
    // integration tests.
}
