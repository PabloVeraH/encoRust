//! `BitWriter`: MSB-first bit-level output. See
//! `docs/mp3-encoder/11-phase8-bitstream-multiplexing.md` §1.
//!
//! Pure bit-packing, no MP3-specific meaning — get this to 100%
//! confidence independently of everything else, per that section's
//! recommendation, before layering standard semantics on top of it.

use alloc::vec::Vec;

/// Accumulates bits MSB-first and pushes completed bytes to `out` as they
/// fill. Allocation-free per [`Self::write_bits`] call (the internal
/// buffer only causes `out` to grow by the natural byte-at-a-time push
/// when it fills — no additional allocation beyond `Vec`'s own amortized
/// growth).
pub struct BitWriter<'a> {
    out: &'a mut Vec<u8>,
    bit_buffer: u32,
    bits_in_buffer: u8,
}

impl<'a> BitWriter<'a> {
    /// Creates a writer appending to `out`.
    pub fn new(out: &'a mut Vec<u8>) -> Self {
        Self {
            out,
            bit_buffer: 0,
            bits_in_buffer: 0,
        }
    }

    /// Writes the low `n_bits` of `value`, MSB-first. `n_bits` must be in
    /// `1..=32`.
    ///
    /// # Panics
    ///
    /// Always, in this scaffold.
    pub fn write_bits(&mut self, value: u32, n_bits: u8) {
        let _ = (
            value,
            n_bits,
            &mut self.bit_buffer,
            &mut self.bits_in_buffer,
            &mut self.out,
        );
        todo!("M8: MSB-first bit packing — see 11-phase8-bitstream-multiplexing.md §1")
    }

    /// Pads the final byte with zero bits and flushes to `out`. Call
    /// exactly once, at the true end of stream — verify against Annex B
    /// whether per-frame flushing is also required before assuming
    /// frames are independently byte-aligned; see
    /// `docs/mp3-encoder/11-phase8-bitstream-multiplexing.md` §1.
    ///
    /// # Panics
    ///
    /// Always, in this scaffold.
    pub fn flush(&mut self) {
        todo!("M8: pad final byte + flush — see 11-phase8-bitstream-multiplexing.md §1")
    }
}

#[cfg(test)]
mod tests {
    // TODO(M8): exhaustive, MP3-agnostic bit-packing tests — write
    // various bit widths/values, read back with a test-only bit reader,
    // assert exact round-trip, including buffer-boundary-crossing cases
    // (n_bits that straddle a byte boundary). See
    // docs/mp3-encoder/11-phase8-bitstream-multiplexing.md §1 and §7.
}
