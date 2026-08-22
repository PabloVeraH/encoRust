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
