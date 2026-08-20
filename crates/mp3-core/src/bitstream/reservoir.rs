//! The bit reservoir and CBR/ABR/VBR rate control. See
//! `docs/mp3-encoder/10-phase7-bit-reservoir-and-rate-control.md`.

use crate::types::Bitrate;

/// How the encoder allocates bits across frames. See
/// `docs/mp3-encoder/10-phase7-bit-reservoir-and-rate-control.md` §4.
#[derive(Debug, Clone, Copy)]
pub enum RateControl {
    /// Fixed nominal bitrate per frame; the reservoir only smooths local
    /// complexity spikes — average output bitrate equals the target
    /// exactly.
    Cbr(Bitrate),
    /// Nominal bitrate is a target *average*, not a per-frame guarantee.
    Abr(Bitrate),
    /// No fixed nominal bitrate — each frame's bitrate is whichever
    /// standard value is the smallest that fits the bits the
    /// quality-driven quantization loop actually produced.
    Vbr(VbrQuality),
}

/// VBR quality target. Scale/meaning TBD at implementation time (a
/// LAME-`-V`-style 0-9 scale is a reasonable, well-precedented choice —
/// see `docs/mp3-encoder/02-standards-and-prior-art.md` §4 on borrowing
/// approach, not code).
#[derive(Debug, Clone, Copy)]
pub struct VbrQuality(pub u8);

/// Tracks banked bits available for a future frame to spend beyond its
/// own nominal allocation. See
/// `docs/mp3-encoder/10-phase7-bit-reservoir-and-rate-control.md` §1-2.
pub struct BitReservoir {
    available_bits: u32,
    /// The cap follows from `main_data_begin`'s width: 511 bytes for
    /// MPEG-1 (9-bit field), 255 bytes for MPEG-2 LSF (8-bit field) —
    /// construct per [`crate::types::MpegVersion`], see
    /// `docs/mp3-encoder/10-phase7-bit-reservoir-and-rate-control.md` §1-2.
    // Not yet read: both methods that use it are `todo!()` pending M7 —
    // see docs/mp3-encoder/14-roadmap-and-milestones.md.
    #[allow(dead_code)]
    max_reservoir_bits: u32,
}

impl BitReservoir {
    /// Creates an empty reservoir with the given cap (in bits).
    #[must_use]
    pub const fn new(max_reservoir_bits: u32) -> Self {
        Self {
            available_bits: 0,
            max_reservoir_bits,
        }
    }

    /// How many bits this frame may spend in total, beyond
    /// `nominal_frame_bits` — bounded by what's currently banked and by
    /// the reservoir's cap.
    ///
    /// # Panics
    ///
    /// Always, in this scaffold.
    #[must_use]
    pub fn available_for_frame(&self, nominal_frame_bits: u32) -> u32 {
        let _ = nominal_frame_bits;
        todo!("M7: nominal + min(available_bits, need), capped — see 10-phase7 §3")
    }

    /// Updates the bank after a frame's actual usage is known (called
    /// once Huffman coding has produced the real bit count).
    ///
    /// # Panics
    ///
    /// Always, in this scaffold.
    pub fn record_frame_usage(&mut self, nominal_frame_bits: u32, actual_bits_used: u32) {
        let _ = (
            nominal_frame_bits,
            actual_bits_used,
            &mut self.available_bits,
        );
        todo!("M7: banked += nominal - actual, clamped [0, max] — see 10-phase7 §2")
    }
}

#[cfg(test)]
mod tests {
    // TODO(M7):
    // - Bits bank correctly across alternating over/under-nominal-usage
    //   frames; available_bits never exceeds max_reservoir_bits or goes
    //   negative.
    // - CBR frame-size formula (144 * bitrate / sample_rate + padding)
    //   matches hand-computed values for representative (bitrate,
    //   sample_rate) pairs.
    // See docs/mp3-encoder/10-phase7-bit-reservoir-and-rate-control.md §5.
}
