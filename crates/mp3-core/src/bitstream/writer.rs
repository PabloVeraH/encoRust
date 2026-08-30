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
    // u64, not u32: between calls `bits_in_buffer` is always < 8 (the
    // flush loop below keeps it that way), and a single call's `n_bits`
    // can be up to 32, so the accumulator must hold up to 7 + 32 = 39
    // bits without overflowing. An earlier, u32 version computed
    // `shift = 32 - bits_in_buffer - n_bits`, which underflows (panics
    // in debug, corrupts in release) whenever a call's `n_bits` is large
    // enough that `bits_in_buffer + n_bits > 32` -- not hit by M6's own
    // usage (Huffman codes/linbits/signs stay well under 32 bits), but
    // latent for whatever M7/M8 need from this shared, general-purpose
    // writer.
    bit_buffer: u64,
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
    /// `0..=32` (`0` is a no-op).
    pub fn write_bits(&mut self, value: u32, n_bits: u8) {
        debug_assert!(
            n_bits <= 32,
            "write_bits: n_bits must be <= 32, got {n_bits}"
        );
        if n_bits == 0 {
            return;
        }
        let masked = u64::from(value) & ((1u64 << n_bits) - 1);
        self.bit_buffer = (self.bit_buffer << n_bits) | masked;
        self.bits_in_buffer += n_bits;

        // Flush complete bytes, MSB-first: the oldest unflushed bits sit
        // just below the bits still being accumulated.
        while self.bits_in_buffer >= 8 {
            let shift = self.bits_in_buffer - 8;
            self.out.push((self.bit_buffer >> shift) as u8);
            self.bits_in_buffer -= 8;
        }
        // Drop anything flushed out of the low `bits_in_buffer` bits so
        // the accumulator never grows past what's still pending.
        self.bit_buffer &= (1u64 << self.bits_in_buffer) - 1;
    }

    /// Pads the final byte with zero bits and flushes to `out`. Call
    /// exactly once, at the true end of stream — verify against Annex B
    /// whether per-frame flushing is also required before assuming
    /// frames are independently byte-aligned; see
    /// `docs/mp3-encoder/11-phase8-bitstream-multiplexing.md` §1.
    pub fn flush(&mut self) {
        if self.bits_in_buffer > 0 {
            let pad = 8 - self.bits_in_buffer;
            self.out.push((self.bit_buffer << pad) as u8);
            self.bit_buffer = 0;
            self.bits_in_buffer = 0;
        }
    }

    /// Total bits written so far, *before* any padding `flush` would add.
    /// Lets a caller capture a section's exact bit length (e.g. one
    /// granule's scalefactors or Huffman data) while it's still being
    /// accumulated in its own scratch `BitWriter`, so that length can
    /// later be spliced bit-exactly into a different, ongoing bitstream
    /// via [`Self::write_raw_bits`] — see that method's doc comment for
    /// why exactness (not the flushed, byte-rounded length) matters.
    #[must_use]
    pub fn bit_len(&self) -> usize {
        self.out.len() * 8 + self.bits_in_buffer as usize
    }

    /// Appends exactly `n_bits` from `bytes` (MSB-first, starting at bit 0
    /// of `bytes[0]`) onto this writer's own bit position.
    ///
    /// Unlike `out.extend_from_slice(bytes)`, this does not require
    /// `bytes` to already be aligned to this writer's current bit
    /// position, and — critically — it does not carry over any padding
    /// a separate `flush()` on the *source* of `bytes` added beyond its
    /// true `n_bits`. Real MP3 `main_data` packs scalefactors and
    /// Huffman data for every granule/channel as one continuous bit
    /// stream with **no** byte-alignment between sections (only the
    /// start of `main_data` itself is byte-aligned); splicing
    /// independently-flushed, byte-padded scratch buffers together with
    /// a plain byte copy inserts up to 7 spurious padding bits at every
    /// section boundary, which desyncs a real decoder's bit-exact parse
    /// from that point on. This method is how a per-section scratch
    /// `BitWriter` (flushed for convenience, so its content is
    /// byte-readable) gets appended to a shared, continuously-packed
    /// bitstream without introducing that gap. See
    /// `docs/mp3-encoder/11-phase8-bitstream-multiplexing.md` §1 and the
    /// gain/corruption bug this fixes in `docs/investigation-log.md`'s
    /// investigation notes.
    ///
    /// # Panics
    ///
    /// Panics (via slice indexing) if `bytes` has fewer than
    /// `n_bits.div_ceil(8)` bytes.
    pub fn write_raw_bits(&mut self, bytes: &[u8], n_bits: usize) {
        let mut remaining = n_bits;
        let mut byte_idx = 0;
        while remaining >= 8 {
            self.write_bits(u32::from(bytes[byte_idx]), 8);
            byte_idx += 1;
            remaining -= 8;
        }
        if remaining > 0 {
            let value = u32::from(bytes[byte_idx]) >> (8 - remaining);
            self.write_bits(value, remaining as u8);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_byte_write_matches_expected_bits() {
        let mut out = Vec::new();
        let mut w = BitWriter::new(&mut out);
        w.write_bits(0b101, 3);
        w.write_bits(0b011, 3);
        w.flush();
        // "101" + "011" + 2 padding zero bits = 0b1010_1100
        assert_eq!(out, [0b1010_1100]);
    }

    #[test]
    fn write_crossing_a_byte_boundary() {
        let mut out = Vec::new();
        let mut w = BitWriter::new(&mut out);
        w.write_bits(0b1111_1111, 8); // fills byte 0 exactly
        w.write_bits(0b101, 3); // spills into byte 1
        w.flush();
        assert_eq!(out, [0b1111_1111, 0b1010_0000]);
    }

    #[test]
    fn write_bits_never_underflows_with_large_n_bits_and_pending_buffer() {
        // Regression test: an earlier u32-accumulator version computed
        // `shift = 32 - bits_in_buffer - n_bits`, which underflows
        // (panics in debug) whenever a single call's `n_bits` is large
        // enough that `bits_in_buffer + n_bits > 32`. 3 pending bits +
        // a 32-bit write is exactly that case.
        let mut out = Vec::new();
        let mut w = BitWriter::new(&mut out);
        w.write_bits(0b101, 3); // 3 bits pending
        w.write_bits(0xFFFF_FFFF, 32); // must not panic
        w.flush();
        // 3 + 32 = 35 bits, padded up to 40 (5 bytes) -> 5 padding zero
        // bits at the end: "101" + 32 ones + "00000":
        // 1011_1111 1111_1111 1111_1111 1111_1111 1110_0000
        assert_eq!(
            out,
            [
                0b1011_1111,
                0b1111_1111,
                0b1111_1111,
                0b1111_1111,
                0b1110_0000
            ]
        );
    }

    #[test]
    fn zero_bit_write_is_a_no_op() {
        let mut out = Vec::new();
        let mut w = BitWriter::new(&mut out);
        w.write_bits(0xDEAD, 0);
        w.write_bits(0b1, 1);
        w.flush();
        assert_eq!(out, [0b1000_0000]);
    }

    #[test]
    fn bit_len_reflects_unpadded_bit_count() {
        let mut out = Vec::new();
        let mut w = BitWriter::new(&mut out);
        assert_eq!(w.bit_len(), 0);
        w.write_bits(0b101, 3);
        assert_eq!(w.bit_len(), 3);
        w.write_bits(0xFF, 8);
        assert_eq!(w.bit_len(), 11);
    }

    #[test]
    fn write_raw_bits_splices_without_source_flush_padding() {
        // Source section: 5 bits ("10110"), flushed (byte-padded with 3
        // zero bits it never had). If those padding bits leaked into the
        // splice, the reader below would see 8 bits instead of 5 for this
        // section and misalign everything that follows -- exactly the
        // per-granule desync this method exists to prevent.
        let mut src_buf = Vec::new();
        let mut src = BitWriter::new(&mut src_buf);
        src.write_bits(0b10110, 5);
        let src_bits = src.bit_len();
        src.flush();
        assert_eq!(src_bits, 5);
        assert_eq!(src_buf, [0b1011_0000]); // 5 real bits + 3 flush-padding bits

        // Destination: not byte-aligned when the splice starts.
        let mut out = Vec::new();
        let mut dst = BitWriter::new(&mut out);
        dst.write_bits(0b11, 2); // 2 bits already pending
        dst.write_raw_bits(&src_buf, src_bits); // splice exactly 5 bits, not 8
        dst.write_bits(0b1, 1); // one more bit to prove position tracking
        dst.flush();

        // Expected bit sequence: "11" + "10110" + "1" = "11101101" --
        // exactly one byte, with nothing left for `flush` to pad. A
        // byte-copy of the flushed source (8 bits, padding included)
        // would have produced 10 bits total instead, spilling into a
        // second byte.
        assert_eq!(out, [0b1110_1101]);
    }

    #[test]
    fn many_small_writes_round_trip_via_test_bit_reader() {
        // Minimal MSB-first bit reader, test-only (the real one is M8
        // scope, chapter 11) -- just enough to prove write_bits' output
        // is self-consistent across many boundary-crossing calls, not
        // just the couple of hand-checked cases above.
        struct BitReader<'a> {
            data: &'a [u8],
            pos: usize,
        }
        impl<'a> BitReader<'a> {
            fn read_bits(&mut self, n: u8) -> u32 {
                let mut v = 0u32;
                for _ in 0..n {
                    let byte = self.data[self.pos / 8];
                    let bit = (byte >> (7 - self.pos % 8)) & 1;
                    v = (v << 1) | u32::from(bit);
                    self.pos += 1;
                }
                v
            }
        }

        let widths_values: [(u8, u32); 10] = [
            (1, 1),
            (3, 5),
            (5, 17),
            (7, 100),
            (9, 300),
            (13, 4000),
            (2, 0),
            (16, 0xBEEF),
            (4, 9),
            (1, 0),
        ];

        let mut out = Vec::new();
        let mut w = BitWriter::new(&mut out);
        for &(bits, value) in &widths_values {
            w.write_bits(value, bits);
        }
        w.flush();

        let mut r = BitReader { data: &out, pos: 0 };
        for &(bits, value) in &widths_values {
            assert_eq!(r.read_bits(bits), value, "mismatch at width {bits}");
        }
    }
}
