//! The fixed Huffman code tables (ISO/IEC 11172-3 Annex B, Table B.7:
//! 34 tables, IDs 0-33). See
//! `docs/mp3-encoder/09-phase6-huffman-coding.md` §2 for the precise
//! inventory: IDs 0-31 are selectable for `big_values` sub-regions
//! (5-bit `table_select`; IDs 4 and 14 reserved/unused), backed by only
//! 16 distinct code *trees* — IDs 16-23 share the tree-16 escape tree
//! with linbits 1,2,3,4,6,8,10,13 and IDs 24-31 share tree 24 with
//! linbits 4,5,6,7,8,9,11,13. IDs 32/33 ("A"/"B") are the two `count1`
//! quadruple tables.
//!
//! # ⚠️ Placeholder — not implemented
//!
//! The statics below are empty. **Do not hand-transcribe the code trees
//! from memory.** Generate this module's real content from a cited,
//! cross-checked source (Annex B directly, cross-checked against a
//! second independent source such as a permissively-licensed decoder's
//! table module) — see `docs/mp3-encoder/09-phase6-huffman-coding.md` §2
//! for the exact process, and prefer a `build.rs` reading a checked-in
//! flat data dump over hand-maintained Rust literals, per that section.
//! The linbits sequences above are part of the same provenance
//! requirement.

/// A `(x, y)` value pair, the key type for one [`HuffmanTree`] entry.
pub type ValuePair = (u8, u8);

/// A `(code, code_length)` pair, the value type for one [`HuffmanTree`]
/// entry — `code`'s low `code_length` bits are the MSB-first Huffman
/// code.
pub type Code = (u32, u8);

/// One distinct code tree: a value-pair (or value-quadruple, for
/// `count1`) lookup keyed by `(x, y)` mapping to `(code, code_length)`.
/// Escape trees (16/24) code magnitudes `0..=15` where 15 acts as ESC.
#[derive(Debug)]
pub struct HuffmanTree {
    /// `(x, y) -> (code, length)` entries. Empty in this placeholder.
    pub codes: &'static [(ValuePair, Code)],
}

/// One selectable `big_values` table ID: a shared tree plus that ID's
/// linbits count (0 for non-escape IDs).
#[derive(Debug, Clone, Copy)]
pub struct HuffmanTable {
    /// The code tree this ID uses (IDs 16-23 all point at tree 16,
    /// 24-31 at tree 24).
    pub tree: &'static HuffmanTree,
    /// Number of raw escape bits appended after an ESC code. `0` for
    /// non-escape IDs. See
    /// `docs/mp3-encoder/09-phase6-huffman-coding.md` §2.
    pub linbits: u8,
}

/// Selection table for `table_select` IDs `0..=31`. Entries 4 and 14
/// are `None` (reserved — a conforming encoder never selects them).
/// See this module's placeholder warning above.
pub static BIG_VALUES_TABLES: [Option<HuffmanTable>; 32] = [None; 32];

/// The two `count1` quadruple trees: Annex B IDs 32 ("A") and 33 ("B"),
/// picked by the 1-bit `count1table_select`. See this module's
/// placeholder warning above.
pub static COUNT1_TABLES: [HuffmanTree; 2] =
    [HuffmanTree { codes: &[] }, HuffmanTree { codes: &[] }];

#[cfg(test)]
mod tests {
    use super::{BIG_VALUES_TABLES, COUNT1_TABLES};

    #[test]
    fn placeholder_tables_are_empty() {
        // Replace with real table-provenance tests once M6 populates
        // these (checksum against a cited source; prefix-free invariant
        // per tree; IDs 4/14 None and only 4/14 None; linbits sequences
        // {1,2,3,4,6,8,10,13} / {4,5,6,7,8,9,11,13} on IDs 16-23/24-31;
        // IDs 16-23 sharing one &HuffmanTree and 24-31 another). See
        // docs/mp3-encoder/13-testing-and-validation.md §Table provenance.
        assert!(BIG_VALUES_TABLES.iter().all(Option::is_none));
        assert!(COUNT1_TABLES.iter().all(|t| t.codes.is_empty()));
    }
}
