//! Huffman tables for MPEG-1/2 Layer III encoding.
//!
//! **Provenance**: numeric data sourced from FFmpeg's
//! `libavcodec/mpegaudiodectab.h`, cross-checked against minimp3 `tabs[]`
//! (CC0) -- both decoders, but decoders need the *exact same* tables to
//! decode correctly, so they're legitimate sources per
//! `docs/mp3-encoder/09-phase6-huffman-coding.md` §2. These 34 fixed
//! tables (16 distinct big_values trees + escape linbits + 2 count1
//! trees) are ISO/IEC 11172-3 **Annex B** (Table B.7) data -- an earlier
//! version of this comment cited Annex D, which is the psychoacoustic
//! model's data (chapter 07), not the Huffman tables.
//!
//! ⚠️ **Licensing note**: FFmpeg's `mpegaudiodectab.h` is LGPL-2.1+;
//! this crate is MIT OR Apache-2.0. The Huffman code words themselves are
//! ISO-standard-mandated data (any conforming encoder/decoder must use
//! the identical bit patterns to interoperate), which is why permissively
//! licensed encoders/decoders (minimp3 CC0, "shine", etc.) all reproduce
//! the same values under different licenses -- but *this file's specific
//! transcription* was checked against FFmpeg first and minimp3 second,
//! the reverse of what chapter 09 §2 recommends for exactly this reason.
//! Before any public release, re-verify this data was independently
//! re-derived/re-checked against the CC0 source (or the standard text
//! directly) rather than transcribed from the LGPL one, and update this
//! note accordingly -- not resolved as part of this pass.

/// A single entry in the flat VLC lookup table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VlcEntry {
    /// Huffman code word (MSB-aligned within bit length).
    pub code: u32,
    /// Number of significant bits in `code`.
    pub bits: u8,
}

/// A Huffman code table for a big_values region.
/// Indexed as `table.lookup[y * xlen + x]`.
#[derive(Debug, Clone, Copy)]
pub struct HuffmanTable {
    /// Number of x values (`xlen`). Square tables have `xlen == ylen`.
    pub xlen: usize,
    /// Flat lookup array, length `xlen * xlen`.
    lookup: &'static [VlcEntry],
}

impl HuffmanTable {
    /// Look up the (code, bits) for a (x, y) pair.
    #[inline]
    pub fn lookup(&self, x: usize, y: usize) -> VlcEntry {
        self.lookup[y * self.xlen + x]
    }

    /// Number of x values (alias for `xlen` field).
    #[inline]
    pub const fn len_x(&self) -> usize {
        self.xlen
    }
}

/// Count1 Huffman tree for quadruples (v, w, x, y).
/// Index: `v*8 + w*4 + x*2 + y`, where each component is 0 for a zero
/// value and 1 for *either* sign (magnitude-only -- sign is transmitted
/// as a separate bit per nonzero value, after the quad code, exactly
/// like big_values' sign bits; see `huffman::encode::encode_granule`'s
/// count1 region). An earlier version of this comment said "-1 maps to
/// 3, 0 to 0, 1 to 1", implying a 3-valued per-component encoding that
/// matches neither the actual table data below (whose own per-entry
/// comments, e.g. `// (0,0,1,1)`, are binary) nor how `encode.rs`
/// actually indexes into it (`signed_to_index`: 0 -> 0, anything else
/// -> 1).
#[derive(Debug, Clone, Copy)]
pub struct Count1Table {
    /// The 16 VlcEntry entries for count1 quadruples.
    pub entries: &'static [VlcEntry; 16],
}

// ─── VLC encoding tables (from FFmpeg mpa_huffcodes_N / mpa_huffbits_N) ──

static VLC_TABLE_1: [VlcEntry; 4] = [
    VlcEntry {
        code: 0x0001,
        bits: 1,
    }, // (0,0)
    VlcEntry {
        code: 0x0001,
        bits: 3,
    }, // (1,0)
    VlcEntry {
        code: 0x0001,
        bits: 2,
    }, // (0,1)
    VlcEntry {
        code: 0x0000,
        bits: 3,
    }, // (1,1)
];

static VLC_TABLE_2: [VlcEntry; 9] = [
    VlcEntry {
        code: 0x0001,
        bits: 1,
    }, // (0,0)
    VlcEntry {
        code: 0x0002,
        bits: 3,
    }, // (1,0)
    VlcEntry {
        code: 0x0001,
        bits: 6,
    }, // (2,0)
    VlcEntry {
        code: 0x0003,
        bits: 3,
    }, // (0,1)
    VlcEntry {
        code: 0x0001,
        bits: 3,
    }, // (1,1)
    VlcEntry {
        code: 0x0001,
        bits: 5,
    }, // (2,1)
    VlcEntry {
        code: 0x0003,
        bits: 5,
    }, // (0,2)
    VlcEntry {
        code: 0x0002,
        bits: 5,
    }, // (1,2)
    VlcEntry {
        code: 0x0000,
        bits: 6,
    }, // (2,2)
];

static VLC_TABLE_3: [VlcEntry; 9] = [
    VlcEntry {
        code: 0x0003,
        bits: 2,
    }, // (0,0)
    VlcEntry {
        code: 0x0002,
        bits: 2,
    }, // (1,0)
    VlcEntry {
        code: 0x0001,
        bits: 6,
    }, // (2,0)
    VlcEntry {
        code: 0x0001,
        bits: 3,
    }, // (0,1)
    VlcEntry {
        code: 0x0001,
        bits: 2,
    }, // (1,1)
    VlcEntry {
        code: 0x0001,
        bits: 5,
    }, // (2,1)
    VlcEntry {
        code: 0x0003,
        bits: 5,
    }, // (0,2)
    VlcEntry {
        code: 0x0002,
        bits: 5,
    }, // (1,2)
    VlcEntry {
        code: 0x0000,
        bits: 6,
    }, // (2,2)
];

static VLC_TABLE_5: [VlcEntry; 16] = [
    VlcEntry {
        code: 0x0001,
        bits: 1,
    }, // (0,0)
    VlcEntry {
        code: 0x0002,
        bits: 3,
    }, // (1,0)
    VlcEntry {
        code: 0x0006,
        bits: 6,
    }, // (2,0)
    VlcEntry {
        code: 0x0005,
        bits: 7,
    }, // (3,0)
    VlcEntry {
        code: 0x0003,
        bits: 3,
    }, // (0,1)
    VlcEntry {
        code: 0x0001,
        bits: 3,
    }, // (1,1)
    VlcEntry {
        code: 0x0004,
        bits: 6,
    }, // (2,1)
    VlcEntry {
        code: 0x0004,
        bits: 7,
    }, // (3,1)
    VlcEntry {
        code: 0x0007,
        bits: 6,
    }, // (0,2)
    VlcEntry {
        code: 0x0005,
        bits: 6,
    }, // (1,2)
    VlcEntry {
        code: 0x0007,
        bits: 7,
    }, // (2,2)
    VlcEntry {
        code: 0x0001,
        bits: 8,
    }, // (3,2)
    VlcEntry {
        code: 0x0006,
        bits: 7,
    }, // (0,3)
    VlcEntry {
        code: 0x0001,
        bits: 6,
    }, // (1,3)
    VlcEntry {
        code: 0x0001,
        bits: 7,
    }, // (2,3)
    VlcEntry {
        code: 0x0000,
        bits: 8,
    }, // (3,3)
];

static VLC_TABLE_6: [VlcEntry; 16] = [
    VlcEntry {
        code: 0x0007,
        bits: 3,
    }, // (0,0)
    VlcEntry {
        code: 0x0003,
        bits: 3,
    }, // (1,0)
    VlcEntry {
        code: 0x0005,
        bits: 5,
    }, // (2,0)
    VlcEntry {
        code: 0x0001,
        bits: 7,
    }, // (3,0)
    VlcEntry {
        code: 0x0006,
        bits: 3,
    }, // (0,1)
    VlcEntry {
        code: 0x0002,
        bits: 2,
    }, // (1,1)
    VlcEntry {
        code: 0x0003,
        bits: 4,
    }, // (2,1)
    VlcEntry {
        code: 0x0002,
        bits: 5,
    }, // (3,1)
    VlcEntry {
        code: 0x0005,
        bits: 4,
    }, // (0,2)
    VlcEntry {
        code: 0x0004,
        bits: 4,
    }, // (1,2)
    VlcEntry {
        code: 0x0004,
        bits: 5,
    }, // (2,2)
    VlcEntry {
        code: 0x0001,
        bits: 6,
    }, // (3,2)
    VlcEntry {
        code: 0x0003,
        bits: 6,
    }, // (0,3)
    VlcEntry {
        code: 0x0003,
        bits: 5,
    }, // (1,3)
    VlcEntry {
        code: 0x0002,
        bits: 6,
    }, // (2,3)
    VlcEntry {
        code: 0x0000,
        bits: 7,
    }, // (3,3)
];

static VLC_TABLE_7: [VlcEntry; 36] = [
    VlcEntry {
        code: 0x0001,
        bits: 1,
    }, // (0,0)
    VlcEntry {
        code: 0x0002,
        bits: 3,
    }, // (1,0)
    VlcEntry {
        code: 0x000a,
        bits: 6,
    }, // (2,0)
    VlcEntry {
        code: 0x0013,
        bits: 8,
    }, // (3,0)
    VlcEntry {
        code: 0x0010,
        bits: 8,
    }, // (4,0)
    VlcEntry {
        code: 0x000a,
        bits: 9,
    }, // (5,0)
    VlcEntry {
        code: 0x0003,
        bits: 3,
    }, // (0,1)
    VlcEntry {
        code: 0x0003,
        bits: 4,
    }, // (1,1)
    VlcEntry {
        code: 0x0007,
        bits: 6,
    }, // (2,1)
    VlcEntry {
        code: 0x000a,
        bits: 7,
    }, // (3,1)
    VlcEntry {
        code: 0x0005,
        bits: 7,
    }, // (4,1)
    VlcEntry {
        code: 0x0003,
        bits: 8,
    }, // (5,1)
    VlcEntry {
        code: 0x000b,
        bits: 6,
    }, // (0,2)
    VlcEntry {
        code: 0x0004,
        bits: 5,
    }, // (1,2)
    VlcEntry {
        code: 0x000d,
        bits: 7,
    }, // (2,2)
    VlcEntry {
        code: 0x0011,
        bits: 8,
    }, // (3,2)
    VlcEntry {
        code: 0x0008,
        bits: 8,
    }, // (4,2)
    VlcEntry {
        code: 0x0004,
        bits: 9,
    }, // (5,2)
    VlcEntry {
        code: 0x000c,
        bits: 7,
    }, // (0,3)
    VlcEntry {
        code: 0x000b,
        bits: 7,
    }, // (1,3)
    VlcEntry {
        code: 0x0012,
        bits: 8,
    }, // (2,3)
    VlcEntry {
        code: 0x000f,
        bits: 9,
    }, // (3,3)
    VlcEntry {
        code: 0x000b,
        bits: 9,
    }, // (4,3)
    VlcEntry {
        code: 0x0002,
        bits: 9,
    }, // (5,3)
    VlcEntry {
        code: 0x0007,
        bits: 7,
    }, // (0,4)
    VlcEntry {
        code: 0x0006,
        bits: 7,
    }, // (1,4)
    VlcEntry {
        code: 0x0009,
        bits: 8,
    }, // (2,4)
    VlcEntry {
        code: 0x000e,
        bits: 9,
    }, // (3,4)
    VlcEntry {
        code: 0x0003,
        bits: 9,
    }, // (4,4)
    VlcEntry {
        code: 0x0001,
        bits: 10,
    }, // (5,4)
    VlcEntry {
        code: 0x0006,
        bits: 8,
    }, // (0,5)
    VlcEntry {
        code: 0x0004,
        bits: 8,
    }, // (1,5)
    VlcEntry {
        code: 0x0005,
        bits: 9,
    }, // (2,5)
    VlcEntry {
        code: 0x0003,
        bits: 10,
    }, // (3,5)
    VlcEntry {
        code: 0x0002,
        bits: 10,
    }, // (4,5)
    VlcEntry {
        code: 0x0000,
        bits: 10,
    }, // (5,5)
];

static VLC_TABLE_8: [VlcEntry; 36] = [
    VlcEntry {
        code: 0x0003,
        bits: 2,
    }, // (0,0)
    VlcEntry {
        code: 0x0004,
        bits: 3,
    }, // (1,0)
    VlcEntry {
        code: 0x0006,
        bits: 6,
    }, // (2,0)
    VlcEntry {
        code: 0x0012,
        bits: 8,
    }, // (3,0)
    VlcEntry {
        code: 0x000c,
        bits: 8,
    }, // (4,0)
    VlcEntry {
        code: 0x0005,
        bits: 9,
    }, // (5,0)
    VlcEntry {
        code: 0x0005,
        bits: 3,
    }, // (0,1)
    VlcEntry {
        code: 0x0001,
        bits: 2,
    }, // (1,1)
    VlcEntry {
        code: 0x0002,
        bits: 4,
    }, // (2,1)
    VlcEntry {
        code: 0x0010,
        bits: 8,
    }, // (3,1)
    VlcEntry {
        code: 0x0009,
        bits: 8,
    }, // (4,1)
    VlcEntry {
        code: 0x0003,
        bits: 8,
    }, // (5,1)
    VlcEntry {
        code: 0x0007,
        bits: 6,
    }, // (0,2)
    VlcEntry {
        code: 0x0003,
        bits: 4,
    }, // (1,2)
    VlcEntry {
        code: 0x0005,
        bits: 6,
    }, // (2,2)
    VlcEntry {
        code: 0x000e,
        bits: 8,
    }, // (3,2)
    VlcEntry {
        code: 0x0007,
        bits: 8,
    }, // (4,2)
    VlcEntry {
        code: 0x0003,
        bits: 9,
    }, // (5,2)
    VlcEntry {
        code: 0x0013,
        bits: 8,
    }, // (0,3)
    VlcEntry {
        code: 0x0011,
        bits: 8,
    }, // (1,3)
    VlcEntry {
        code: 0x000f,
        bits: 8,
    }, // (2,3)
    VlcEntry {
        code: 0x000d,
        bits: 9,
    }, // (3,3)
    VlcEntry {
        code: 0x000a,
        bits: 9,
    }, // (4,3)
    VlcEntry {
        code: 0x0004,
        bits: 10,
    }, // (5,3)
    VlcEntry {
        code: 0x000d,
        bits: 8,
    }, // (0,4)
    VlcEntry {
        code: 0x0005,
        bits: 7,
    }, // (1,4)
    VlcEntry {
        code: 0x0008,
        bits: 8,
    }, // (2,4)
    VlcEntry {
        code: 0x000b,
        bits: 9,
    }, // (3,4)
    VlcEntry {
        code: 0x0005,
        bits: 10,
    }, // (4,4)
    VlcEntry {
        code: 0x0001,
        bits: 10,
    }, // (5,4)
    VlcEntry {
        code: 0x000c,
        bits: 9,
    }, // (0,5)
    VlcEntry {
        code: 0x0004,
        bits: 8,
    }, // (1,5)
    VlcEntry {
        code: 0x0004,
        bits: 9,
    }, // (2,5)
    VlcEntry {
        code: 0x0001,
        bits: 9,
    }, // (3,5)
    VlcEntry {
        code: 0x0001,
        bits: 11,
    }, // (4,5)
    VlcEntry {
        code: 0x0000,
        bits: 11,
    }, // (5,5)
];

static VLC_TABLE_9: [VlcEntry; 36] = [
    VlcEntry {
        code: 0x0007,
        bits: 3,
    }, // (0,0)
    VlcEntry {
        code: 0x0005,
        bits: 3,
    }, // (1,0)
    VlcEntry {
        code: 0x0009,
        bits: 5,
    }, // (2,0)
    VlcEntry {
        code: 0x000e,
        bits: 6,
    }, // (3,0)
    VlcEntry {
        code: 0x000f,
        bits: 8,
    }, // (4,0)
    VlcEntry {
        code: 0x0007,
        bits: 9,
    }, // (5,0)
    VlcEntry {
        code: 0x0006,
        bits: 3,
    }, // (0,1)
    VlcEntry {
        code: 0x0004,
        bits: 3,
    }, // (1,1)
    VlcEntry {
        code: 0x0005,
        bits: 4,
    }, // (2,1)
    VlcEntry {
        code: 0x0005,
        bits: 5,
    }, // (3,1)
    VlcEntry {
        code: 0x0006,
        bits: 6,
    }, // (4,1)
    VlcEntry {
        code: 0x0007,
        bits: 8,
    }, // (5,1)
    VlcEntry {
        code: 0x0007,
        bits: 4,
    }, // (0,2)
    VlcEntry {
        code: 0x0006,
        bits: 4,
    }, // (1,2)
    VlcEntry {
        code: 0x0008,
        bits: 5,
    }, // (2,2)
    VlcEntry {
        code: 0x0008,
        bits: 6,
    }, // (3,2)
    VlcEntry {
        code: 0x0008,
        bits: 7,
    }, // (4,2)
    VlcEntry {
        code: 0x0005,
        bits: 8,
    }, // (5,2)
    VlcEntry {
        code: 0x000f,
        bits: 6,
    }, // (0,3)
    VlcEntry {
        code: 0x0006,
        bits: 5,
    }, // (1,3)
    VlcEntry {
        code: 0x0009,
        bits: 6,
    }, // (2,3)
    VlcEntry {
        code: 0x000a,
        bits: 7,
    }, // (3,3)
    VlcEntry {
        code: 0x0005,
        bits: 7,
    }, // (4,3)
    VlcEntry {
        code: 0x0001,
        bits: 8,
    }, // (5,3)
    VlcEntry {
        code: 0x000b,
        bits: 7,
    }, // (0,4)
    VlcEntry {
        code: 0x0007,
        bits: 6,
    }, // (1,4)
    VlcEntry {
        code: 0x0009,
        bits: 7,
    }, // (2,4)
    VlcEntry {
        code: 0x0006,
        bits: 7,
    }, // (3,4)
    VlcEntry {
        code: 0x0004,
        bits: 8,
    }, // (4,4)
    VlcEntry {
        code: 0x0001,
        bits: 9,
    }, // (5,4)
    VlcEntry {
        code: 0x000e,
        bits: 8,
    }, // (0,5)
    VlcEntry {
        code: 0x0004,
        bits: 7,
    }, // (1,5)
    VlcEntry {
        code: 0x0006,
        bits: 8,
    }, // (2,5)
    VlcEntry {
        code: 0x0002,
        bits: 8,
    }, // (3,5)
    VlcEntry {
        code: 0x0006,
        bits: 9,
    }, // (4,5)
    VlcEntry {
        code: 0x0000,
        bits: 9,
    }, // (5,5)
];

static VLC_TABLE_10: [VlcEntry; 64] = [
    VlcEntry {
        code: 0x0001,
        bits: 1,
    }, // (0,0)
    VlcEntry {
        code: 0x0002,
        bits: 3,
    }, // (1,0)
    VlcEntry {
        code: 0x000a,
        bits: 6,
    }, // (2,0)
    VlcEntry {
        code: 0x0017,
        bits: 8,
    }, // (3,0)
    VlcEntry {
        code: 0x0023,
        bits: 9,
    }, // (4,0)
    VlcEntry {
        code: 0x001e,
        bits: 9,
    }, // (5,0)
    VlcEntry {
        code: 0x000c,
        bits: 9,
    }, // (6,0)
    VlcEntry {
        code: 0x0011,
        bits: 10,
    }, // (7,0)
    VlcEntry {
        code: 0x0003,
        bits: 3,
    }, // (0,1)
    VlcEntry {
        code: 0x0003,
        bits: 4,
    }, // (1,1)
    VlcEntry {
        code: 0x0008,
        bits: 6,
    }, // (2,1)
    VlcEntry {
        code: 0x000c,
        bits: 7,
    }, // (3,1)
    VlcEntry {
        code: 0x0012,
        bits: 8,
    }, // (4,1)
    VlcEntry {
        code: 0x0015,
        bits: 9,
    }, // (5,1)
    VlcEntry {
        code: 0x000c,
        bits: 8,
    }, // (6,1)
    VlcEntry {
        code: 0x0007,
        bits: 8,
    }, // (7,1)
    VlcEntry {
        code: 0x000b,
        bits: 6,
    }, // (0,2)
    VlcEntry {
        code: 0x0009,
        bits: 6,
    }, // (1,2)
    VlcEntry {
        code: 0x000f,
        bits: 7,
    }, // (2,2)
    VlcEntry {
        code: 0x0015,
        bits: 8,
    }, // (3,2)
    VlcEntry {
        code: 0x0020,
        bits: 9,
    }, // (4,2)
    VlcEntry {
        code: 0x0028,
        bits: 10,
    }, // (5,2)
    VlcEntry {
        code: 0x0013,
        bits: 9,
    }, // (6,2)
    VlcEntry {
        code: 0x0006,
        bits: 9,
    }, // (7,2)
    VlcEntry {
        code: 0x000e,
        bits: 7,
    }, // (0,3)
    VlcEntry {
        code: 0x000d,
        bits: 7,
    }, // (1,3)
    VlcEntry {
        code: 0x0016,
        bits: 8,
    }, // (2,3)
    VlcEntry {
        code: 0x0022,
        bits: 9,
    }, // (3,3)
    VlcEntry {
        code: 0x002e,
        bits: 10,
    }, // (4,3)
    VlcEntry {
        code: 0x0017,
        bits: 10,
    }, // (5,3)
    VlcEntry {
        code: 0x0012,
        bits: 9,
    }, // (6,3)
    VlcEntry {
        code: 0x0007,
        bits: 10,
    }, // (7,3)
    VlcEntry {
        code: 0x0014,
        bits: 8,
    }, // (0,4)
    VlcEntry {
        code: 0x0013,
        bits: 8,
    }, // (1,4)
    VlcEntry {
        code: 0x0021,
        bits: 9,
    }, // (2,4)
    VlcEntry {
        code: 0x002f,
        bits: 10,
    }, // (3,4)
    VlcEntry {
        code: 0x001b,
        bits: 10,
    }, // (4,4)
    VlcEntry {
        code: 0x0016,
        bits: 10,
    }, // (5,4)
    VlcEntry {
        code: 0x0009,
        bits: 10,
    }, // (6,4)
    VlcEntry {
        code: 0x0003,
        bits: 10,
    }, // (7,4)
    VlcEntry {
        code: 0x001f,
        bits: 9,
    }, // (0,5)
    VlcEntry {
        code: 0x0016,
        bits: 9,
    }, // (1,5)
    VlcEntry {
        code: 0x0029,
        bits: 10,
    }, // (2,5)
    VlcEntry {
        code: 0x001a,
        bits: 10,
    }, // (3,5)
    VlcEntry {
        code: 0x0015,
        bits: 11,
    }, // (4,5)
    VlcEntry {
        code: 0x0014,
        bits: 11,
    }, // (5,5)
    VlcEntry {
        code: 0x0005,
        bits: 10,
    }, // (6,5)
    VlcEntry {
        code: 0x0003,
        bits: 11,
    }, // (7,5)
    VlcEntry {
        code: 0x000e,
        bits: 8,
    }, // (0,6)
    VlcEntry {
        code: 0x000d,
        bits: 8,
    }, // (1,6)
    VlcEntry {
        code: 0x000a,
        bits: 9,
    }, // (2,6)
    VlcEntry {
        code: 0x000b,
        bits: 10,
    }, // (3,6)
    VlcEntry {
        code: 0x0010,
        bits: 10,
    }, // (4,6)
    VlcEntry {
        code: 0x0006,
        bits: 10,
    }, // (5,6)
    VlcEntry {
        code: 0x0005,
        bits: 11,
    }, // (6,6)
    VlcEntry {
        code: 0x0001,
        bits: 11,
    }, // (7,6)
    VlcEntry {
        code: 0x0009,
        bits: 9,
    }, // (0,7)
    VlcEntry {
        code: 0x0008,
        bits: 8,
    }, // (1,7)
    VlcEntry {
        code: 0x0007,
        bits: 9,
    }, // (2,7)
    VlcEntry {
        code: 0x0008,
        bits: 10,
    }, // (3,7)
    VlcEntry {
        code: 0x0004,
        bits: 10,
    }, // (4,7)
    VlcEntry {
        code: 0x0004,
        bits: 11,
    }, // (5,7)
    VlcEntry {
        code: 0x0002,
        bits: 11,
    }, // (6,7)
    VlcEntry {
        code: 0x0000,
        bits: 11,
    }, // (7,7)
];

static VLC_TABLE_11: [VlcEntry; 64] = [
    VlcEntry {
        code: 0x0003,
        bits: 2,
    }, // (0,0)
    VlcEntry {
        code: 0x0004,
        bits: 3,
    }, // (1,0)
    VlcEntry {
        code: 0x000a,
        bits: 5,
    }, // (2,0)
    VlcEntry {
        code: 0x0018,
        bits: 7,
    }, // (3,0)
    VlcEntry {
        code: 0x0022,
        bits: 8,
    }, // (4,0)
    VlcEntry {
        code: 0x0021,
        bits: 9,
    }, // (5,0)
    VlcEntry {
        code: 0x0015,
        bits: 8,
    }, // (6,0)
    VlcEntry {
        code: 0x000f,
        bits: 9,
    }, // (7,0)
    VlcEntry {
        code: 0x0005,
        bits: 3,
    }, // (0,1)
    VlcEntry {
        code: 0x0003,
        bits: 3,
    }, // (1,1)
    VlcEntry {
        code: 0x0004,
        bits: 4,
    }, // (2,1)
    VlcEntry {
        code: 0x000a,
        bits: 6,
    }, // (3,1)
    VlcEntry {
        code: 0x0020,
        bits: 8,
    }, // (4,1)
    VlcEntry {
        code: 0x0011,
        bits: 8,
    }, // (5,1)
    VlcEntry {
        code: 0x000b,
        bits: 7,
    }, // (6,1)
    VlcEntry {
        code: 0x000a,
        bits: 8,
    }, // (7,1)
    VlcEntry {
        code: 0x000b,
        bits: 5,
    }, // (0,2)
    VlcEntry {
        code: 0x0007,
        bits: 5,
    }, // (1,2)
    VlcEntry {
        code: 0x000d,
        bits: 6,
    }, // (2,2)
    VlcEntry {
        code: 0x0012,
        bits: 7,
    }, // (3,2)
    VlcEntry {
        code: 0x001e,
        bits: 8,
    }, // (4,2)
    VlcEntry {
        code: 0x001f,
        bits: 9,
    }, // (5,2)
    VlcEntry {
        code: 0x0014,
        bits: 8,
    }, // (6,2)
    VlcEntry {
        code: 0x0005,
        bits: 8,
    }, // (7,2)
    VlcEntry {
        code: 0x0019,
        bits: 7,
    }, // (0,3)
    VlcEntry {
        code: 0x000b,
        bits: 6,
    }, // (1,3)
    VlcEntry {
        code: 0x0013,
        bits: 7,
    }, // (2,3)
    VlcEntry {
        code: 0x003b,
        bits: 9,
    }, // (3,3)
    VlcEntry {
        code: 0x001b,
        bits: 8,
    }, // (4,3)
    VlcEntry {
        code: 0x0012,
        bits: 10,
    }, // (5,3)
    VlcEntry {
        code: 0x000c,
        bits: 8,
    }, // (6,3)
    VlcEntry {
        code: 0x0005,
        bits: 9,
    }, // (7,3)
    VlcEntry {
        code: 0x0023,
        bits: 8,
    }, // (0,4)
    VlcEntry {
        code: 0x0021,
        bits: 8,
    }, // (1,4)
    VlcEntry {
        code: 0x001f,
        bits: 8,
    }, // (2,4)
    VlcEntry {
        code: 0x003a,
        bits: 9,
    }, // (3,4)
    VlcEntry {
        code: 0x001e,
        bits: 9,
    }, // (4,4)
    VlcEntry {
        code: 0x0010,
        bits: 10,
    }, // (5,4)
    VlcEntry {
        code: 0x0007,
        bits: 9,
    }, // (6,4)
    VlcEntry {
        code: 0x0005,
        bits: 10,
    }, // (7,4)
    VlcEntry {
        code: 0x001c,
        bits: 8,
    }, // (0,5)
    VlcEntry {
        code: 0x001a,
        bits: 8,
    }, // (1,5)
    VlcEntry {
        code: 0x0020,
        bits: 9,
    }, // (2,5)
    VlcEntry {
        code: 0x0013,
        bits: 10,
    }, // (3,5)
    VlcEntry {
        code: 0x0011,
        bits: 10,
    }, // (4,5)
    VlcEntry {
        code: 0x000f,
        bits: 11,
    }, // (5,5)
    VlcEntry {
        code: 0x0008,
        bits: 10,
    }, // (6,5)
    VlcEntry {
        code: 0x000e,
        bits: 11,
    }, // (7,5)
    VlcEntry {
        code: 0x000e,
        bits: 8,
    }, // (0,6)
    VlcEntry {
        code: 0x000c,
        bits: 7,
    }, // (1,6)
    VlcEntry {
        code: 0x0009,
        bits: 7,
    }, // (2,6)
    VlcEntry {
        code: 0x000d,
        bits: 8,
    }, // (3,6)
    VlcEntry {
        code: 0x000e,
        bits: 9,
    }, // (4,6)
    VlcEntry {
        code: 0x0009,
        bits: 10,
    }, // (5,6)
    VlcEntry {
        code: 0x0004,
        bits: 10,
    }, // (6,6)
    VlcEntry {
        code: 0x0001,
        bits: 10,
    }, // (7,6)
    VlcEntry {
        code: 0x000b,
        bits: 8,
    }, // (0,7)
    VlcEntry {
        code: 0x0004,
        bits: 7,
    }, // (1,7)
    VlcEntry {
        code: 0x0006,
        bits: 8,
    }, // (2,7)
    VlcEntry {
        code: 0x0006,
        bits: 9,
    }, // (3,7)
    VlcEntry {
        code: 0x0006,
        bits: 10,
    }, // (4,7)
    VlcEntry {
        code: 0x0003,
        bits: 10,
    }, // (5,7)
    VlcEntry {
        code: 0x0002,
        bits: 10,
    }, // (6,7)
    VlcEntry {
        code: 0x0000,
        bits: 10,
    }, // (7,7)
];

static VLC_TABLE_12: [VlcEntry; 64] = [
    VlcEntry {
        code: 0x0009,
        bits: 4,
    }, // (0,0)
    VlcEntry {
        code: 0x0006,
        bits: 3,
    }, // (1,0)
    VlcEntry {
        code: 0x0010,
        bits: 5,
    }, // (2,0)
    VlcEntry {
        code: 0x0021,
        bits: 7,
    }, // (3,0)
    VlcEntry {
        code: 0x0029,
        bits: 8,
    }, // (4,0)
    VlcEntry {
        code: 0x0027,
        bits: 9,
    }, // (5,0)
    VlcEntry {
        code: 0x0026,
        bits: 9,
    }, // (6,0)
    VlcEntry {
        code: 0x001a,
        bits: 9,
    }, // (7,0)
    VlcEntry {
        code: 0x0007,
        bits: 3,
    }, // (0,1)
    VlcEntry {
        code: 0x0005,
        bits: 3,
    }, // (1,1)
    VlcEntry {
        code: 0x0006,
        bits: 4,
    }, // (2,1)
    VlcEntry {
        code: 0x0009,
        bits: 5,
    }, // (3,1)
    VlcEntry {
        code: 0x0017,
        bits: 7,
    }, // (4,1)
    VlcEntry {
        code: 0x0010,
        bits: 7,
    }, // (5,1)
    VlcEntry {
        code: 0x001a,
        bits: 8,
    }, // (6,1)
    VlcEntry {
        code: 0x000b,
        bits: 8,
    }, // (7,1)
    VlcEntry {
        code: 0x0011,
        bits: 5,
    }, // (0,2)
    VlcEntry {
        code: 0x0007,
        bits: 4,
    }, // (1,2)
    VlcEntry {
        code: 0x000b,
        bits: 5,
    }, // (2,2)
    VlcEntry {
        code: 0x000e,
        bits: 6,
    }, // (3,2)
    VlcEntry {
        code: 0x0015,
        bits: 7,
    }, // (4,2)
    VlcEntry {
        code: 0x001e,
        bits: 8,
    }, // (5,2)
    VlcEntry {
        code: 0x000a,
        bits: 7,
    }, // (6,2)
    VlcEntry {
        code: 0x0007,
        bits: 8,
    }, // (7,2)
    VlcEntry {
        code: 0x0011,
        bits: 6,
    }, // (0,3)
    VlcEntry {
        code: 0x000a,
        bits: 5,
    }, // (1,3)
    VlcEntry {
        code: 0x000f,
        bits: 6,
    }, // (2,3)
    VlcEntry {
        code: 0x000c,
        bits: 6,
    }, // (3,3)
    VlcEntry {
        code: 0x0012,
        bits: 7,
    }, // (4,3)
    VlcEntry {
        code: 0x001c,
        bits: 8,
    }, // (5,3)
    VlcEntry {
        code: 0x000e,
        bits: 8,
    }, // (6,3)
    VlcEntry {
        code: 0x0005,
        bits: 8,
    }, // (7,3)
    VlcEntry {
        code: 0x0020,
        bits: 7,
    }, // (0,4)
    VlcEntry {
        code: 0x000d,
        bits: 6,
    }, // (1,4)
    VlcEntry {
        code: 0x0016,
        bits: 7,
    }, // (2,4)
    VlcEntry {
        code: 0x0013,
        bits: 7,
    }, // (3,4)
    VlcEntry {
        code: 0x0012,
        bits: 8,
    }, // (4,4)
    VlcEntry {
        code: 0x0010,
        bits: 8,
    }, // (5,4)
    VlcEntry {
        code: 0x0009,
        bits: 8,
    }, // (6,4)
    VlcEntry {
        code: 0x0005,
        bits: 9,
    }, // (7,4)
    VlcEntry {
        code: 0x0028,
        bits: 8,
    }, // (0,5)
    VlcEntry {
        code: 0x0011,
        bits: 7,
    }, // (1,5)
    VlcEntry {
        code: 0x001f,
        bits: 8,
    }, // (2,5)
    VlcEntry {
        code: 0x001d,
        bits: 8,
    }, // (3,5)
    VlcEntry {
        code: 0x0011,
        bits: 8,
    }, // (4,5)
    VlcEntry {
        code: 0x000d,
        bits: 9,
    }, // (5,5)
    VlcEntry {
        code: 0x0004,
        bits: 8,
    }, // (6,5)
    VlcEntry {
        code: 0x0002,
        bits: 9,
    }, // (7,5)
    VlcEntry {
        code: 0x001b,
        bits: 8,
    }, // (0,6)
    VlcEntry {
        code: 0x000c,
        bits: 7,
    }, // (1,6)
    VlcEntry {
        code: 0x000b,
        bits: 7,
    }, // (2,6)
    VlcEntry {
        code: 0x000f,
        bits: 8,
    }, // (3,6)
    VlcEntry {
        code: 0x000a,
        bits: 8,
    }, // (4,6)
    VlcEntry {
        code: 0x0007,
        bits: 9,
    }, // (5,6)
    VlcEntry {
        code: 0x0004,
        bits: 9,
    }, // (6,6)
    VlcEntry {
        code: 0x0001,
        bits: 10,
    }, // (7,6)
    VlcEntry {
        code: 0x001b,
        bits: 9,
    }, // (0,7)
    VlcEntry {
        code: 0x000c,
        bits: 8,
    }, // (1,7)
    VlcEntry {
        code: 0x0008,
        bits: 8,
    }, // (2,7)
    VlcEntry {
        code: 0x000c,
        bits: 9,
    }, // (3,7)
    VlcEntry {
        code: 0x0006,
        bits: 9,
    }, // (4,7)
    VlcEntry {
        code: 0x0003,
        bits: 9,
    }, // (5,7)
    VlcEntry {
        code: 0x0001,
        bits: 9,
    }, // (6,7)
    VlcEntry {
        code: 0x0000,
        bits: 10,
    }, // (7,7)
];

static VLC_TABLE_13: [VlcEntry; 256] = [
    VlcEntry {
        code: 0x0001,
        bits: 1,
    }, // (0,0)
    VlcEntry {
        code: 0x0005,
        bits: 4,
    }, // (1,0)
    VlcEntry {
        code: 0x000e,
        bits: 6,
    }, // (2,0)
    VlcEntry {
        code: 0x0015,
        bits: 7,
    }, // (3,0)
    VlcEntry {
        code: 0x0022,
        bits: 8,
    }, // (4,0)
    VlcEntry {
        code: 0x0033,
        bits: 9,
    }, // (5,0)
    VlcEntry {
        code: 0x002e,
        bits: 9,
    }, // (6,0)
    VlcEntry {
        code: 0x0047,
        bits: 10,
    }, // (7,0)
    VlcEntry {
        code: 0x002a,
        bits: 9,
    }, // (8,0)
    VlcEntry {
        code: 0x0034,
        bits: 10,
    }, // (9,0)
    VlcEntry {
        code: 0x0044,
        bits: 11,
    }, // (10,0)
    VlcEntry {
        code: 0x0034,
        bits: 11,
    }, // (11,0)
    VlcEntry {
        code: 0x0043,
        bits: 12,
    }, // (12,0)
    VlcEntry {
        code: 0x002c,
        bits: 12,
    }, // (13,0)
    VlcEntry {
        code: 0x002b,
        bits: 13,
    }, // (14,0)
    VlcEntry {
        code: 0x0013,
        bits: 13,
    }, // (15,0)
    VlcEntry {
        code: 0x0003,
        bits: 3,
    }, // (0,1)
    VlcEntry {
        code: 0x0004,
        bits: 4,
    }, // (1,1)
    VlcEntry {
        code: 0x000c,
        bits: 6,
    }, // (2,1)
    VlcEntry {
        code: 0x0013,
        bits: 7,
    }, // (3,1)
    VlcEntry {
        code: 0x001f,
        bits: 8,
    }, // (4,1)
    VlcEntry {
        code: 0x001a,
        bits: 8,
    }, // (5,1)
    VlcEntry {
        code: 0x002c,
        bits: 9,
    }, // (6,1)
    VlcEntry {
        code: 0x0021,
        bits: 9,
    }, // (7,1)
    VlcEntry {
        code: 0x001f,
        bits: 9,
    }, // (8,1)
    VlcEntry {
        code: 0x0018,
        bits: 9,
    }, // (9,1)
    VlcEntry {
        code: 0x0020,
        bits: 10,
    }, // (10,1)
    VlcEntry {
        code: 0x0018,
        bits: 10,
    }, // (11,1)
    VlcEntry {
        code: 0x001f,
        bits: 11,
    }, // (12,1)
    VlcEntry {
        code: 0x0023,
        bits: 12,
    }, // (13,1)
    VlcEntry {
        code: 0x0016,
        bits: 12,
    }, // (14,1)
    VlcEntry {
        code: 0x000e,
        bits: 12,
    }, // (15,1)
    VlcEntry {
        code: 0x000f,
        bits: 6,
    }, // (0,2)
    VlcEntry {
        code: 0x000d,
        bits: 6,
    }, // (1,2)
    VlcEntry {
        code: 0x0017,
        bits: 7,
    }, // (2,2)
    VlcEntry {
        code: 0x0024,
        bits: 8,
    }, // (3,2)
    VlcEntry {
        code: 0x003b,
        bits: 9,
    }, // (4,2)
    VlcEntry {
        code: 0x0031,
        bits: 9,
    }, // (5,2)
    VlcEntry {
        code: 0x004d,
        bits: 10,
    }, // (6,2)
    VlcEntry {
        code: 0x0041,
        bits: 10,
    }, // (7,2)
    VlcEntry {
        code: 0x001d,
        bits: 9,
    }, // (8,2)
    VlcEntry {
        code: 0x0028,
        bits: 10,
    }, // (9,2)
    VlcEntry {
        code: 0x001e,
        bits: 10,
    }, // (10,2)
    VlcEntry {
        code: 0x0028,
        bits: 11,
    }, // (11,2)
    VlcEntry {
        code: 0x001b,
        bits: 11,
    }, // (12,2)
    VlcEntry {
        code: 0x0021,
        bits: 12,
    }, // (13,2)
    VlcEntry {
        code: 0x002a,
        bits: 13,
    }, // (14,2)
    VlcEntry {
        code: 0x0010,
        bits: 13,
    }, // (15,2)
    VlcEntry {
        code: 0x0016,
        bits: 7,
    }, // (0,3)
    VlcEntry {
        code: 0x0014,
        bits: 7,
    }, // (1,3)
    VlcEntry {
        code: 0x0025,
        bits: 8,
    }, // (2,3)
    VlcEntry {
        code: 0x003d,
        bits: 9,
    }, // (3,3)
    VlcEntry {
        code: 0x0038,
        bits: 9,
    }, // (4,3)
    VlcEntry {
        code: 0x004f,
        bits: 10,
    }, // (5,3)
    VlcEntry {
        code: 0x0049,
        bits: 10,
    }, // (6,3)
    VlcEntry {
        code: 0x0040,
        bits: 10,
    }, // (7,3)
    VlcEntry {
        code: 0x002b,
        bits: 10,
    }, // (8,3)
    VlcEntry {
        code: 0x004c,
        bits: 11,
    }, // (9,3)
    VlcEntry {
        code: 0x0038,
        bits: 11,
    }, // (10,3)
    VlcEntry {
        code: 0x0025,
        bits: 11,
    }, // (11,3)
    VlcEntry {
        code: 0x001a,
        bits: 11,
    }, // (12,3)
    VlcEntry {
        code: 0x001f,
        bits: 12,
    }, // (13,3)
    VlcEntry {
        code: 0x0019,
        bits: 13,
    }, // (14,3)
    VlcEntry {
        code: 0x000e,
        bits: 13,
    }, // (15,3)
    VlcEntry {
        code: 0x0023,
        bits: 8,
    }, // (0,4)
    VlcEntry {
        code: 0x0010,
        bits: 7,
    }, // (1,4)
    VlcEntry {
        code: 0x003c,
        bits: 9,
    }, // (2,4)
    VlcEntry {
        code: 0x0039,
        bits: 9,
    }, // (3,4)
    VlcEntry {
        code: 0x0061,
        bits: 10,
    }, // (4,4)
    VlcEntry {
        code: 0x004b,
        bits: 10,
    }, // (5,4)
    VlcEntry {
        code: 0x0072,
        bits: 11,
    }, // (6,4)
    VlcEntry {
        code: 0x005b,
        bits: 11,
    }, // (7,4)
    VlcEntry {
        code: 0x0036,
        bits: 10,
    }, // (8,4)
    VlcEntry {
        code: 0x0049,
        bits: 11,
    }, // (9,4)
    VlcEntry {
        code: 0x0037,
        bits: 11,
    }, // (10,4)
    VlcEntry {
        code: 0x0029,
        bits: 12,
    }, // (11,4)
    VlcEntry {
        code: 0x0030,
        bits: 12,
    }, // (12,4)
    VlcEntry {
        code: 0x0035,
        bits: 13,
    }, // (13,4)
    VlcEntry {
        code: 0x0017,
        bits: 13,
    }, // (14,4)
    VlcEntry {
        code: 0x0018,
        bits: 14,
    }, // (15,4)
    VlcEntry {
        code: 0x003a,
        bits: 9,
    }, // (0,5)
    VlcEntry {
        code: 0x001b,
        bits: 8,
    }, // (1,5)
    VlcEntry {
        code: 0x0032,
        bits: 9,
    }, // (2,5)
    VlcEntry {
        code: 0x0060,
        bits: 10,
    }, // (3,5)
    VlcEntry {
        code: 0x004c,
        bits: 10,
    }, // (4,5)
    VlcEntry {
        code: 0x0046,
        bits: 10,
    }, // (5,5)
    VlcEntry {
        code: 0x005d,
        bits: 11,
    }, // (6,5)
    VlcEntry {
        code: 0x0054,
        bits: 11,
    }, // (7,5)
    VlcEntry {
        code: 0x004d,
        bits: 11,
    }, // (8,5)
    VlcEntry {
        code: 0x003a,
        bits: 11,
    }, // (9,5)
    VlcEntry {
        code: 0x004f,
        bits: 12,
    }, // (10,5)
    VlcEntry {
        code: 0x001d,
        bits: 11,
    }, // (11,5)
    VlcEntry {
        code: 0x004a,
        bits: 13,
    }, // (12,5)
    VlcEntry {
        code: 0x0031,
        bits: 13,
    }, // (13,5)
    VlcEntry {
        code: 0x0029,
        bits: 14,
    }, // (14,5)
    VlcEntry {
        code: 0x0011,
        bits: 14,
    }, // (15,5)
    VlcEntry {
        code: 0x002f,
        bits: 9,
    }, // (0,6)
    VlcEntry {
        code: 0x002d,
        bits: 9,
    }, // (1,6)
    VlcEntry {
        code: 0x004e,
        bits: 10,
    }, // (2,6)
    VlcEntry {
        code: 0x004a,
        bits: 10,
    }, // (3,6)
    VlcEntry {
        code: 0x0073,
        bits: 11,
    }, // (4,6)
    VlcEntry {
        code: 0x005e,
        bits: 11,
    }, // (5,6)
    VlcEntry {
        code: 0x005a,
        bits: 11,
    }, // (6,6)
    VlcEntry {
        code: 0x004f,
        bits: 11,
    }, // (7,6)
    VlcEntry {
        code: 0x0045,
        bits: 11,
    }, // (8,6)
    VlcEntry {
        code: 0x0053,
        bits: 12,
    }, // (9,6)
    VlcEntry {
        code: 0x0047,
        bits: 12,
    }, // (10,6)
    VlcEntry {
        code: 0x0032,
        bits: 12,
    }, // (11,6)
    VlcEntry {
        code: 0x003b,
        bits: 13,
    }, // (12,6)
    VlcEntry {
        code: 0x0026,
        bits: 13,
    }, // (13,6)
    VlcEntry {
        code: 0x0024,
        bits: 14,
    }, // (14,6)
    VlcEntry {
        code: 0x000f,
        bits: 14,
    }, // (15,6)
    VlcEntry {
        code: 0x0048,
        bits: 10,
    }, // (0,7)
    VlcEntry {
        code: 0x0022,
        bits: 9,
    }, // (1,7)
    VlcEntry {
        code: 0x0038,
        bits: 10,
    }, // (2,7)
    VlcEntry {
        code: 0x005f,
        bits: 11,
    }, // (3,7)
    VlcEntry {
        code: 0x005c,
        bits: 11,
    }, // (4,7)
    VlcEntry {
        code: 0x0055,
        bits: 11,
    }, // (5,7)
    VlcEntry {
        code: 0x005b,
        bits: 12,
    }, // (6,7)
    VlcEntry {
        code: 0x005a,
        bits: 12,
    }, // (7,7)
    VlcEntry {
        code: 0x0056,
        bits: 12,
    }, // (8,7)
    VlcEntry {
        code: 0x0049,
        bits: 12,
    }, // (9,7)
    VlcEntry {
        code: 0x004d,
        bits: 13,
    }, // (10,7)
    VlcEntry {
        code: 0x0041,
        bits: 13,
    }, // (11,7)
    VlcEntry {
        code: 0x0033,
        bits: 13,
    }, // (12,7)
    VlcEntry {
        code: 0x002c,
        bits: 14,
    }, // (13,7)
    VlcEntry {
        code: 0x002b,
        bits: 16,
    }, // (14,7)
    VlcEntry {
        code: 0x002a,
        bits: 16,
    }, // (15,7)
    VlcEntry {
        code: 0x002b,
        bits: 9,
    }, // (0,8)
    VlcEntry {
        code: 0x0014,
        bits: 8,
    }, // (1,8)
    VlcEntry {
        code: 0x001e,
        bits: 9,
    }, // (2,8)
    VlcEntry {
        code: 0x002c,
        bits: 10,
    }, // (3,8)
    VlcEntry {
        code: 0x0037,
        bits: 10,
    }, // (4,8)
    VlcEntry {
        code: 0x004e,
        bits: 11,
    }, // (5,8)
    VlcEntry {
        code: 0x0048,
        bits: 11,
    }, // (6,8)
    VlcEntry {
        code: 0x0057,
        bits: 12,
    }, // (7,8)
    VlcEntry {
        code: 0x004e,
        bits: 12,
    }, // (8,8)
    VlcEntry {
        code: 0x003d,
        bits: 12,
    }, // (9,8)
    VlcEntry {
        code: 0x002e,
        bits: 12,
    }, // (10,8)
    VlcEntry {
        code: 0x0036,
        bits: 13,
    }, // (11,8)
    VlcEntry {
        code: 0x0025,
        bits: 13,
    }, // (12,8)
    VlcEntry {
        code: 0x001e,
        bits: 14,
    }, // (13,8)
    VlcEntry {
        code: 0x0014,
        bits: 15,
    }, // (14,8)
    VlcEntry {
        code: 0x0010,
        bits: 15,
    }, // (15,8)
    VlcEntry {
        code: 0x0035,
        bits: 10,
    }, // (0,9)
    VlcEntry {
        code: 0x0019,
        bits: 9,
    }, // (1,9)
    VlcEntry {
        code: 0x0029,
        bits: 10,
    }, // (2,9)
    VlcEntry {
        code: 0x0025,
        bits: 10,
    }, // (3,9)
    VlcEntry {
        code: 0x002c,
        bits: 11,
    }, // (4,9)
    VlcEntry {
        code: 0x003b,
        bits: 11,
    }, // (5,9)
    VlcEntry {
        code: 0x0036,
        bits: 11,
    }, // (6,9)
    VlcEntry {
        code: 0x0051,
        bits: 13,
    }, // (7,9)
    VlcEntry {
        code: 0x0042,
        bits: 12,
    }, // (8,9)
    VlcEntry {
        code: 0x004c,
        bits: 13,
    }, // (9,9)
    VlcEntry {
        code: 0x0039,
        bits: 13,
    }, // (10,9)
    VlcEntry {
        code: 0x0036,
        bits: 14,
    }, // (11,9)
    VlcEntry {
        code: 0x0025,
        bits: 14,
    }, // (12,9)
    VlcEntry {
        code: 0x0012,
        bits: 14,
    }, // (13,9)
    VlcEntry {
        code: 0x0027,
        bits: 16,
    }, // (14,9)
    VlcEntry {
        code: 0x000b,
        bits: 15,
    }, // (15,9)
    VlcEntry {
        code: 0x0023,
        bits: 10,
    }, // (0,10)
    VlcEntry {
        code: 0x0021,
        bits: 10,
    }, // (1,10)
    VlcEntry {
        code: 0x001f,
        bits: 10,
    }, // (2,10)
    VlcEntry {
        code: 0x0039,
        bits: 11,
    }, // (3,10)
    VlcEntry {
        code: 0x002a,
        bits: 11,
    }, // (4,10)
    VlcEntry {
        code: 0x0052,
        bits: 12,
    }, // (5,10)
    VlcEntry {
        code: 0x0048,
        bits: 12,
    }, // (6,10)
    VlcEntry {
        code: 0x0050,
        bits: 13,
    }, // (7,10)
    VlcEntry {
        code: 0x002f,
        bits: 12,
    }, // (8,10)
    VlcEntry {
        code: 0x003a,
        bits: 13,
    }, // (9,10)
    VlcEntry {
        code: 0x0037,
        bits: 14,
    }, // (10,10)
    VlcEntry {
        code: 0x0015,
        bits: 13,
    }, // (11,10)
    VlcEntry {
        code: 0x0016,
        bits: 14,
    }, // (12,10)
    VlcEntry {
        code: 0x001a,
        bits: 15,
    }, // (13,10)
    VlcEntry {
        code: 0x0026,
        bits: 16,
    }, // (14,10)
    VlcEntry {
        code: 0x0016,
        bits: 17,
    }, // (15,10)
    VlcEntry {
        code: 0x0035,
        bits: 11,
    }, // (0,11)
    VlcEntry {
        code: 0x0019,
        bits: 10,
    }, // (1,11)
    VlcEntry {
        code: 0x0017,
        bits: 10,
    }, // (2,11)
    VlcEntry {
        code: 0x0026,
        bits: 11,
    }, // (3,11)
    VlcEntry {
        code: 0x0046,
        bits: 12,
    }, // (4,11)
    VlcEntry {
        code: 0x003c,
        bits: 12,
    }, // (5,11)
    VlcEntry {
        code: 0x0033,
        bits: 12,
    }, // (6,11)
    VlcEntry {
        code: 0x0024,
        bits: 12,
    }, // (7,11)
    VlcEntry {
        code: 0x0037,
        bits: 13,
    }, // (8,11)
    VlcEntry {
        code: 0x001a,
        bits: 13,
    }, // (9,11)
    VlcEntry {
        code: 0x0022,
        bits: 13,
    }, // (10,11)
    VlcEntry {
        code: 0x0017,
        bits: 14,
    }, // (11,11)
    VlcEntry {
        code: 0x001b,
        bits: 15,
    }, // (12,11)
    VlcEntry {
        code: 0x000e,
        bits: 15,
    }, // (13,11)
    VlcEntry {
        code: 0x0009,
        bits: 15,
    }, // (14,11)
    VlcEntry {
        code: 0x0007,
        bits: 16,
    }, // (15,11)
    VlcEntry {
        code: 0x0022,
        bits: 11,
    }, // (0,12)
    VlcEntry {
        code: 0x0020,
        bits: 11,
    }, // (1,12)
    VlcEntry {
        code: 0x001c,
        bits: 11,
    }, // (2,12)
    VlcEntry {
        code: 0x0027,
        bits: 12,
    }, // (3,12)
    VlcEntry {
        code: 0x0031,
        bits: 12,
    }, // (4,12)
    VlcEntry {
        code: 0x004b,
        bits: 13,
    }, // (5,12)
    VlcEntry {
        code: 0x001e,
        bits: 12,
    }, // (6,12)
    VlcEntry {
        code: 0x0034,
        bits: 13,
    }, // (7,12)
    VlcEntry {
        code: 0x0030,
        bits: 14,
    }, // (8,12)
    VlcEntry {
        code: 0x0028,
        bits: 14,
    }, // (9,12)
    VlcEntry {
        code: 0x0034,
        bits: 15,
    }, // (10,12)
    VlcEntry {
        code: 0x001c,
        bits: 15,
    }, // (11,12)
    VlcEntry {
        code: 0x0012,
        bits: 15,
    }, // (12,12)
    VlcEntry {
        code: 0x0011,
        bits: 16,
    }, // (13,12)
    VlcEntry {
        code: 0x0009,
        bits: 16,
    }, // (14,12)
    VlcEntry {
        code: 0x0005,
        bits: 16,
    }, // (15,12)
    VlcEntry {
        code: 0x002d,
        bits: 12,
    }, // (0,13)
    VlcEntry {
        code: 0x0015,
        bits: 11,
    }, // (1,13)
    VlcEntry {
        code: 0x0022,
        bits: 12,
    }, // (2,13)
    VlcEntry {
        code: 0x0040,
        bits: 13,
    }, // (3,13)
    VlcEntry {
        code: 0x0038,
        bits: 13,
    }, // (4,13)
    VlcEntry {
        code: 0x0032,
        bits: 13,
    }, // (5,13)
    VlcEntry {
        code: 0x0031,
        bits: 14,
    }, // (6,13)
    VlcEntry {
        code: 0x002d,
        bits: 14,
    }, // (7,13)
    VlcEntry {
        code: 0x001f,
        bits: 14,
    }, // (8,13)
    VlcEntry {
        code: 0x0013,
        bits: 14,
    }, // (9,13)
    VlcEntry {
        code: 0x000c,
        bits: 14,
    }, // (10,13)
    VlcEntry {
        code: 0x000f,
        bits: 15,
    }, // (11,13)
    VlcEntry {
        code: 0x000a,
        bits: 16,
    }, // (12,13)
    VlcEntry {
        code: 0x0007,
        bits: 15,
    }, // (13,13)
    VlcEntry {
        code: 0x0006,
        bits: 16,
    }, // (14,13)
    VlcEntry {
        code: 0x0003,
        bits: 16,
    }, // (15,13)
    VlcEntry {
        code: 0x0030,
        bits: 13,
    }, // (0,14)
    VlcEntry {
        code: 0x0017,
        bits: 12,
    }, // (1,14)
    VlcEntry {
        code: 0x0014,
        bits: 12,
    }, // (2,14)
    VlcEntry {
        code: 0x0027,
        bits: 13,
    }, // (3,14)
    VlcEntry {
        code: 0x0024,
        bits: 13,
    }, // (4,14)
    VlcEntry {
        code: 0x0023,
        bits: 13,
    }, // (5,14)
    VlcEntry {
        code: 0x0035,
        bits: 15,
    }, // (6,14)
    VlcEntry {
        code: 0x0015,
        bits: 14,
    }, // (7,14)
    VlcEntry {
        code: 0x0010,
        bits: 14,
    }, // (8,14)
    VlcEntry {
        code: 0x0017,
        bits: 17,
    }, // (9,14)
    VlcEntry {
        code: 0x000d,
        bits: 15,
    }, // (10,14)
    VlcEntry {
        code: 0x000a,
        bits: 15,
    }, // (11,14)
    VlcEntry {
        code: 0x0006,
        bits: 15,
    }, // (12,14)
    VlcEntry {
        code: 0x0001,
        bits: 17,
    }, // (13,14)
    VlcEntry {
        code: 0x0004,
        bits: 16,
    }, // (14,14)
    VlcEntry {
        code: 0x0002,
        bits: 16,
    }, // (15,14)
    VlcEntry {
        code: 0x0010,
        bits: 12,
    }, // (0,15)
    VlcEntry {
        code: 0x000f,
        bits: 12,
    }, // (1,15)
    VlcEntry {
        code: 0x0011,
        bits: 13,
    }, // (2,15)
    VlcEntry {
        code: 0x001b,
        bits: 14,
    }, // (3,15)
    VlcEntry {
        code: 0x0019,
        bits: 14,
    }, // (4,15)
    VlcEntry {
        code: 0x0014,
        bits: 14,
    }, // (5,15)
    VlcEntry {
        code: 0x001d,
        bits: 15,
    }, // (6,15)
    VlcEntry {
        code: 0x000b,
        bits: 14,
    }, // (7,15)
    VlcEntry {
        code: 0x0011,
        bits: 15,
    }, // (8,15)
    VlcEntry {
        code: 0x000c,
        bits: 15,
    }, // (9,15)
    VlcEntry {
        code: 0x0010,
        bits: 16,
    }, // (10,15)
    VlcEntry {
        code: 0x0008,
        bits: 16,
    }, // (11,15)
    VlcEntry {
        code: 0x0001,
        bits: 19,
    }, // (12,15)
    VlcEntry {
        code: 0x0001,
        bits: 18,
    }, // (13,15)
    VlcEntry {
        code: 0x0000,
        bits: 19,
    }, // (14,15)
    VlcEntry {
        code: 0x0001,
        bits: 16,
    }, // (15,15)
];

static VLC_TABLE_15: [VlcEntry; 256] = [
    VlcEntry {
        code: 0x0007,
        bits: 3,
    }, // (0,0)
    VlcEntry {
        code: 0x000c,
        bits: 4,
    }, // (1,0)
    VlcEntry {
        code: 0x0012,
        bits: 5,
    }, // (2,0)
    VlcEntry {
        code: 0x0035,
        bits: 7,
    }, // (3,0)
    VlcEntry {
        code: 0x002f,
        bits: 7,
    }, // (4,0)
    VlcEntry {
        code: 0x004c,
        bits: 8,
    }, // (5,0)
    VlcEntry {
        code: 0x007c,
        bits: 9,
    }, // (6,0)
    VlcEntry {
        code: 0x006c,
        bits: 9,
    }, // (7,0)
    VlcEntry {
        code: 0x0059,
        bits: 9,
    }, // (8,0)
    VlcEntry {
        code: 0x007b,
        bits: 10,
    }, // (9,0)
    VlcEntry {
        code: 0x006c,
        bits: 10,
    }, // (10,0)
    VlcEntry {
        code: 0x0077,
        bits: 11,
    }, // (11,0)
    VlcEntry {
        code: 0x006b,
        bits: 11,
    }, // (12,0)
    VlcEntry {
        code: 0x0051,
        bits: 11,
    }, // (13,0)
    VlcEntry {
        code: 0x007a,
        bits: 12,
    }, // (14,0)
    VlcEntry {
        code: 0x003f,
        bits: 13,
    }, // (15,0)
    VlcEntry {
        code: 0x000d,
        bits: 4,
    }, // (0,1)
    VlcEntry {
        code: 0x0005,
        bits: 3,
    }, // (1,1)
    VlcEntry {
        code: 0x0010,
        bits: 5,
    }, // (2,1)
    VlcEntry {
        code: 0x001b,
        bits: 6,
    }, // (3,1)
    VlcEntry {
        code: 0x002e,
        bits: 7,
    }, // (4,1)
    VlcEntry {
        code: 0x0024,
        bits: 7,
    }, // (5,1)
    VlcEntry {
        code: 0x003d,
        bits: 8,
    }, // (6,1)
    VlcEntry {
        code: 0x0033,
        bits: 8,
    }, // (7,1)
    VlcEntry {
        code: 0x002a,
        bits: 8,
    }, // (8,1)
    VlcEntry {
        code: 0x0046,
        bits: 9,
    }, // (9,1)
    VlcEntry {
        code: 0x0034,
        bits: 9,
    }, // (10,1)
    VlcEntry {
        code: 0x0053,
        bits: 10,
    }, // (11,1)
    VlcEntry {
        code: 0x0041,
        bits: 10,
    }, // (12,1)
    VlcEntry {
        code: 0x0029,
        bits: 10,
    }, // (13,1)
    VlcEntry {
        code: 0x003b,
        bits: 11,
    }, // (14,1)
    VlcEntry {
        code: 0x0024,
        bits: 11,
    }, // (15,1)
    VlcEntry {
        code: 0x0013,
        bits: 5,
    }, // (0,2)
    VlcEntry {
        code: 0x0011,
        bits: 5,
    }, // (1,2)
    VlcEntry {
        code: 0x000f,
        bits: 5,
    }, // (2,2)
    VlcEntry {
        code: 0x0018,
        bits: 6,
    }, // (3,2)
    VlcEntry {
        code: 0x0029,
        bits: 7,
    }, // (4,2)
    VlcEntry {
        code: 0x0022,
        bits: 7,
    }, // (5,2)
    VlcEntry {
        code: 0x003b,
        bits: 8,
    }, // (6,2)
    VlcEntry {
        code: 0x0030,
        bits: 8,
    }, // (7,2)
    VlcEntry {
        code: 0x0028,
        bits: 8,
    }, // (8,2)
    VlcEntry {
        code: 0x0040,
        bits: 9,
    }, // (9,2)
    VlcEntry {
        code: 0x0032,
        bits: 9,
    }, // (10,2)
    VlcEntry {
        code: 0x004e,
        bits: 10,
    }, // (11,2)
    VlcEntry {
        code: 0x003e,
        bits: 10,
    }, // (12,2)
    VlcEntry {
        code: 0x0050,
        bits: 11,
    }, // (13,2)
    VlcEntry {
        code: 0x0038,
        bits: 11,
    }, // (14,2)
    VlcEntry {
        code: 0x0021,
        bits: 11,
    }, // (15,2)
    VlcEntry {
        code: 0x001d,
        bits: 6,
    }, // (0,3)
    VlcEntry {
        code: 0x001c,
        bits: 6,
    }, // (1,3)
    VlcEntry {
        code: 0x0019,
        bits: 6,
    }, // (2,3)
    VlcEntry {
        code: 0x002b,
        bits: 7,
    }, // (3,3)
    VlcEntry {
        code: 0x0027,
        bits: 7,
    }, // (4,3)
    VlcEntry {
        code: 0x003f,
        bits: 8,
    }, // (5,3)
    VlcEntry {
        code: 0x0037,
        bits: 8,
    }, // (6,3)
    VlcEntry {
        code: 0x005d,
        bits: 9,
    }, // (7,3)
    VlcEntry {
        code: 0x004c,
        bits: 9,
    }, // (8,3)
    VlcEntry {
        code: 0x003b,
        bits: 9,
    }, // (9,3)
    VlcEntry {
        code: 0x005d,
        bits: 10,
    }, // (10,3)
    VlcEntry {
        code: 0x0048,
        bits: 10,
    }, // (11,3)
    VlcEntry {
        code: 0x0036,
        bits: 10,
    }, // (12,3)
    VlcEntry {
        code: 0x004b,
        bits: 11,
    }, // (13,3)
    VlcEntry {
        code: 0x0032,
        bits: 11,
    }, // (14,3)
    VlcEntry {
        code: 0x001d,
        bits: 11,
    }, // (15,3)
    VlcEntry {
        code: 0x0034,
        bits: 7,
    }, // (0,4)
    VlcEntry {
        code: 0x0016,
        bits: 6,
    }, // (1,4)
    VlcEntry {
        code: 0x002a,
        bits: 7,
    }, // (2,4)
    VlcEntry {
        code: 0x0028,
        bits: 7,
    }, // (3,4)
    VlcEntry {
        code: 0x0043,
        bits: 8,
    }, // (4,4)
    VlcEntry {
        code: 0x0039,
        bits: 8,
    }, // (5,4)
    VlcEntry {
        code: 0x005f,
        bits: 9,
    }, // (6,4)
    VlcEntry {
        code: 0x004f,
        bits: 9,
    }, // (7,4)
    VlcEntry {
        code: 0x0048,
        bits: 9,
    }, // (8,4)
    VlcEntry {
        code: 0x0039,
        bits: 9,
    }, // (9,4)
    VlcEntry {
        code: 0x0059,
        bits: 10,
    }, // (10,4)
    VlcEntry {
        code: 0x0045,
        bits: 10,
    }, // (11,4)
    VlcEntry {
        code: 0x0031,
        bits: 10,
    }, // (12,4)
    VlcEntry {
        code: 0x0042,
        bits: 11,
    }, // (13,4)
    VlcEntry {
        code: 0x002e,
        bits: 11,
    }, // (14,4)
    VlcEntry {
        code: 0x001b,
        bits: 11,
    }, // (15,4)
    VlcEntry {
        code: 0x004d,
        bits: 8,
    }, // (0,5)
    VlcEntry {
        code: 0x0025,
        bits: 7,
    }, // (1,5)
    VlcEntry {
        code: 0x0023,
        bits: 7,
    }, // (2,5)
    VlcEntry {
        code: 0x0042,
        bits: 8,
    }, // (3,5)
    VlcEntry {
        code: 0x003a,
        bits: 8,
    }, // (4,5)
    VlcEntry {
        code: 0x0034,
        bits: 8,
    }, // (5,5)
    VlcEntry {
        code: 0x005b,
        bits: 9,
    }, // (6,5)
    VlcEntry {
        code: 0x004a,
        bits: 9,
    }, // (7,5)
    VlcEntry {
        code: 0x003e,
        bits: 9,
    }, // (8,5)
    VlcEntry {
        code: 0x0030,
        bits: 9,
    }, // (9,5)
    VlcEntry {
        code: 0x004f,
        bits: 10,
    }, // (10,5)
    VlcEntry {
        code: 0x003f,
        bits: 10,
    }, // (11,5)
    VlcEntry {
        code: 0x005a,
        bits: 11,
    }, // (12,5)
    VlcEntry {
        code: 0x003e,
        bits: 11,
    }, // (13,5)
    VlcEntry {
        code: 0x0028,
        bits: 11,
    }, // (14,5)
    VlcEntry {
        code: 0x0026,
        bits: 12,
    }, // (15,5)
    VlcEntry {
        code: 0x007d,
        bits: 9,
    }, // (0,6)
    VlcEntry {
        code: 0x0020,
        bits: 7,
    }, // (1,6)
    VlcEntry {
        code: 0x003c,
        bits: 8,
    }, // (2,6)
    VlcEntry {
        code: 0x0038,
        bits: 8,
    }, // (3,6)
    VlcEntry {
        code: 0x0032,
        bits: 8,
    }, // (4,6)
    VlcEntry {
        code: 0x005c,
        bits: 9,
    }, // (5,6)
    VlcEntry {
        code: 0x004e,
        bits: 9,
    }, // (6,6)
    VlcEntry {
        code: 0x0041,
        bits: 9,
    }, // (7,6)
    VlcEntry {
        code: 0x0037,
        bits: 9,
    }, // (8,6)
    VlcEntry {
        code: 0x0057,
        bits: 10,
    }, // (9,6)
    VlcEntry {
        code: 0x0047,
        bits: 10,
    }, // (10,6)
    VlcEntry {
        code: 0x0033,
        bits: 10,
    }, // (11,6)
    VlcEntry {
        code: 0x0049,
        bits: 11,
    }, // (12,6)
    VlcEntry {
        code: 0x0033,
        bits: 11,
    }, // (13,6)
    VlcEntry {
        code: 0x0046,
        bits: 12,
    }, // (14,6)
    VlcEntry {
        code: 0x001e,
        bits: 12,
    }, // (15,6)
    VlcEntry {
        code: 0x006d,
        bits: 9,
    }, // (0,7)
    VlcEntry {
        code: 0x0035,
        bits: 8,
    }, // (1,7)
    VlcEntry {
        code: 0x0031,
        bits: 8,
    }, // (2,7)
    VlcEntry {
        code: 0x005e,
        bits: 9,
    }, // (3,7)
    VlcEntry {
        code: 0x0058,
        bits: 9,
    }, // (4,7)
    VlcEntry {
        code: 0x004b,
        bits: 9,
    }, // (5,7)
    VlcEntry {
        code: 0x0042,
        bits: 9,
    }, // (6,7)
    VlcEntry {
        code: 0x007a,
        bits: 10,
    }, // (7,7)
    VlcEntry {
        code: 0x005b,
        bits: 10,
    }, // (8,7)
    VlcEntry {
        code: 0x0049,
        bits: 10,
    }, // (9,7)
    VlcEntry {
        code: 0x0038,
        bits: 10,
    }, // (10,7)
    VlcEntry {
        code: 0x002a,
        bits: 10,
    }, // (11,7)
    VlcEntry {
        code: 0x0040,
        bits: 11,
    }, // (12,7)
    VlcEntry {
        code: 0x002c,
        bits: 11,
    }, // (13,7)
    VlcEntry {
        code: 0x0015,
        bits: 11,
    }, // (14,7)
    VlcEntry {
        code: 0x0019,
        bits: 12,
    }, // (15,7)
    VlcEntry {
        code: 0x005a,
        bits: 9,
    }, // (0,8)
    VlcEntry {
        code: 0x002b,
        bits: 8,
    }, // (1,8)
    VlcEntry {
        code: 0x0029,
        bits: 8,
    }, // (2,8)
    VlcEntry {
        code: 0x004d,
        bits: 9,
    }, // (3,8)
    VlcEntry {
        code: 0x0049,
        bits: 9,
    }, // (4,8)
    VlcEntry {
        code: 0x003f,
        bits: 9,
    }, // (5,8)
    VlcEntry {
        code: 0x0038,
        bits: 9,
    }, // (6,8)
    VlcEntry {
        code: 0x005c,
        bits: 10,
    }, // (7,8)
    VlcEntry {
        code: 0x004d,
        bits: 10,
    }, // (8,8)
    VlcEntry {
        code: 0x0042,
        bits: 10,
    }, // (9,8)
    VlcEntry {
        code: 0x002f,
        bits: 10,
    }, // (10,8)
    VlcEntry {
        code: 0x0043,
        bits: 11,
    }, // (11,8)
    VlcEntry {
        code: 0x0030,
        bits: 11,
    }, // (12,8)
    VlcEntry {
        code: 0x0035,
        bits: 12,
    }, // (13,8)
    VlcEntry {
        code: 0x0024,
        bits: 12,
    }, // (14,8)
    VlcEntry {
        code: 0x0014,
        bits: 12,
    }, // (15,8)
    VlcEntry {
        code: 0x0047,
        bits: 9,
    }, // (0,9)
    VlcEntry {
        code: 0x0022,
        bits: 8,
    }, // (1,9)
    VlcEntry {
        code: 0x0043,
        bits: 9,
    }, // (2,9)
    VlcEntry {
        code: 0x003c,
        bits: 9,
    }, // (3,9)
    VlcEntry {
        code: 0x003a,
        bits: 9,
    }, // (4,9)
    VlcEntry {
        code: 0x0031,
        bits: 9,
    }, // (5,9)
    VlcEntry {
        code: 0x0058,
        bits: 10,
    }, // (6,9)
    VlcEntry {
        code: 0x004c,
        bits: 10,
    }, // (7,9)
    VlcEntry {
        code: 0x0043,
        bits: 10,
    }, // (8,9)
    VlcEntry {
        code: 0x006a,
        bits: 11,
    }, // (9,9)
    VlcEntry {
        code: 0x0047,
        bits: 11,
    }, // (10,9)
    VlcEntry {
        code: 0x0036,
        bits: 11,
    }, // (11,9)
    VlcEntry {
        code: 0x0026,
        bits: 11,
    }, // (12,9)
    VlcEntry {
        code: 0x0027,
        bits: 12,
    }, // (13,9)
    VlcEntry {
        code: 0x0017,
        bits: 12,
    }, // (14,9)
    VlcEntry {
        code: 0x000f,
        bits: 12,
    }, // (15,9)
    VlcEntry {
        code: 0x006d,
        bits: 10,
    }, // (0,10)
    VlcEntry {
        code: 0x0035,
        bits: 9,
    }, // (1,10)
    VlcEntry {
        code: 0x0033,
        bits: 9,
    }, // (2,10)
    VlcEntry {
        code: 0x002f,
        bits: 9,
    }, // (3,10)
    VlcEntry {
        code: 0x005a,
        bits: 10,
    }, // (4,10)
    VlcEntry {
        code: 0x0052,
        bits: 10,
    }, // (5,10)
    VlcEntry {
        code: 0x003a,
        bits: 10,
    }, // (6,10)
    VlcEntry {
        code: 0x0039,
        bits: 10,
    }, // (7,10)
    VlcEntry {
        code: 0x0030,
        bits: 10,
    }, // (8,10)
    VlcEntry {
        code: 0x0048,
        bits: 11,
    }, // (9,10)
    VlcEntry {
        code: 0x0039,
        bits: 11,
    }, // (10,10)
    VlcEntry {
        code: 0x0029,
        bits: 11,
    }, // (11,10)
    VlcEntry {
        code: 0x0017,
        bits: 11,
    }, // (12,10)
    VlcEntry {
        code: 0x001b,
        bits: 12,
    }, // (13,10)
    VlcEntry {
        code: 0x003e,
        bits: 13,
    }, // (14,10)
    VlcEntry {
        code: 0x0009,
        bits: 12,
    }, // (15,10)
    VlcEntry {
        code: 0x0056,
        bits: 10,
    }, // (0,11)
    VlcEntry {
        code: 0x002a,
        bits: 9,
    }, // (1,11)
    VlcEntry {
        code: 0x0028,
        bits: 9,
    }, // (2,11)
    VlcEntry {
        code: 0x0025,
        bits: 9,
    }, // (3,11)
    VlcEntry {
        code: 0x0046,
        bits: 10,
    }, // (4,11)
    VlcEntry {
        code: 0x0040,
        bits: 10,
    }, // (5,11)
    VlcEntry {
        code: 0x0034,
        bits: 10,
    }, // (6,11)
    VlcEntry {
        code: 0x002b,
        bits: 10,
    }, // (7,11)
    VlcEntry {
        code: 0x0046,
        bits: 11,
    }, // (8,11)
    VlcEntry {
        code: 0x0037,
        bits: 11,
    }, // (9,11)
    VlcEntry {
        code: 0x002a,
        bits: 11,
    }, // (10,11)
    VlcEntry {
        code: 0x0019,
        bits: 11,
    }, // (11,11)
    VlcEntry {
        code: 0x001d,
        bits: 12,
    }, // (12,11)
    VlcEntry {
        code: 0x0012,
        bits: 12,
    }, // (13,11)
    VlcEntry {
        code: 0x000b,
        bits: 12,
    }, // (14,11)
    VlcEntry {
        code: 0x000b,
        bits: 13,
    }, // (15,11)
    VlcEntry {
        code: 0x0076,
        bits: 11,
    }, // (0,12)
    VlcEntry {
        code: 0x0044,
        bits: 10,
    }, // (1,12)
    VlcEntry {
        code: 0x001e,
        bits: 9,
    }, // (2,12)
    VlcEntry {
        code: 0x0037,
        bits: 10,
    }, // (3,12)
    VlcEntry {
        code: 0x0032,
        bits: 10,
    }, // (4,12)
    VlcEntry {
        code: 0x002e,
        bits: 10,
    }, // (5,12)
    VlcEntry {
        code: 0x004a,
        bits: 11,
    }, // (6,12)
    VlcEntry {
        code: 0x0041,
        bits: 11,
    }, // (7,12)
    VlcEntry {
        code: 0x0031,
        bits: 11,
    }, // (8,12)
    VlcEntry {
        code: 0x0027,
        bits: 11,
    }, // (9,12)
    VlcEntry {
        code: 0x0018,
        bits: 11,
    }, // (10,12)
    VlcEntry {
        code: 0x0010,
        bits: 11,
    }, // (11,12)
    VlcEntry {
        code: 0x0016,
        bits: 12,
    }, // (12,12)
    VlcEntry {
        code: 0x000d,
        bits: 12,
    }, // (13,12)
    VlcEntry {
        code: 0x000e,
        bits: 13,
    }, // (14,12)
    VlcEntry {
        code: 0x0007,
        bits: 13,
    }, // (15,12)
    VlcEntry {
        code: 0x005b,
        bits: 11,
    }, // (0,13)
    VlcEntry {
        code: 0x002c,
        bits: 10,
    }, // (1,13)
    VlcEntry {
        code: 0x0027,
        bits: 10,
    }, // (2,13)
    VlcEntry {
        code: 0x0026,
        bits: 10,
    }, // (3,13)
    VlcEntry {
        code: 0x0022,
        bits: 10,
    }, // (4,13)
    VlcEntry {
        code: 0x003f,
        bits: 11,
    }, // (5,13)
    VlcEntry {
        code: 0x0034,
        bits: 11,
    }, // (6,13)
    VlcEntry {
        code: 0x002d,
        bits: 11,
    }, // (7,13)
    VlcEntry {
        code: 0x001f,
        bits: 11,
    }, // (8,13)
    VlcEntry {
        code: 0x0034,
        bits: 12,
    }, // (9,13)
    VlcEntry {
        code: 0x001c,
        bits: 12,
    }, // (10,13)
    VlcEntry {
        code: 0x0013,
        bits: 12,
    }, // (11,13)
    VlcEntry {
        code: 0x000e,
        bits: 12,
    }, // (12,13)
    VlcEntry {
        code: 0x0008,
        bits: 12,
    }, // (13,13)
    VlcEntry {
        code: 0x0009,
        bits: 13,
    }, // (14,13)
    VlcEntry {
        code: 0x0003,
        bits: 13,
    }, // (15,13)
    VlcEntry {
        code: 0x007b,
        bits: 12,
    }, // (0,14)
    VlcEntry {
        code: 0x003c,
        bits: 11,
    }, // (1,14)
    VlcEntry {
        code: 0x003a,
        bits: 11,
    }, // (2,14)
    VlcEntry {
        code: 0x0035,
        bits: 11,
    }, // (3,14)
    VlcEntry {
        code: 0x002f,
        bits: 11,
    }, // (4,14)
    VlcEntry {
        code: 0x002b,
        bits: 11,
    }, // (5,14)
    VlcEntry {
        code: 0x0020,
        bits: 11,
    }, // (6,14)
    VlcEntry {
        code: 0x0016,
        bits: 11,
    }, // (7,14)
    VlcEntry {
        code: 0x0025,
        bits: 12,
    }, // (8,14)
    VlcEntry {
        code: 0x0018,
        bits: 12,
    }, // (9,14)
    VlcEntry {
        code: 0x0011,
        bits: 12,
    }, // (10,14)
    VlcEntry {
        code: 0x000c,
        bits: 12,
    }, // (11,14)
    VlcEntry {
        code: 0x000f,
        bits: 13,
    }, // (12,14)
    VlcEntry {
        code: 0x000a,
        bits: 13,
    }, // (13,14)
    VlcEntry {
        code: 0x0002,
        bits: 12,
    }, // (14,14)
    VlcEntry {
        code: 0x0001,
        bits: 13,
    }, // (15,14)
    VlcEntry {
        code: 0x0047,
        bits: 12,
    }, // (0,15)
    VlcEntry {
        code: 0x0025,
        bits: 11,
    }, // (1,15)
    VlcEntry {
        code: 0x0022,
        bits: 11,
    }, // (2,15)
    VlcEntry {
        code: 0x001e,
        bits: 11,
    }, // (3,15)
    VlcEntry {
        code: 0x001c,
        bits: 11,
    }, // (4,15)
    VlcEntry {
        code: 0x0014,
        bits: 11,
    }, // (5,15)
    VlcEntry {
        code: 0x0011,
        bits: 11,
    }, // (6,15)
    VlcEntry {
        code: 0x001a,
        bits: 12,
    }, // (7,15)
    VlcEntry {
        code: 0x0015,
        bits: 12,
    }, // (8,15)
    VlcEntry {
        code: 0x0010,
        bits: 12,
    }, // (9,15)
    VlcEntry {
        code: 0x000a,
        bits: 12,
    }, // (10,15)
    VlcEntry {
        code: 0x0006,
        bits: 12,
    }, // (11,15)
    VlcEntry {
        code: 0x0008,
        bits: 13,
    }, // (12,15)
    VlcEntry {
        code: 0x0006,
        bits: 13,
    }, // (13,15)
    VlcEntry {
        code: 0x0002,
        bits: 13,
    }, // (14,15)
    VlcEntry {
        code: 0x0000,
        bits: 13,
    }, // (15,15)
];

static VLC_TABLE_16: [VlcEntry; 256] = [
    VlcEntry {
        code: 0x0001,
        bits: 1,
    }, // (0,0)
    VlcEntry {
        code: 0x0005,
        bits: 4,
    }, // (1,0)
    VlcEntry {
        code: 0x000e,
        bits: 6,
    }, // (2,0)
    VlcEntry {
        code: 0x002c,
        bits: 8,
    }, // (3,0)
    VlcEntry {
        code: 0x004a,
        bits: 9,
    }, // (4,0)
    VlcEntry {
        code: 0x003f,
        bits: 9,
    }, // (5,0)
    VlcEntry {
        code: 0x006e,
        bits: 10,
    }, // (6,0)
    VlcEntry {
        code: 0x005d,
        bits: 10,
    }, // (7,0)
    VlcEntry {
        code: 0x00ac,
        bits: 11,
    }, // (8,0)
    VlcEntry {
        code: 0x0095,
        bits: 11,
    }, // (9,0)
    VlcEntry {
        code: 0x008a,
        bits: 11,
    }, // (10,0)
    VlcEntry {
        code: 0x00f2,
        bits: 12,
    }, // (11,0)
    VlcEntry {
        code: 0x00e1,
        bits: 12,
    }, // (12,0)
    VlcEntry {
        code: 0x00c3,
        bits: 12,
    }, // (13,0)
    VlcEntry {
        code: 0x0178,
        bits: 13,
    }, // (14,0)
    VlcEntry {
        code: 0x0011,
        bits: 9,
    }, // (15,0)
    VlcEntry {
        code: 0x0003,
        bits: 3,
    }, // (0,1)
    VlcEntry {
        code: 0x0004,
        bits: 4,
    }, // (1,1)
    VlcEntry {
        code: 0x000c,
        bits: 6,
    }, // (2,1)
    VlcEntry {
        code: 0x0014,
        bits: 7,
    }, // (3,1)
    VlcEntry {
        code: 0x0023,
        bits: 8,
    }, // (4,1)
    VlcEntry {
        code: 0x003e,
        bits: 9,
    }, // (5,1)
    VlcEntry {
        code: 0x0035,
        bits: 9,
    }, // (6,1)
    VlcEntry {
        code: 0x002f,
        bits: 9,
    }, // (7,1)
    VlcEntry {
        code: 0x0053,
        bits: 10,
    }, // (8,1)
    VlcEntry {
        code: 0x004b,
        bits: 10,
    }, // (9,1)
    VlcEntry {
        code: 0x0044,
        bits: 10,
    }, // (10,1)
    VlcEntry {
        code: 0x0077,
        bits: 11,
    }, // (11,1)
    VlcEntry {
        code: 0x00c9,
        bits: 12,
    }, // (12,1)
    VlcEntry {
        code: 0x006b,
        bits: 11,
    }, // (13,1)
    VlcEntry {
        code: 0x00cf,
        bits: 12,
    }, // (14,1)
    VlcEntry {
        code: 0x0009,
        bits: 8,
    }, // (15,1)
    VlcEntry {
        code: 0x000f,
        bits: 6,
    }, // (0,2)
    VlcEntry {
        code: 0x000d,
        bits: 6,
    }, // (1,2)
    VlcEntry {
        code: 0x0017,
        bits: 7,
    }, // (2,2)
    VlcEntry {
        code: 0x0026,
        bits: 8,
    }, // (3,2)
    VlcEntry {
        code: 0x0043,
        bits: 9,
    }, // (4,2)
    VlcEntry {
        code: 0x003a,
        bits: 9,
    }, // (5,2)
    VlcEntry {
        code: 0x0067,
        bits: 10,
    }, // (6,2)
    VlcEntry {
        code: 0x005a,
        bits: 10,
    }, // (7,2)
    VlcEntry {
        code: 0x00a1,
        bits: 11,
    }, // (8,2)
    VlcEntry {
        code: 0x0048,
        bits: 10,
    }, // (9,2)
    VlcEntry {
        code: 0x007f,
        bits: 11,
    }, // (10,2)
    VlcEntry {
        code: 0x0075,
        bits: 11,
    }, // (11,2)
    VlcEntry {
        code: 0x006e,
        bits: 11,
    }, // (12,2)
    VlcEntry {
        code: 0x00d1,
        bits: 12,
    }, // (13,2)
    VlcEntry {
        code: 0x00ce,
        bits: 12,
    }, // (14,2)
    VlcEntry {
        code: 0x0010,
        bits: 9,
    }, // (15,2)
    VlcEntry {
        code: 0x002d,
        bits: 8,
    }, // (0,3)
    VlcEntry {
        code: 0x0015,
        bits: 7,
    }, // (1,3)
    VlcEntry {
        code: 0x0027,
        bits: 8,
    }, // (2,3)
    VlcEntry {
        code: 0x0045,
        bits: 9,
    }, // (3,3)
    VlcEntry {
        code: 0x0040,
        bits: 9,
    }, // (4,3)
    VlcEntry {
        code: 0x0072,
        bits: 10,
    }, // (5,3)
    VlcEntry {
        code: 0x0063,
        bits: 10,
    }, // (6,3)
    VlcEntry {
        code: 0x0057,
        bits: 10,
    }, // (7,3)
    VlcEntry {
        code: 0x009e,
        bits: 11,
    }, // (8,3)
    VlcEntry {
        code: 0x008c,
        bits: 11,
    }, // (9,3)
    VlcEntry {
        code: 0x00fc,
        bits: 12,
    }, // (10,3)
    VlcEntry {
        code: 0x00d4,
        bits: 12,
    }, // (11,3)
    VlcEntry {
        code: 0x00c7,
        bits: 12,
    }, // (12,3)
    VlcEntry {
        code: 0x0183,
        bits: 13,
    }, // (13,3)
    VlcEntry {
        code: 0x016d,
        bits: 13,
    }, // (14,3)
    VlcEntry {
        code: 0x001a,
        bits: 10,
    }, // (15,3)
    VlcEntry {
        code: 0x004b,
        bits: 9,
    }, // (0,4)
    VlcEntry {
        code: 0x0024,
        bits: 8,
    }, // (1,4)
    VlcEntry {
        code: 0x0044,
        bits: 9,
    }, // (2,4)
    VlcEntry {
        code: 0x0041,
        bits: 9,
    }, // (3,4)
    VlcEntry {
        code: 0x0073,
        bits: 10,
    }, // (4,4)
    VlcEntry {
        code: 0x0065,
        bits: 10,
    }, // (5,4)
    VlcEntry {
        code: 0x00b3,
        bits: 11,
    }, // (6,4)
    VlcEntry {
        code: 0x00a4,
        bits: 11,
    }, // (7,4)
    VlcEntry {
        code: 0x009b,
        bits: 11,
    }, // (8,4)
    VlcEntry {
        code: 0x0108,
        bits: 12,
    }, // (9,4)
    VlcEntry {
        code: 0x00f6,
        bits: 12,
    }, // (10,4)
    VlcEntry {
        code: 0x00e2,
        bits: 12,
    }, // (11,4)
    VlcEntry {
        code: 0x018b,
        bits: 13,
    }, // (12,4)
    VlcEntry {
        code: 0x017e,
        bits: 13,
    }, // (13,4)
    VlcEntry {
        code: 0x016a,
        bits: 13,
    }, // (14,4)
    VlcEntry {
        code: 0x0009,
        bits: 9,
    }, // (15,4)
    VlcEntry {
        code: 0x0042,
        bits: 9,
    }, // (0,5)
    VlcEntry {
        code: 0x001e,
        bits: 8,
    }, // (1,5)
    VlcEntry {
        code: 0x003b,
        bits: 9,
    }, // (2,5)
    VlcEntry {
        code: 0x0038,
        bits: 9,
    }, // (3,5)
    VlcEntry {
        code: 0x0066,
        bits: 10,
    }, // (4,5)
    VlcEntry {
        code: 0x00b9,
        bits: 11,
    }, // (5,5)
    VlcEntry {
        code: 0x00ad,
        bits: 11,
    }, // (6,5)
    VlcEntry {
        code: 0x0109,
        bits: 12,
    }, // (7,5)
    VlcEntry {
        code: 0x008e,
        bits: 11,
    }, // (8,5)
    VlcEntry {
        code: 0x00fd,
        bits: 12,
    }, // (9,5)
    VlcEntry {
        code: 0x00e8,
        bits: 12,
    }, // (10,5)
    VlcEntry {
        code: 0x0190,
        bits: 13,
    }, // (11,5)
    VlcEntry {
        code: 0x0184,
        bits: 13,
    }, // (12,5)
    VlcEntry {
        code: 0x017a,
        bits: 13,
    }, // (13,5)
    VlcEntry {
        code: 0x01bd,
        bits: 14,
    }, // (14,5)
    VlcEntry {
        code: 0x0010,
        bits: 10,
    }, // (15,5)
    VlcEntry {
        code: 0x006f,
        bits: 10,
    }, // (0,6)
    VlcEntry {
        code: 0x0036,
        bits: 9,
    }, // (1,6)
    VlcEntry {
        code: 0x0034,
        bits: 9,
    }, // (2,6)
    VlcEntry {
        code: 0x0064,
        bits: 10,
    }, // (3,6)
    VlcEntry {
        code: 0x00b8,
        bits: 11,
    }, // (4,6)
    VlcEntry {
        code: 0x00b2,
        bits: 11,
    }, // (5,6)
    VlcEntry {
        code: 0x00a0,
        bits: 11,
    }, // (6,6)
    VlcEntry {
        code: 0x0085,
        bits: 11,
    }, // (7,6)
    VlcEntry {
        code: 0x0101,
        bits: 12,
    }, // (8,6)
    VlcEntry {
        code: 0x00f4,
        bits: 12,
    }, // (9,6)
    VlcEntry {
        code: 0x00e4,
        bits: 12,
    }, // (10,6)
    VlcEntry {
        code: 0x00d9,
        bits: 12,
    }, // (11,6)
    VlcEntry {
        code: 0x0181,
        bits: 13,
    }, // (12,6)
    VlcEntry {
        code: 0x016e,
        bits: 13,
    }, // (13,6)
    VlcEntry {
        code: 0x02cb,
        bits: 14,
    }, // (14,6)
    VlcEntry {
        code: 0x000a,
        bits: 10,
    }, // (15,6)
    VlcEntry {
        code: 0x0062,
        bits: 10,
    }, // (0,7)
    VlcEntry {
        code: 0x0030,
        bits: 9,
    }, // (1,7)
    VlcEntry {
        code: 0x005b,
        bits: 10,
    }, // (2,7)
    VlcEntry {
        code: 0x0058,
        bits: 10,
    }, // (3,7)
    VlcEntry {
        code: 0x00a5,
        bits: 11,
    }, // (4,7)
    VlcEntry {
        code: 0x009d,
        bits: 11,
    }, // (5,7)
    VlcEntry {
        code: 0x0094,
        bits: 11,
    }, // (6,7)
    VlcEntry {
        code: 0x0105,
        bits: 12,
    }, // (7,7)
    VlcEntry {
        code: 0x00f8,
        bits: 12,
    }, // (8,7)
    VlcEntry {
        code: 0x0197,
        bits: 13,
    }, // (9,7)
    VlcEntry {
        code: 0x018d,
        bits: 13,
    }, // (10,7)
    VlcEntry {
        code: 0x0174,
        bits: 13,
    }, // (11,7)
    VlcEntry {
        code: 0x017c,
        bits: 13,
    }, // (12,7)
    VlcEntry {
        code: 0x0379,
        bits: 15,
    }, // (13,7)
    VlcEntry {
        code: 0x0374,
        bits: 15,
    }, // (14,7)
    VlcEntry {
        code: 0x0008,
        bits: 10,
    }, // (15,7)
    VlcEntry {
        code: 0x0055,
        bits: 10,
    }, // (0,8)
    VlcEntry {
        code: 0x0054,
        bits: 10,
    }, // (1,8)
    VlcEntry {
        code: 0x0051,
        bits: 10,
    }, // (2,8)
    VlcEntry {
        code: 0x009f,
        bits: 11,
    }, // (3,8)
    VlcEntry {
        code: 0x009c,
        bits: 11,
    }, // (4,8)
    VlcEntry {
        code: 0x008f,
        bits: 11,
    }, // (5,8)
    VlcEntry {
        code: 0x0104,
        bits: 12,
    }, // (6,8)
    VlcEntry {
        code: 0x00f9,
        bits: 12,
    }, // (7,8)
    VlcEntry {
        code: 0x01ab,
        bits: 13,
    }, // (8,8)
    VlcEntry {
        code: 0x0191,
        bits: 13,
    }, // (9,8)
    VlcEntry {
        code: 0x0188,
        bits: 13,
    }, // (10,8)
    VlcEntry {
        code: 0x017f,
        bits: 13,
    }, // (11,8)
    VlcEntry {
        code: 0x02d7,
        bits: 14,
    }, // (12,8)
    VlcEntry {
        code: 0x02c9,
        bits: 14,
    }, // (13,8)
    VlcEntry {
        code: 0x02c4,
        bits: 14,
    }, // (14,8)
    VlcEntry {
        code: 0x0007,
        bits: 10,
    }, // (15,8)
    VlcEntry {
        code: 0x009a,
        bits: 11,
    }, // (0,9)
    VlcEntry {
        code: 0x004c,
        bits: 10,
    }, // (1,9)
    VlcEntry {
        code: 0x0049,
        bits: 10,
    }, // (2,9)
    VlcEntry {
        code: 0x008d,
        bits: 11,
    }, // (3,9)
    VlcEntry {
        code: 0x0083,
        bits: 11,
    }, // (4,9)
    VlcEntry {
        code: 0x0100,
        bits: 12,
    }, // (5,9)
    VlcEntry {
        code: 0x00f5,
        bits: 12,
    }, // (6,9)
    VlcEntry {
        code: 0x01aa,
        bits: 13,
    }, // (7,9)
    VlcEntry {
        code: 0x0196,
        bits: 13,
    }, // (8,9)
    VlcEntry {
        code: 0x018a,
        bits: 13,
    }, // (9,9)
    VlcEntry {
        code: 0x0180,
        bits: 13,
    }, // (10,9)
    VlcEntry {
        code: 0x02df,
        bits: 14,
    }, // (11,9)
    VlcEntry {
        code: 0x0167,
        bits: 13,
    }, // (12,9)
    VlcEntry {
        code: 0x02c6,
        bits: 14,
    }, // (13,9)
    VlcEntry {
        code: 0x0160,
        bits: 13,
    }, // (14,9)
    VlcEntry {
        code: 0x000b,
        bits: 11,
    }, // (15,9)
    VlcEntry {
        code: 0x008b,
        bits: 11,
    }, // (0,10)
    VlcEntry {
        code: 0x0081,
        bits: 11,
    }, // (1,10)
    VlcEntry {
        code: 0x0043,
        bits: 10,
    }, // (2,10)
    VlcEntry {
        code: 0x007d,
        bits: 11,
    }, // (3,10)
    VlcEntry {
        code: 0x00f7,
        bits: 12,
    }, // (4,10)
    VlcEntry {
        code: 0x00e9,
        bits: 12,
    }, // (5,10)
    VlcEntry {
        code: 0x00e5,
        bits: 12,
    }, // (6,10)
    VlcEntry {
        code: 0x00db,
        bits: 12,
    }, // (7,10)
    VlcEntry {
        code: 0x0189,
        bits: 13,
    }, // (8,10)
    VlcEntry {
        code: 0x02e7,
        bits: 14,
    }, // (9,10)
    VlcEntry {
        code: 0x02e1,
        bits: 14,
    }, // (10,10)
    VlcEntry {
        code: 0x02d0,
        bits: 14,
    }, // (11,10)
    VlcEntry {
        code: 0x0375,
        bits: 15,
    }, // (12,10)
    VlcEntry {
        code: 0x0372,
        bits: 15,
    }, // (13,10)
    VlcEntry {
        code: 0x01b7,
        bits: 14,
    }, // (14,10)
    VlcEntry {
        code: 0x0004,
        bits: 10,
    }, // (15,10)
    VlcEntry {
        code: 0x00f3,
        bits: 12,
    }, // (0,11)
    VlcEntry {
        code: 0x0078,
        bits: 11,
    }, // (1,11)
    VlcEntry {
        code: 0x0076,
        bits: 11,
    }, // (2,11)
    VlcEntry {
        code: 0x0073,
        bits: 11,
    }, // (3,11)
    VlcEntry {
        code: 0x00e3,
        bits: 12,
    }, // (4,11)
    VlcEntry {
        code: 0x00df,
        bits: 12,
    }, // (5,11)
    VlcEntry {
        code: 0x018c,
        bits: 13,
    }, // (6,11)
    VlcEntry {
        code: 0x02ea,
        bits: 14,
    }, // (7,11)
    VlcEntry {
        code: 0x02e6,
        bits: 14,
    }, // (8,11)
    VlcEntry {
        code: 0x02e0,
        bits: 14,
    }, // (9,11)
    VlcEntry {
        code: 0x02d1,
        bits: 14,
    }, // (10,11)
    VlcEntry {
        code: 0x02c8,
        bits: 14,
    }, // (11,11)
    VlcEntry {
        code: 0x02c2,
        bits: 14,
    }, // (12,11)
    VlcEntry {
        code: 0x00df,
        bits: 13,
    }, // (13,11)
    VlcEntry {
        code: 0x01b4,
        bits: 14,
    }, // (14,11)
    VlcEntry {
        code: 0x0006,
        bits: 11,
    }, // (15,11)
    VlcEntry {
        code: 0x00ca,
        bits: 12,
    }, // (0,12)
    VlcEntry {
        code: 0x00e0,
        bits: 12,
    }, // (1,12)
    VlcEntry {
        code: 0x00de,
        bits: 12,
    }, // (2,12)
    VlcEntry {
        code: 0x00da,
        bits: 12,
    }, // (3,12)
    VlcEntry {
        code: 0x00d8,
        bits: 12,
    }, // (4,12)
    VlcEntry {
        code: 0x0185,
        bits: 13,
    }, // (5,12)
    VlcEntry {
        code: 0x0182,
        bits: 13,
    }, // (6,12)
    VlcEntry {
        code: 0x017d,
        bits: 13,
    }, // (7,12)
    VlcEntry {
        code: 0x016c,
        bits: 13,
    }, // (8,12)
    VlcEntry {
        code: 0x0378,
        bits: 15,
    }, // (9,12)
    VlcEntry {
        code: 0x01bb,
        bits: 14,
    }, // (10,12)
    VlcEntry {
        code: 0x02c3,
        bits: 14,
    }, // (11,12)
    VlcEntry {
        code: 0x01b8,
        bits: 14,
    }, // (12,12)
    VlcEntry {
        code: 0x01b5,
        bits: 14,
    }, // (13,12)
    VlcEntry {
        code: 0x06c0,
        bits: 16,
    }, // (14,12)
    VlcEntry {
        code: 0x0004,
        bits: 11,
    }, // (15,12)
    VlcEntry {
        code: 0x02eb,
        bits: 14,
    }, // (0,13)
    VlcEntry {
        code: 0x00d3,
        bits: 12,
    }, // (1,13)
    VlcEntry {
        code: 0x00d2,
        bits: 12,
    }, // (2,13)
    VlcEntry {
        code: 0x00d0,
        bits: 12,
    }, // (3,13)
    VlcEntry {
        code: 0x0172,
        bits: 13,
    }, // (4,13)
    VlcEntry {
        code: 0x017b,
        bits: 13,
    }, // (5,13)
    VlcEntry {
        code: 0x02de,
        bits: 14,
    }, // (6,13)
    VlcEntry {
        code: 0x02d3,
        bits: 14,
    }, // (7,13)
    VlcEntry {
        code: 0x02ca,
        bits: 14,
    }, // (8,13)
    VlcEntry {
        code: 0x06c7,
        bits: 16,
    }, // (9,13)
    VlcEntry {
        code: 0x0373,
        bits: 15,
    }, // (10,13)
    VlcEntry {
        code: 0x036d,
        bits: 15,
    }, // (11,13)
    VlcEntry {
        code: 0x036c,
        bits: 15,
    }, // (12,13)
    VlcEntry {
        code: 0x0d83,
        bits: 17,
    }, // (13,13)
    VlcEntry {
        code: 0x0361,
        bits: 15,
    }, // (14,13)
    VlcEntry {
        code: 0x0002,
        bits: 11,
    }, // (15,13)
    VlcEntry {
        code: 0x0179,
        bits: 13,
    }, // (0,14)
    VlcEntry {
        code: 0x0171,
        bits: 13,
    }, // (1,14)
    VlcEntry {
        code: 0x0066,
        bits: 11,
    }, // (2,14)
    VlcEntry {
        code: 0x00bb,
        bits: 12,
    }, // (3,14)
    VlcEntry {
        code: 0x02d6,
        bits: 14,
    }, // (4,14)
    VlcEntry {
        code: 0x02d2,
        bits: 14,
    }, // (5,14)
    VlcEntry {
        code: 0x0166,
        bits: 13,
    }, // (6,14)
    VlcEntry {
        code: 0x02c7,
        bits: 14,
    }, // (7,14)
    VlcEntry {
        code: 0x02c5,
        bits: 14,
    }, // (8,14)
    VlcEntry {
        code: 0x0362,
        bits: 15,
    }, // (9,14)
    VlcEntry {
        code: 0x06c6,
        bits: 16,
    }, // (10,14)
    VlcEntry {
        code: 0x0367,
        bits: 15,
    }, // (11,14)
    VlcEntry {
        code: 0x0d82,
        bits: 17,
    }, // (12,14)
    VlcEntry {
        code: 0x0366,
        bits: 15,
    }, // (13,14)
    VlcEntry {
        code: 0x01b2,
        bits: 14,
    }, // (14,14)
    VlcEntry {
        code: 0x0000,
        bits: 11,
    }, // (15,14)
    VlcEntry {
        code: 0x000c,
        bits: 9,
    }, // (0,15)
    VlcEntry {
        code: 0x000a,
        bits: 8,
    }, // (1,15)
    VlcEntry {
        code: 0x0007,
        bits: 8,
    }, // (2,15)
    VlcEntry {
        code: 0x000b,
        bits: 9,
    }, // (3,15)
    VlcEntry {
        code: 0x000a,
        bits: 9,
    }, // (4,15)
    VlcEntry {
        code: 0x0011,
        bits: 10,
    }, // (5,15)
    VlcEntry {
        code: 0x000b,
        bits: 10,
    }, // (6,15)
    VlcEntry {
        code: 0x0009,
        bits: 10,
    }, // (7,15)
    VlcEntry {
        code: 0x000d,
        bits: 11,
    }, // (8,15)
    VlcEntry {
        code: 0x000c,
        bits: 11,
    }, // (9,15)
    VlcEntry {
        code: 0x000a,
        bits: 11,
    }, // (10,15)
    VlcEntry {
        code: 0x0007,
        bits: 11,
    }, // (11,15)
    VlcEntry {
        code: 0x0005,
        bits: 11,
    }, // (12,15)
    VlcEntry {
        code: 0x0003,
        bits: 11,
    }, // (13,15)
    VlcEntry {
        code: 0x0001,
        bits: 11,
    }, // (14,15)
    VlcEntry {
        code: 0x0003,
        bits: 8,
    }, // (15,15)
];

static VLC_TABLE_24: [VlcEntry; 256] = [
    VlcEntry {
        code: 0x000f,
        bits: 4,
    }, // (0,0)
    VlcEntry {
        code: 0x000d,
        bits: 4,
    }, // (1,0)
    VlcEntry {
        code: 0x002e,
        bits: 6,
    }, // (2,0)
    VlcEntry {
        code: 0x0050,
        bits: 7,
    }, // (3,0)
    VlcEntry {
        code: 0x0092,
        bits: 8,
    }, // (4,0)
    VlcEntry {
        code: 0x0106,
        bits: 9,
    }, // (5,0)
    VlcEntry {
        code: 0x00f8,
        bits: 9,
    }, // (6,0)
    VlcEntry {
        code: 0x01b2,
        bits: 10,
    }, // (7,0)
    VlcEntry {
        code: 0x01aa,
        bits: 10,
    }, // (8,0)
    VlcEntry {
        code: 0x029d,
        bits: 11,
    }, // (9,0)
    VlcEntry {
        code: 0x028d,
        bits: 11,
    }, // (10,0)
    VlcEntry {
        code: 0x0289,
        bits: 11,
    }, // (11,0)
    VlcEntry {
        code: 0x026d,
        bits: 11,
    }, // (12,0)
    VlcEntry {
        code: 0x0205,
        bits: 11,
    }, // (13,0)
    VlcEntry {
        code: 0x0408,
        bits: 12,
    }, // (14,0)
    VlcEntry {
        code: 0x0058,
        bits: 9,
    }, // (15,0)
    VlcEntry {
        code: 0x000e,
        bits: 4,
    }, // (0,1)
    VlcEntry {
        code: 0x000c,
        bits: 4,
    }, // (1,1)
    VlcEntry {
        code: 0x0015,
        bits: 5,
    }, // (2,1)
    VlcEntry {
        code: 0x0026,
        bits: 6,
    }, // (3,1)
    VlcEntry {
        code: 0x0047,
        bits: 7,
    }, // (4,1)
    VlcEntry {
        code: 0x0082,
        bits: 8,
    }, // (5,1)
    VlcEntry {
        code: 0x007a,
        bits: 8,
    }, // (6,1)
    VlcEntry {
        code: 0x00d8,
        bits: 9,
    }, // (7,1)
    VlcEntry {
        code: 0x00d1,
        bits: 9,
    }, // (8,1)
    VlcEntry {
        code: 0x00c6,
        bits: 9,
    }, // (9,1)
    VlcEntry {
        code: 0x0147,
        bits: 10,
    }, // (10,1)
    VlcEntry {
        code: 0x0159,
        bits: 10,
    }, // (11,1)
    VlcEntry {
        code: 0x013f,
        bits: 10,
    }, // (12,1)
    VlcEntry {
        code: 0x0129,
        bits: 10,
    }, // (13,1)
    VlcEntry {
        code: 0x0117,
        bits: 10,
    }, // (14,1)
    VlcEntry {
        code: 0x002a,
        bits: 8,
    }, // (15,1)
    VlcEntry {
        code: 0x002f,
        bits: 6,
    }, // (0,2)
    VlcEntry {
        code: 0x0016,
        bits: 5,
    }, // (1,2)
    VlcEntry {
        code: 0x0029,
        bits: 6,
    }, // (2,2)
    VlcEntry {
        code: 0x004a,
        bits: 7,
    }, // (3,2)
    VlcEntry {
        code: 0x0044,
        bits: 7,
    }, // (4,2)
    VlcEntry {
        code: 0x0080,
        bits: 8,
    }, // (5,2)
    VlcEntry {
        code: 0x0078,
        bits: 8,
    }, // (6,2)
    VlcEntry {
        code: 0x00dd,
        bits: 9,
    }, // (7,2)
    VlcEntry {
        code: 0x00cf,
        bits: 9,
    }, // (8,2)
    VlcEntry {
        code: 0x00c2,
        bits: 9,
    }, // (9,2)
    VlcEntry {
        code: 0x00b6,
        bits: 9,
    }, // (10,2)
    VlcEntry {
        code: 0x0154,
        bits: 10,
    }, // (11,2)
    VlcEntry {
        code: 0x013b,
        bits: 10,
    }, // (12,2)
    VlcEntry {
        code: 0x0127,
        bits: 10,
    }, // (13,2)
    VlcEntry {
        code: 0x021d,
        bits: 11,
    }, // (14,2)
    VlcEntry {
        code: 0x0012,
        bits: 7,
    }, // (15,2)
    VlcEntry {
        code: 0x0051,
        bits: 7,
    }, // (0,3)
    VlcEntry {
        code: 0x0027,
        bits: 6,
    }, // (1,3)
    VlcEntry {
        code: 0x004b,
        bits: 7,
    }, // (2,3)
    VlcEntry {
        code: 0x0046,
        bits: 7,
    }, // (3,3)
    VlcEntry {
        code: 0x0086,
        bits: 8,
    }, // (4,3)
    VlcEntry {
        code: 0x007d,
        bits: 8,
    }, // (5,3)
    VlcEntry {
        code: 0x0074,
        bits: 8,
    }, // (6,3)
    VlcEntry {
        code: 0x00dc,
        bits: 9,
    }, // (7,3)
    VlcEntry {
        code: 0x00cc,
        bits: 9,
    }, // (8,3)
    VlcEntry {
        code: 0x00be,
        bits: 9,
    }, // (9,3)
    VlcEntry {
        code: 0x00b2,
        bits: 9,
    }, // (10,3)
    VlcEntry {
        code: 0x0145,
        bits: 10,
    }, // (11,3)
    VlcEntry {
        code: 0x0137,
        bits: 10,
    }, // (12,3)
    VlcEntry {
        code: 0x0125,
        bits: 10,
    }, // (13,3)
    VlcEntry {
        code: 0x010f,
        bits: 10,
    }, // (14,3)
    VlcEntry {
        code: 0x0010,
        bits: 7,
    }, // (15,3)
    VlcEntry {
        code: 0x0093,
        bits: 8,
    }, // (0,4)
    VlcEntry {
        code: 0x0048,
        bits: 7,
    }, // (1,4)
    VlcEntry {
        code: 0x0045,
        bits: 7,
    }, // (2,4)
    VlcEntry {
        code: 0x0087,
        bits: 8,
    }, // (3,4)
    VlcEntry {
        code: 0x007f,
        bits: 8,
    }, // (4,4)
    VlcEntry {
        code: 0x0076,
        bits: 8,
    }, // (5,4)
    VlcEntry {
        code: 0x0070,
        bits: 8,
    }, // (6,4)
    VlcEntry {
        code: 0x00d2,
        bits: 9,
    }, // (7,4)
    VlcEntry {
        code: 0x00c8,
        bits: 9,
    }, // (8,4)
    VlcEntry {
        code: 0x00bc,
        bits: 9,
    }, // (9,4)
    VlcEntry {
        code: 0x0160,
        bits: 10,
    }, // (10,4)
    VlcEntry {
        code: 0x0143,
        bits: 10,
    }, // (11,4)
    VlcEntry {
        code: 0x0132,
        bits: 10,
    }, // (12,4)
    VlcEntry {
        code: 0x011d,
        bits: 10,
    }, // (13,4)
    VlcEntry {
        code: 0x021c,
        bits: 11,
    }, // (14,4)
    VlcEntry {
        code: 0x000e,
        bits: 7,
    }, // (15,4)
    VlcEntry {
        code: 0x0107,
        bits: 9,
    }, // (0,5)
    VlcEntry {
        code: 0x0042,
        bits: 7,
    }, // (1,5)
    VlcEntry {
        code: 0x0081,
        bits: 8,
    }, // (2,5)
    VlcEntry {
        code: 0x007e,
        bits: 8,
    }, // (3,5)
    VlcEntry {
        code: 0x0077,
        bits: 8,
    }, // (4,5)
    VlcEntry {
        code: 0x0072,
        bits: 8,
    }, // (5,5)
    VlcEntry {
        code: 0x00d6,
        bits: 9,
    }, // (6,5)
    VlcEntry {
        code: 0x00ca,
        bits: 9,
    }, // (7,5)
    VlcEntry {
        code: 0x00c0,
        bits: 9,
    }, // (8,5)
    VlcEntry {
        code: 0x00b4,
        bits: 9,
    }, // (9,5)
    VlcEntry {
        code: 0x0155,
        bits: 10,
    }, // (10,5)
    VlcEntry {
        code: 0x013d,
        bits: 10,
    }, // (11,5)
    VlcEntry {
        code: 0x012d,
        bits: 10,
    }, // (12,5)
    VlcEntry {
        code: 0x0119,
        bits: 10,
    }, // (13,5)
    VlcEntry {
        code: 0x0106,
        bits: 10,
    }, // (14,5)
    VlcEntry {
        code: 0x000c,
        bits: 7,
    }, // (15,5)
    VlcEntry {
        code: 0x00f9,
        bits: 9,
    }, // (0,6)
    VlcEntry {
        code: 0x007b,
        bits: 8,
    }, // (1,6)
    VlcEntry {
        code: 0x0079,
        bits: 8,
    }, // (2,6)
    VlcEntry {
        code: 0x0075,
        bits: 8,
    }, // (3,6)
    VlcEntry {
        code: 0x0071,
        bits: 8,
    }, // (4,6)
    VlcEntry {
        code: 0x00d7,
        bits: 9,
    }, // (5,6)
    VlcEntry {
        code: 0x00ce,
        bits: 9,
    }, // (6,6)
    VlcEntry {
        code: 0x00c3,
        bits: 9,
    }, // (7,6)
    VlcEntry {
        code: 0x00b9,
        bits: 9,
    }, // (8,6)
    VlcEntry {
        code: 0x015b,
        bits: 10,
    }, // (9,6)
    VlcEntry {
        code: 0x014a,
        bits: 10,
    }, // (10,6)
    VlcEntry {
        code: 0x0134,
        bits: 10,
    }, // (11,6)
    VlcEntry {
        code: 0x0123,
        bits: 10,
    }, // (12,6)
    VlcEntry {
        code: 0x0110,
        bits: 10,
    }, // (13,6)
    VlcEntry {
        code: 0x0208,
        bits: 11,
    }, // (14,6)
    VlcEntry {
        code: 0x000a,
        bits: 7,
    }, // (15,6)
    VlcEntry {
        code: 0x01b3,
        bits: 10,
    }, // (0,7)
    VlcEntry {
        code: 0x0073,
        bits: 8,
    }, // (1,7)
    VlcEntry {
        code: 0x006f,
        bits: 8,
    }, // (2,7)
    VlcEntry {
        code: 0x006d,
        bits: 8,
    }, // (3,7)
    VlcEntry {
        code: 0x00d3,
        bits: 9,
    }, // (4,7)
    VlcEntry {
        code: 0x00cb,
        bits: 9,
    }, // (5,7)
    VlcEntry {
        code: 0x00c4,
        bits: 9,
    }, // (6,7)
    VlcEntry {
        code: 0x00bb,
        bits: 9,
    }, // (7,7)
    VlcEntry {
        code: 0x0161,
        bits: 10,
    }, // (8,7)
    VlcEntry {
        code: 0x014c,
        bits: 10,
    }, // (9,7)
    VlcEntry {
        code: 0x0139,
        bits: 10,
    }, // (10,7)
    VlcEntry {
        code: 0x012a,
        bits: 10,
    }, // (11,7)
    VlcEntry {
        code: 0x011b,
        bits: 10,
    }, // (12,7)
    VlcEntry {
        code: 0x0213,
        bits: 11,
    }, // (13,7)
    VlcEntry {
        code: 0x017d,
        bits: 11,
    }, // (14,7)
    VlcEntry {
        code: 0x0011,
        bits: 8,
    }, // (15,7)
    VlcEntry {
        code: 0x01ab,
        bits: 10,
    }, // (0,8)
    VlcEntry {
        code: 0x00d4,
        bits: 9,
    }, // (1,8)
    VlcEntry {
        code: 0x00d0,
        bits: 9,
    }, // (2,8)
    VlcEntry {
        code: 0x00cd,
        bits: 9,
    }, // (3,8)
    VlcEntry {
        code: 0x00c9,
        bits: 9,
    }, // (4,8)
    VlcEntry {
        code: 0x00c1,
        bits: 9,
    }, // (5,8)
    VlcEntry {
        code: 0x00ba,
        bits: 9,
    }, // (6,8)
    VlcEntry {
        code: 0x00b1,
        bits: 9,
    }, // (7,8)
    VlcEntry {
        code: 0x00a9,
        bits: 9,
    }, // (8,8)
    VlcEntry {
        code: 0x0140,
        bits: 10,
    }, // (9,8)
    VlcEntry {
        code: 0x012f,
        bits: 10,
    }, // (10,8)
    VlcEntry {
        code: 0x011e,
        bits: 10,
    }, // (11,8)
    VlcEntry {
        code: 0x010c,
        bits: 10,
    }, // (12,8)
    VlcEntry {
        code: 0x0202,
        bits: 11,
    }, // (13,8)
    VlcEntry {
        code: 0x0179,
        bits: 11,
    }, // (14,8)
    VlcEntry {
        code: 0x0010,
        bits: 8,
    }, // (15,8)
    VlcEntry {
        code: 0x014f,
        bits: 10,
    }, // (0,9)
    VlcEntry {
        code: 0x00c7,
        bits: 9,
    }, // (1,9)
    VlcEntry {
        code: 0x00c5,
        bits: 9,
    }, // (2,9)
    VlcEntry {
        code: 0x00bf,
        bits: 9,
    }, // (3,9)
    VlcEntry {
        code: 0x00bd,
        bits: 9,
    }, // (4,9)
    VlcEntry {
        code: 0x00b5,
        bits: 9,
    }, // (5,9)
    VlcEntry {
        code: 0x00ae,
        bits: 9,
    }, // (6,9)
    VlcEntry {
        code: 0x014d,
        bits: 10,
    }, // (7,9)
    VlcEntry {
        code: 0x0141,
        bits: 10,
    }, // (8,9)
    VlcEntry {
        code: 0x0131,
        bits: 10,
    }, // (9,9)
    VlcEntry {
        code: 0x0121,
        bits: 10,
    }, // (10,9)
    VlcEntry {
        code: 0x0113,
        bits: 10,
    }, // (11,9)
    VlcEntry {
        code: 0x0209,
        bits: 11,
    }, // (12,9)
    VlcEntry {
        code: 0x017b,
        bits: 11,
    }, // (13,9)
    VlcEntry {
        code: 0x0173,
        bits: 11,
    }, // (14,9)
    VlcEntry {
        code: 0x000b,
        bits: 8,
    }, // (15,9)
    VlcEntry {
        code: 0x029c,
        bits: 11,
    }, // (0,10)
    VlcEntry {
        code: 0x00b8,
        bits: 9,
    }, // (1,10)
    VlcEntry {
        code: 0x00b7,
        bits: 9,
    }, // (2,10)
    VlcEntry {
        code: 0x00b3,
        bits: 9,
    }, // (3,10)
    VlcEntry {
        code: 0x00af,
        bits: 9,
    }, // (4,10)
    VlcEntry {
        code: 0x0158,
        bits: 10,
    }, // (5,10)
    VlcEntry {
        code: 0x014b,
        bits: 10,
    }, // (6,10)
    VlcEntry {
        code: 0x013a,
        bits: 10,
    }, // (7,10)
    VlcEntry {
        code: 0x0130,
        bits: 10,
    }, // (8,10)
    VlcEntry {
        code: 0x0122,
        bits: 10,
    }, // (9,10)
    VlcEntry {
        code: 0x0115,
        bits: 10,
    }, // (10,10)
    VlcEntry {
        code: 0x0212,
        bits: 11,
    }, // (11,10)
    VlcEntry {
        code: 0x017f,
        bits: 11,
    }, // (12,10)
    VlcEntry {
        code: 0x0175,
        bits: 11,
    }, // (13,10)
    VlcEntry {
        code: 0x016e,
        bits: 11,
    }, // (14,10)
    VlcEntry {
        code: 0x000a,
        bits: 8,
    }, // (15,10)
    VlcEntry {
        code: 0x028c,
        bits: 11,
    }, // (0,11)
    VlcEntry {
        code: 0x015a,
        bits: 10,
    }, // (1,11)
    VlcEntry {
        code: 0x00ab,
        bits: 9,
    }, // (2,11)
    VlcEntry {
        code: 0x00a8,
        bits: 9,
    }, // (3,11)
    VlcEntry {
        code: 0x00a4,
        bits: 9,
    }, // (4,11)
    VlcEntry {
        code: 0x013e,
        bits: 10,
    }, // (5,11)
    VlcEntry {
        code: 0x0135,
        bits: 10,
    }, // (6,11)
    VlcEntry {
        code: 0x012b,
        bits: 10,
    }, // (7,11)
    VlcEntry {
        code: 0x011f,
        bits: 10,
    }, // (8,11)
    VlcEntry {
        code: 0x0114,
        bits: 10,
    }, // (9,11)
    VlcEntry {
        code: 0x0107,
        bits: 10,
    }, // (10,11)
    VlcEntry {
        code: 0x0201,
        bits: 11,
    }, // (11,11)
    VlcEntry {
        code: 0x0177,
        bits: 11,
    }, // (12,11)
    VlcEntry {
        code: 0x0170,
        bits: 11,
    }, // (13,11)
    VlcEntry {
        code: 0x016a,
        bits: 11,
    }, // (14,11)
    VlcEntry {
        code: 0x0006,
        bits: 8,
    }, // (15,11)
    VlcEntry {
        code: 0x0288,
        bits: 11,
    }, // (0,12)
    VlcEntry {
        code: 0x0142,
        bits: 10,
    }, // (1,12)
    VlcEntry {
        code: 0x013c,
        bits: 10,
    }, // (2,12)
    VlcEntry {
        code: 0x0138,
        bits: 10,
    }, // (3,12)
    VlcEntry {
        code: 0x0133,
        bits: 10,
    }, // (4,12)
    VlcEntry {
        code: 0x012e,
        bits: 10,
    }, // (5,12)
    VlcEntry {
        code: 0x0124,
        bits: 10,
    }, // (6,12)
    VlcEntry {
        code: 0x011c,
        bits: 10,
    }, // (7,12)
    VlcEntry {
        code: 0x010d,
        bits: 10,
    }, // (8,12)
    VlcEntry {
        code: 0x0105,
        bits: 10,
    }, // (9,12)
    VlcEntry {
        code: 0x0200,
        bits: 11,
    }, // (10,12)
    VlcEntry {
        code: 0x0178,
        bits: 11,
    }, // (11,12)
    VlcEntry {
        code: 0x0172,
        bits: 11,
    }, // (12,12)
    VlcEntry {
        code: 0x016c,
        bits: 11,
    }, // (13,12)
    VlcEntry {
        code: 0x0167,
        bits: 11,
    }, // (14,12)
    VlcEntry {
        code: 0x0004,
        bits: 8,
    }, // (15,12)
    VlcEntry {
        code: 0x026c,
        bits: 11,
    }, // (0,13)
    VlcEntry {
        code: 0x012c,
        bits: 10,
    }, // (1,13)
    VlcEntry {
        code: 0x0128,
        bits: 10,
    }, // (2,13)
    VlcEntry {
        code: 0x0126,
        bits: 10,
    }, // (3,13)
    VlcEntry {
        code: 0x0120,
        bits: 10,
    }, // (4,13)
    VlcEntry {
        code: 0x011a,
        bits: 10,
    }, // (5,13)
    VlcEntry {
        code: 0x0111,
        bits: 10,
    }, // (6,13)
    VlcEntry {
        code: 0x010a,
        bits: 10,
    }, // (7,13)
    VlcEntry {
        code: 0x0203,
        bits: 11,
    }, // (8,13)
    VlcEntry {
        code: 0x017c,
        bits: 11,
    }, // (9,13)
    VlcEntry {
        code: 0x0176,
        bits: 11,
    }, // (10,13)
    VlcEntry {
        code: 0x0171,
        bits: 11,
    }, // (11,13)
    VlcEntry {
        code: 0x016d,
        bits: 11,
    }, // (12,13)
    VlcEntry {
        code: 0x0169,
        bits: 11,
    }, // (13,13)
    VlcEntry {
        code: 0x0165,
        bits: 11,
    }, // (14,13)
    VlcEntry {
        code: 0x0002,
        bits: 8,
    }, // (15,13)
    VlcEntry {
        code: 0x0409,
        bits: 12,
    }, // (0,14)
    VlcEntry {
        code: 0x0118,
        bits: 10,
    }, // (1,14)
    VlcEntry {
        code: 0x0116,
        bits: 10,
    }, // (2,14)
    VlcEntry {
        code: 0x0112,
        bits: 10,
    }, // (3,14)
    VlcEntry {
        code: 0x010b,
        bits: 10,
    }, // (4,14)
    VlcEntry {
        code: 0x0108,
        bits: 10,
    }, // (5,14)
    VlcEntry {
        code: 0x0103,
        bits: 10,
    }, // (6,14)
    VlcEntry {
        code: 0x017e,
        bits: 11,
    }, // (7,14)
    VlcEntry {
        code: 0x017a,
        bits: 11,
    }, // (8,14)
    VlcEntry {
        code: 0x0174,
        bits: 11,
    }, // (9,14)
    VlcEntry {
        code: 0x016f,
        bits: 11,
    }, // (10,14)
    VlcEntry {
        code: 0x016b,
        bits: 11,
    }, // (11,14)
    VlcEntry {
        code: 0x0168,
        bits: 11,
    }, // (12,14)
    VlcEntry {
        code: 0x0166,
        bits: 11,
    }, // (13,14)
    VlcEntry {
        code: 0x0164,
        bits: 11,
    }, // (14,14)
    VlcEntry {
        code: 0x0000,
        bits: 8,
    }, // (15,14)
    VlcEntry {
        code: 0x002b,
        bits: 8,
    }, // (0,15)
    VlcEntry {
        code: 0x0014,
        bits: 7,
    }, // (1,15)
    VlcEntry {
        code: 0x0013,
        bits: 7,
    }, // (2,15)
    VlcEntry {
        code: 0x0011,
        bits: 7,
    }, // (3,15)
    VlcEntry {
        code: 0x000f,
        bits: 7,
    }, // (4,15)
    VlcEntry {
        code: 0x000d,
        bits: 7,
    }, // (5,15)
    VlcEntry {
        code: 0x000b,
        bits: 7,
    }, // (6,15)
    VlcEntry {
        code: 0x0009,
        bits: 7,
    }, // (7,15)
    VlcEntry {
        code: 0x0007,
        bits: 7,
    }, // (8,15)
    VlcEntry {
        code: 0x0006,
        bits: 7,
    }, // (9,15)
    VlcEntry {
        code: 0x0004,
        bits: 7,
    }, // (10,15)
    VlcEntry {
        code: 0x0007,
        bits: 8,
    }, // (11,15)
    VlcEntry {
        code: 0x0005,
        bits: 8,
    }, // (12,15)
    VlcEntry {
        code: 0x0003,
        bits: 8,
    }, // (13,15)
    VlcEntry {
        code: 0x0001,
        bits: 8,
    }, // (14,15)
    VlcEntry {
        code: 0x0003,
        bits: 4,
    }, // (15,15)
];

// ─── Count1 quad tables (from FFmpeg mpa_quad_codes / mpa_quad_bits) ───

static QUAD_TABLE_0: [VlcEntry; 16] = [
    VlcEntry {
        code: 0x0001,
        bits: 1,
    }, // (0,0,0,0)
    VlcEntry {
        code: 0x0005,
        bits: 4,
    }, // (0,0,0,1)
    VlcEntry {
        code: 0x0004,
        bits: 4,
    }, // (0,0,1,0)
    VlcEntry {
        code: 0x0005,
        bits: 5,
    }, // (0,0,1,1)
    VlcEntry {
        code: 0x0006,
        bits: 4,
    }, // (0,1,0,0)
    VlcEntry {
        code: 0x0005,
        bits: 6,
    }, // (0,1,0,1)
    VlcEntry {
        code: 0x0004,
        bits: 5,
    }, // (0,1,1,0)
    VlcEntry {
        code: 0x0004,
        bits: 6,
    }, // (0,1,1,1)
    VlcEntry {
        code: 0x0007,
        bits: 4,
    }, // (1,0,0,0)
    VlcEntry {
        code: 0x0003,
        bits: 5,
    }, // (1,0,0,1)
    VlcEntry {
        code: 0x0006,
        bits: 5,
    }, // (1,0,1,0)
    VlcEntry {
        code: 0x0000,
        bits: 6,
    }, // (1,0,1,1)
    VlcEntry {
        code: 0x0007,
        bits: 5,
    }, // (1,1,0,0)
    VlcEntry {
        code: 0x0002,
        bits: 6,
    }, // (1,1,0,1)
    VlcEntry {
        code: 0x0003,
        bits: 6,
    }, // (1,1,1,0)
    VlcEntry {
        code: 0x0001,
        bits: 6,
    }, // (1,1,1,1)
];

static QUAD_TABLE_1: [VlcEntry; 16] = [
    VlcEntry {
        code: 0x000f,
        bits: 4,
    }, // (0,0,0,0)
    VlcEntry {
        code: 0x000e,
        bits: 4,
    }, // (0,0,0,1)
    VlcEntry {
        code: 0x000d,
        bits: 4,
    }, // (0,0,1,0)
    VlcEntry {
        code: 0x000c,
        bits: 4,
    }, // (0,0,1,1)
    VlcEntry {
        code: 0x000b,
        bits: 4,
    }, // (0,1,0,0)
    VlcEntry {
        code: 0x000a,
        bits: 4,
    }, // (0,1,0,1)
    VlcEntry {
        code: 0x0009,
        bits: 4,
    }, // (0,1,1,0)
    VlcEntry {
        code: 0x0008,
        bits: 4,
    }, // (0,1,1,1)
    VlcEntry {
        code: 0x0007,
        bits: 4,
    }, // (1,0,0,0)
    VlcEntry {
        code: 0x0006,
        bits: 4,
    }, // (1,0,0,1)
    VlcEntry {
        code: 0x0005,
        bits: 4,
    }, // (1,0,1,0)
    VlcEntry {
        code: 0x0004,
        bits: 4,
    }, // (1,0,1,1)
    VlcEntry {
        code: 0x0003,
        bits: 4,
    }, // (1,1,0,0)
    VlcEntry {
        code: 0x0002,
        bits: 4,
    }, // (1,1,0,1)
    VlcEntry {
        code: 0x0001,
        bits: 4,
    }, // (1,1,1,0)
    VlcEntry {
        code: 0x0000,
        bits: 4,
    }, // (1,1,1,1)
];

// ─── big_values table selection (from FFmpeg mpa_huff_data[32][2]) ───

/// Maps table_select id (0-31) to (vlc_table_index, linbits).
/// `None` means the table id is genuinely reserved (chapter 09 §2: only
/// **4 and 14** are unused/reserved -- ID 0 is a valid, selectable ID,
/// just a trivially-empty tree, used when a big_values sub-region has no
/// values to code at all; `huffman::encode` special-cases table_id == 0
/// as "skip, zero bits" everywhere rather than indexing this array with
/// it, but this entry must still say `Some` to match the ID's real
/// validity -- an earlier version marked it `None`, i.e. "invalid",
/// which was inert only because nothing happened to query index 0).
pub const BIG_VALUES_TABLES: [Option<(usize, u8)>; 32] = [
    Some((0, 0)),   // 0 (valid -- trivially empty, xlen=0)
    Some((1, 0)),   // 1
    Some((2, 0)),   // 2
    Some((3, 0)),   // 3
    None,           // 4 (invalid)
    Some((4, 0)),   // 5
    Some((5, 0)),   // 6
    Some((6, 0)),   // 7
    Some((7, 0)),   // 8
    Some((8, 0)),   // 9
    Some((9, 0)),   // 10
    Some((10, 0)),  // 11
    Some((11, 0)),  // 12
    Some((12, 0)),  // 13
    None,           // 14 (invalid)
    Some((13, 0)),  // 15
    Some((14, 1)),  // 16
    Some((14, 2)),  // 17
    Some((14, 3)),  // 18
    Some((14, 4)),  // 19
    Some((14, 6)),  // 20
    Some((14, 8)),  // 21
    Some((14, 10)), // 22
    Some((14, 13)), // 23
    Some((15, 4)),  // 24
    Some((15, 5)),  // 25
    Some((15, 6)),  // 26
    Some((15, 7)),  // 27
    Some((15, 8)),  // 28
    Some((15, 9)),  // 29
    Some((15, 11)), // 30
    Some((15, 13)), // 31
];

/// The 16 VLC encoding tables, indexed by vlc_table_index.
///
/// `static`, not `const`: each entry's `lookup` field references one of
/// the `static VLC_TABLE_*` arrays above, and a `const` referencing a
/// `static` (`const_refs_to_static`) only stabilized in Rust 1.83 — this
/// crate's MSRV is 1.82 (`rust-toolchain.toml`, `Cargo.toml`). `static`
/// referencing `static` has always been legal, so this keeps the real
/// MSRV honest instead of only appearing to hold. Runtime behavior is
/// identical either way — every use site indexes/borrows this table,
/// none requires it to be usable in a `const` context.
pub static VLC_TABLES: [HuffmanTable; 16] = [
    HuffmanTable {
        xlen: 0,
        lookup: &[],
    }, // 0: unused
    HuffmanTable {
        xlen: 2,
        lookup: &VLC_TABLE_1,
    }, // vlc_index for table 1
    HuffmanTable {
        xlen: 3,
        lookup: &VLC_TABLE_2,
    }, // vlc_index for table 2
    HuffmanTable {
        xlen: 3,
        lookup: &VLC_TABLE_3,
    }, // vlc_index for table 3
    HuffmanTable {
        xlen: 4,
        lookup: &VLC_TABLE_5,
    }, // vlc_index for table 5
    HuffmanTable {
        xlen: 4,
        lookup: &VLC_TABLE_6,
    }, // vlc_index for table 6
    HuffmanTable {
        xlen: 6,
        lookup: &VLC_TABLE_7,
    }, // vlc_index for table 7
    HuffmanTable {
        xlen: 6,
        lookup: &VLC_TABLE_8,
    }, // vlc_index for table 8
    HuffmanTable {
        xlen: 6,
        lookup: &VLC_TABLE_9,
    }, // vlc_index for table 9
    HuffmanTable {
        xlen: 8,
        lookup: &VLC_TABLE_10,
    }, // vlc_index for table 10
    HuffmanTable {
        xlen: 8,
        lookup: &VLC_TABLE_11,
    }, // vlc_index for table 11
    HuffmanTable {
        xlen: 8,
        lookup: &VLC_TABLE_12,
    }, // vlc_index for table 12
    HuffmanTable {
        xlen: 16,
        lookup: &VLC_TABLE_13,
    }, // vlc_index for table 13
    HuffmanTable {
        xlen: 16,
        lookup: &VLC_TABLE_15,
    }, // vlc_index for table 15
    HuffmanTable {
        xlen: 16,
        lookup: &VLC_TABLE_16,
    }, // vlc_index for table 16
    HuffmanTable {
        xlen: 16,
        lookup: &VLC_TABLE_24,
    }, // vlc_index for table 24
];

/// The 2 count1 quad tables.
///
/// `static`, not `const` — same MSRV reason as [`VLC_TABLES`] above.
pub static COUNT1_TABLES: [Count1Table; 2] = [
    Count1Table {
        entries: &QUAD_TABLE_0,
    }, // count1table_select=false (table 32)
    Count1Table {
        entries: &QUAD_TABLE_1,
    }, // count1table_select=true  (table 33)
];

// Note: an earlier version of this file also had a `LINBITS: [u8; 16]`
// constant indexed by vlc_table_index. It was dead code (never
// referenced) and semantically ill-defined -- linbits is a property of
// the *selectable ID* (0-31), not the underlying tree (0-15): IDs 16-23
// all share vlc_table_index 14 with *different* linbits (1,2,3,4,6,8,10,
// 13), so a single value per tree index can't represent it. The real,
// correctly per-ID linbits already lives in `BIG_VALUES_TABLES`'s
// `(vlc_table_index, linbits)` tuples above.

#[cfg(test)]
mod tests {
    use super::*;

    // --- Table-provenance tests (docs/mp3-encoder/13-testing-and-
    // validation.md #table-provenance) -- an earlier version of this
    // file had none at all, despite chapter 09 calling this "the largest
    // block of load-bearing numeric data in the entire project" and
    // explicitly requiring one.

    /// Deterministic checksum over every big_values/count1 entry's raw
    /// (code, bits), in table-then-entry order. Stability check only --
    /// catches an accidental future edit to the ~2000 hand-generated
    /// entries. Does not by itself prove the *original* values are
    /// right; the prefix-free invariant test below and this file's
    /// module-doc cross-check citations carry that weight instead (per
    /// chapter 13: an invariant test is included *in addition to*, not
    /// instead of, a checksum test wherever a real invariant exists).
    fn checksum_all_tables() -> u64 {
        let mut sum: u64 = 0;
        for table in &VLC_TABLES {
            for entry in table.lookup {
                sum = sum.wrapping_mul(31).wrapping_add(u64::from(entry.code));
                sum = sum.wrapping_mul(31).wrapping_add(u64::from(entry.bits));
            }
        }
        for table in &COUNT1_TABLES {
            for entry in table.entries {
                sum = sum.wrapping_mul(31).wrapping_add(u64::from(entry.code));
                sum = sum.wrapping_mul(31).wrapping_add(u64::from(entry.bits));
            }
        }
        sum
    }

    #[test]
    fn table_data_checksum_stable() {
        let sum = checksum_all_tables();
        assert_eq!(
            sum, 16_209_751_036_793_560_317,
            "Huffman table data checksum changed -- if this is an \
             intentional data fix, re-verify against the sources cited \
             in this module's doc comment before updating the expected \
             value. Got {sum}"
        );
    }

    /// True if `shorter`'s code is a bit-prefix of `longer`'s code
    /// (codes are MSB-aligned within their `bits` length -- see
    /// [`VlcEntry`]'s doc comment). Two entries in the same table where
    /// either is a prefix of the other make it ambiguous to decode: a
    /// streaming decoder reading bit-by-bit could stop at the shorter
    /// code even when the longer one was the one actually transmitted.
    fn is_prefix(shorter: &VlcEntry, longer: &VlcEntry) -> bool {
        if shorter.bits >= longer.bits {
            return false;
        }
        (longer.code >> (longer.bits - shorter.bits)) == shorter.code
    }

    #[test]
    fn all_big_values_tables_are_prefix_free() {
        // The strongest kind of table-provenance test per chapter 13:
        // validates actual decodability, not just data stability.
        for (idx, table) in VLC_TABLES.iter().enumerate() {
            if table.xlen == 0 {
                continue; // table 0: trivially empty, nothing to check
            }
            let entries = table.lookup;
            for i in 0..entries.len() {
                for j in (i + 1)..entries.len() {
                    let (a, b) = (&entries[i], &entries[j]);
                    assert!(
                        !is_prefix(a, b) && !is_prefix(b, a),
                        "vlc table index {idx}: entries {i} and {j} violate \
                         the prefix-free property (a={a:?}, b={b:?})"
                    );
                }
            }
        }
    }

    #[test]
    fn both_count1_tables_are_prefix_free() {
        for (idx, table) in COUNT1_TABLES.iter().enumerate() {
            let entries = table.entries;
            for i in 0..entries.len() {
                for j in (i + 1)..entries.len() {
                    let (a, b) = (&entries[i], &entries[j]);
                    assert!(
                        !is_prefix(a, b) && !is_prefix(b, a),
                        "count1 table {idx}: entries {i} and {j} violate the \
                         prefix-free property (a={a:?}, b={b:?})"
                    );
                }
            }
        }
    }

    #[test]
    fn table_shapes_are_consistent() {
        assert_eq!(VLC_TABLES.len(), 16, "16 distinct trees expected (§2)");
        assert_eq!(COUNT1_TABLES.len(), 2, "exactly 2 count1 trees (§2)");
        for (idx, table) in VLC_TABLES.iter().enumerate() {
            assert_eq!(
                table.lookup.len(),
                table.xlen * table.xlen,
                "vlc table index {idx}: lookup length != xlen*xlen"
            );
        }
    }

    #[test]
    fn reserved_and_valid_ids_match_chapter_09() {
        // Chapter 09 §2: only IDs 4 and 14 are unused/reserved. ID 0 is
        // valid (a trivially-empty tree) -- an earlier version marked it
        // `None` too, conflating "reserved" with "trivial".
        for (id, entry) in BIG_VALUES_TABLES.iter().enumerate() {
            let should_be_reserved = id == 4 || id == 14;
            assert_eq!(
                entry.is_none(),
                should_be_reserved,
                "ID {id}: reserved-ness doesn't match chapter 09 §2"
            );
        }
    }

    #[test]
    fn escape_linbits_sequences_match_chapter_09() {
        // IDs 16-23 share tree 16 (vlc_table_index 14); IDs 24-31 share
        // tree 24 (vlc_table_index 15) -- chapter 09 §2, cross-checked
        // against FFmpeg's mpa_huff_data[32][2].
        let expected_16_23 = [1u8, 2, 3, 4, 6, 8, 10, 13];
        for (offset, &lb) in expected_16_23.iter().enumerate() {
            let id = 16 + offset;
            let (vlc_idx, linbits) = BIG_VALUES_TABLES[id].unwrap();
            assert_eq!(vlc_idx, 14, "ID {id} should share vlc_table_index 14");
            assert_eq!(linbits, lb, "ID {id} linbits mismatch");
        }
        let expected_24_31 = [4u8, 5, 6, 7, 8, 9, 11, 13];
        for (offset, &lb) in expected_24_31.iter().enumerate() {
            let id = 24 + offset;
            let (vlc_idx, linbits) = BIG_VALUES_TABLES[id].unwrap();
            assert_eq!(vlc_idx, 15, "ID {id} should share vlc_table_index 15");
            assert_eq!(linbits, lb, "ID {id} linbits mismatch");
        }
    }
}
