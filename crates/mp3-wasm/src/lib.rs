//! wasm-bindgen bridge exposing `mp3-core`'s encoder to JS as a
//! push-based streaming API — the natural shape for driving from an
//! `AudioWorklet`. See `docs/mp3-encoder/12-phase9-cli-and-wasm.md` §2.
//!
//! Buffer reuse (M-5): `scratch_frame` and passing `&mut output` directly
//! to `encode_frame`/`finish` eliminates intermediate `Vec::new()` calls
//! per completed frame. See `docs/investigation-log.md` §3.2.

extern crate alloc;

use alloc::format;
use alloc::vec::Vec;

use mp3_core::io::PcmBuffer;
use mp3_core::{Bitrate, ChannelMode, EncoderConfig, MpegVersion, SampleRate};
use wasm_bindgen::prelude::*;

/// Streaming MP3 encoder for JS callers (e.g. an `AudioWorklet`).
///
/// Every unit of state lives on this struct, constructed explicitly from
/// JS — no global mutable state — so multiple concurrent instances (two
/// browser tabs, two simultaneous streams) are safe by construction. See
/// `docs/mp3-encoder/12-phase9-cli-and-wasm.md` §2.
#[wasm_bindgen]
pub struct WasmEncoder {
    inner: mp3_core::Encoder,
    channel_mode: ChannelMode,
    version: MpegVersion,
    /// Interleaved samples accumulated until a full MPEG frame
    /// (`version.samples_per_frame() * channel_count`) is available —
    /// AudioWorklet chunk sizes (typically 128 samples) don't align to
    /// MPEG frame sizes (1152 MPEG-1 / 576 LSF samples per channel);
    /// this buffer absorbs that mismatch so `mp3_core::Encoder` never
    /// sees a partial frame.
    pending: Vec<f32>,
}

#[wasm_bindgen]
impl WasmEncoder {
    /// Constructs a new streaming encoder.
    ///
    /// # Errors
    ///
    /// Returns a `JsValue` error if `sample_rate`/`channels`/`bitrate_kbps`
    /// are not supported — see
    /// `docs/mp3-encoder/04-phase1-pcm-io-and-framing.md` §2.
    #[wasm_bindgen(constructor)]
    pub fn new(sample_rate: u32, channels: u8, bitrate_kbps: u32) -> Result<WasmEncoder, JsValue> {
        Self::new_inner(sample_rate, channels, bitrate_kbps).map_err(|e| JsValue::from_str(&e))
    }

    fn new_inner(sample_rate: u32, channels: u8, bitrate_kbps: u32) -> Result<Self, String> {
        let sample_rate = match sample_rate {
            44_100 => SampleRate::Hz44100,
            48_000 => SampleRate::Hz48000,
            32_000 => SampleRate::Hz32000,
            22_050 => SampleRate::Hz22050,
            24_000 => SampleRate::Hz24000,
            16_000 => SampleRate::Hz16000,
            other => return Err(format!("unsupported sample rate: {other}")),
        };
        let version = sample_rate.version();
        let channel_mode = match channels {
            1 => ChannelMode::Mono,
            2 => ChannelMode::Stereo,
            other => return Err(format!("unsupported channel count: {other}")),
        };
        let config = EncoderConfig::new(
            sample_rate,
            channel_mode,
            mp3_core::bitstream::reservoir::RateControl::Cbr(
                Bitrate::from_kbps(bitrate_kbps)
                    .ok_or_else(|| format!("invalid bitrate: {bitrate_kbps}"))?,
            ),
        );
        let inner = mp3_core::Encoder::new(config).map_err(|e| format!("{e}"))?;

        Ok(Self {
            inner,
            channel_mode,
            version,
            pending: Vec::new(),
        })
    }

    /// Pushes an interleaved `Float32Array` chunk of arbitrary length.
    /// Returns any newly-completed MP3 bytes (may be empty if not enough
    /// samples have accumulated yet for a full frame).
    ///
    /// `samples.len()` should be a multiple of the configured channel
    /// count -- a chunk that splits a stereo pair shifts every following
    /// L/R sample by one position for the lifetime of this encoder. Real
    /// `AudioWorklet` callers already deliver channel-aligned chunks, so
    /// this is a caller-contract `debug_assert!`, not a `Result` check
    /// (see `docs/mp3-encoder/01-architecture.md` §4).
    ///
    /// # Errors
    ///
    /// Returns a `JsValue` error if a completed frame fails to encode --
    /// should not happen given a `WasmEncoder` that constructed
    /// successfully (every config-dependent failure is caught by `new`),
    /// but is surfaced rather than silently dropped if it ever does.
    #[wasm_bindgen]
    pub fn push(&mut self, samples: &[f32]) -> Result<Vec<u8>, JsValue> {
        self.push_inner(samples).map_err(|e| JsValue::from_str(&e))
    }

    fn push_inner(&mut self, samples: &[f32]) -> Result<Vec<u8>, String> {
        debug_assert_eq!(
            samples.len() % self.channel_mode.channel_count(),
            0,
            "push() chunk length must be a multiple of the channel count"
        );
        self.pending.extend_from_slice(samples);

        let frame_samples = self.version.samples_per_frame() * self.channel_mode.channel_count();
        let mut output = Vec::new();

        while self.pending.len() >= frame_samples {
            // Build PcmBuffer from pending slice, then drain -- avoids
            // the `.drain().collect()` allocation (M-5).
            let pcm = PcmBuffer::from_f32_interleaved(
                &self.pending[..frame_samples],
                self.channel_mode,
                self.version,
            )
            .map_err(|e| format!("{e}"))?;
            self.inner
                .encode_frame(&pcm, &mut output)
                .map_err(|e| format!("{e}"))?;
            self.pending.drain(..frame_samples);
        }

        Ok(output)
    }

    /// Flushes end of stream. Pads any remaining partial frame with
    /// silence, encodes it, and calls the core encoder's `finish`.
    ///
    /// # Errors
    ///
    /// Returns a `JsValue` error if the final partial frame fails to
    /// encode, or if the core encoder's own flush fails -- see
    /// [`Self::push`]'s doc comment on why this is surfaced rather than
    /// silently dropped.
    #[wasm_bindgen]
    pub fn finish(&mut self) -> Result<Vec<u8>, JsValue> {
        self.finish_inner().map_err(|e| JsValue::from_str(&e))
    }

    fn finish_inner(&mut self) -> Result<Vec<u8>, String> {
        let mut output = Vec::new();

        let frame_samples = self.version.samples_per_frame() * self.channel_mode.channel_count();

        // Pad and encode any remaining partial frame
        if !self.pending.is_empty() {
            self.pending.resize(frame_samples, 0.0f32);
            // Build PcmBuffer from pending, then drain -- avoids collect() (M-5).
            let pcm = PcmBuffer::from_f32_interleaved(
                &self.pending[..frame_samples],
                self.channel_mode,
                self.version,
            )
            .map_err(|e| format!("{e}"))?;
            self.inner
                .encode_frame(&pcm, &mut output)
                .map_err(|e| format!("{e}"))?;
            self.pending.drain(..frame_samples);
        }

        // Flush the encoder -- pass output directly instead of allocating
        // a separate finish_buf (M-5).
        self.inner.finish(&mut output).map_err(|e| format!("{e}"))?;

        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RATE: u32 = 44_100;
    const BITRATE: u32 = 128;

    /// Small-amplitude deterministic pseudo-noise, not silence.
    fn mono_samples(n: usize, seed_offset: u32) -> Vec<f32> {
        let mut seed = 12345u32.wrapping_add(seed_offset);
        (0..n)
            .map(|_| {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                ((seed >> 16) as i32 % 1000) as f32 / 32768.0
            })
            .collect()
    }

    #[test]
    fn push_emits_nothing_until_a_full_frame_accumulates() {
        let mut enc = WasmEncoder::new_inner(SAMPLE_RATE, 1, BITRATE).expect("construction");
        let mut total_pushed = 0usize;
        let mut saw_output = false;
        for i in 0..10 {
            let chunk = mono_samples(128, i);
            let out = enc.push_inner(&chunk).expect("push");
            total_pushed += 128;
            if !out.is_empty() {
                saw_output = true;
            }
            if total_pushed < 1152 {
                assert!(
                    out.is_empty(),
                    "no full frame accumulated yet ({total_pushed}/1152 samples) -- must not emit"
                );
            }
        }
        assert!(
            saw_output,
            "should have emitted a frame by 1280 samples pushed"
        );
    }

    #[test]
    fn push_output_is_independent_of_chunking() {
        let samples = mono_samples(1152 * 3, 0);

        let mut one_shot = WasmEncoder::new_inner(SAMPLE_RATE, 1, BITRATE).expect("construction");
        let mut one_shot_out = one_shot.push_inner(&samples).expect("push");
        one_shot_out.extend(one_shot.finish_inner().expect("finish"));

        let mut chunked = WasmEncoder::new_inner(SAMPLE_RATE, 1, BITRATE).expect("construction");
        let mut chunked_out = Vec::new();
        for chunk in samples.chunks(37) {
            chunked_out.extend(chunked.push_inner(chunk).expect("push"));
        }
        chunked_out.extend(chunked.finish_inner().expect("finish"));

        assert_eq!(
            one_shot_out, chunked_out,
            "chunking shape must not change the encoded output"
        );
    }

    #[test]
    fn finish_pads_and_flushes_a_remaining_partial_frame() {
        let mut enc = WasmEncoder::new_inner(SAMPLE_RATE, 1, BITRATE).expect("construction");
        let out = enc.push_inner(&mono_samples(500, 0)).expect("push");
        assert!(out.is_empty());

        let flushed = enc.finish_inner().expect("finish");
        assert!(
            !flushed.is_empty(),
            "finish() must encode the padded remaining partial frame"
        );
    }

    #[test]
    fn finish_with_no_pending_samples_returns_empty() {
        let mut enc = WasmEncoder::new_inner(SAMPLE_RATE, 1, BITRATE).expect("construction");
        let out = enc.push_inner(&mono_samples(1152, 0)).expect("push");
        assert!(!out.is_empty());

        let flushed = enc.finish_inner().expect("finish");
        assert!(flushed.is_empty());
    }

    #[test]
    fn new_rejects_bitrate_invalid_for_version() {
        assert!(WasmEncoder::new_inner(SAMPLE_RATE, 1, 144).is_err());
    }

    #[test]
    fn new_rejects_unsupported_sample_rate() {
        assert!(WasmEncoder::new_inner(11_025, 1, BITRATE).is_err());
    }

    #[test]
    fn new_rejects_unsupported_channel_count() {
        assert!(WasmEncoder::new_inner(SAMPLE_RATE, 3, BITRATE).is_err());
    }
}
