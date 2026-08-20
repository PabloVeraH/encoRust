//! wasm-bindgen bridge exposing `mp3-core`'s encoder to JS as a
//! push-based streaming API — the natural shape for driving from an
//! `AudioWorklet`. See `docs/mp3-encoder/12-phase9-cli-and-wasm.md` §2.
//!
//! # Scaffold status
//!
//! `mp3_core::Encoder::new` currently panics (`todo!()`) — see
//! `docs/mp3-encoder/14-roadmap-and-milestones.md`. This bridge's job
//! (accumulating arbitrary-sized pushed chunks into full MPEG frames) is
//! itself real, working logic once written, independent of that.

extern crate alloc;

use alloc::format;
use alloc::vec::Vec;

use mp3_core::bitstream::RateControl;
use mp3_core::{Bitrate, ChannelMode, EncoderConfig, SampleRate};
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
        let sample_rate = match sample_rate {
            44_100 => SampleRate::Hz44100,
            48_000 => SampleRate::Hz48000,
            32_000 => SampleRate::Hz32000,
            22_050 => SampleRate::Hz22050,
            24_000 => SampleRate::Hz24000,
            16_000 => SampleRate::Hz16000,
            other => {
                return Err(JsValue::from_str(&format!(
                    "unsupported sample rate: {other}"
                )))
            }
        };
        let channel_mode = match channels {
            1 => ChannelMode::Mono,
            2 => ChannelMode::Stereo,
            other => {
                return Err(JsValue::from_str(&format!(
                    "unsupported channel count: {other}"
                )))
            }
        };
        let config = EncoderConfig {
            sample_rate,
            channel_mode,
            rate_control: RateControl::Cbr(
                Bitrate::from_kbps(bitrate_kbps).expect("invalid bitrate"),
            ),
        };
        let inner =
            mp3_core::Encoder::new(config).map_err(|e| JsValue::from_str(&format!("{e}")))?;
        Ok(Self {
            inner,
            channel_mode,
            pending: Vec::new(),
        })
    }

    /// Pushes an interleaved `Float32Array` chunk of arbitrary length.
    /// Returns any newly-completed MP3 bytes (may be empty if not enough
    /// samples have accumulated yet for a full frame).
    ///
    /// # Panics
    ///
    /// Always, in this scaffold — depends on
    /// `mp3_core::Encoder::encode_frame` (M1-M8, not yet implemented).
    #[wasm_bindgen]
    pub fn push(&mut self, samples: &[f32]) -> Vec<u8> {
        let _ = (
            &mut self.inner,
            self.channel_mode,
            samples,
            &mut self.pending,
        );
        todo!(
            "M9: accumulate into self.pending, drain full frames through \
             self.inner.encode_frame — see 12-phase9-cli-and-wasm.md §2"
        )
    }

    /// Flushes end of stream.
    ///
    /// # Panics
    ///
    /// Always, in this scaffold.
    #[wasm_bindgen]
    pub fn finish(&mut self) -> Vec<u8> {
        let _ = &mut self.inner;
        todo!("M9: mp3_core::Encoder::finish — see 12-phase9-cli-and-wasm.md §2")
    }
}
