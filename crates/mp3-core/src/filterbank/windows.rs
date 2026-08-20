//! The 512-tap analysis prototype filter. See
//! `docs/mp3-encoder/05-phase2-polyphase-filterbank.md` §1 step 2.

/// The 512-tap Layer I/II/III analysis prototype filter
/// (ISO/IEC 11172-3 Annex B, Table B.9 — confirm the table number
/// against your edition of the standard; it's the *values*, not the
/// number, that must match).
///
/// # ⚠️ Placeholder — not implemented
///
/// This array is **zeroed**, not filled in. Per
/// `docs/mp3-encoder/00-overview.md` §4.1, these 512 constants must be
/// sourced from Annex B directly (or cross-validated against ≥2
/// independent implementations — see
/// `docs/mp3-encoder/05-phase2-polyphase-filterbank.md` §1 step 2 and
/// `docs/mp3-encoder/13-testing-and-validation.md` §Table provenance)
/// before this is usable. A zeroed filter compiles and type-checks but
/// produces silent, all-zero subband output — do not mistake "it
/// compiles" for "it works" with this table specifically.
pub const ANALYSIS_PROTOTYPE_FILTER: [f32; 512] = [0.0; 512];

#[cfg(test)]
mod tests {
    use super::ANALYSIS_PROTOTYPE_FILTER;

    #[test]
    fn placeholder_is_not_yet_the_real_table() {
        // This test exists to be *replaced* by the real table-provenance
        // test (checksum against a cited source) once M2 populates the
        // array for real — see
        // docs/mp3-encoder/13-testing-and-validation.md §Table provenance.
        // Until then it simply documents the current placeholder state
        // so a `cargo test` pass is never mistaken for "M2 done."
        assert!(
            ANALYSIS_PROTOTYPE_FILTER.iter().all(|&c| c == 0.0),
            "replace this test with a real table-provenance checksum \
             once ANALYSIS_PROTOTYPE_FILTER is populated (M2)"
        );
    }
}
