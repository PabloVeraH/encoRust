//! The sliding-window polyphase analysis filter. See
//! `docs/mp3-encoder/05-phase2-polyphase-filterbank.md` §1 and §3.

use crate::types::SUBBANDS;

/// Sliding 512-sample analysis buffer + prototype filter application.
///
/// One instance per channel — the 512-sample history is a continuous
/// window over the entire PCM stream (seeded with zeros at stream
/// start), **not** reset per frame or per granule. See
/// `docs/mp3-encoder/05-phase2-polyphase-filterbank.md` §3 for the exact
/// shift/indexing convention this must follow.
pub struct PolyphaseFilterbank {
    history: [f32; 512],
}

impl Default for PolyphaseFilterbank {
    fn default() -> Self {
        Self::new()
    }
}

impl PolyphaseFilterbank {
    /// Creates a filterbank with a zeroed history (stream start).
    #[must_use]
    pub const fn new() -> Self {
        Self {
            history: [0.0; 512],
        }
    }

    /// Feeds exactly 32 new PCM samples (natural time order — oldest to
    /// newest within the chunk) and produces the 32 subband output
    /// samples for this analysis step.
    ///
    /// Called 18 times per granule
    /// (`18 * 32 == SAMPLES_PER_GRANULE`, see
    /// [`crate::types::SAMPLES_PER_GRANULE`]).
    ///
    /// # Panics
    ///
    /// Always, in this scaffold. Implements
    /// `docs/mp3-encoder/05-phase2-polyphase-filterbank.md` §1: shift
    /// history, apply the prototype filter
    /// ([`crate::filterbank::ANALYSIS_PROTOTYPE_FILTER`]), partial-sum to
    /// 64 values, then matrix to 32 subband outputs via the closed-form
    /// cosine matrix given in that section.
    pub fn analyze(&mut self, pcm_chunk: &[f32; 32]) -> [f32; SUBBANDS] {
        let _ = pcm_chunk;
        let _ = &self.history;
        todo!("M2: shift history + apply prototype filter + partial-sum + matrix")
    }
}

#[cfg(test)]
mod tests {
    // TODO(M2): frequency-response test (sine wave at a known frequency
    // concentrates energy in the expected subband) and impulse-response
    // test (unit impulse reproduces the prototype filter shape split
    // across subbands). See
    // docs/mp3-encoder/05-phase2-polyphase-filterbank.md §4.
}
