//! The 32-band polyphase analysis filterbank (the first stage of Layer
//! III's hybrid filterbank). See
//! `docs/mp3-encoder/05-phase2-polyphase-filterbank.md`.

mod polyphase;
mod windows;

pub use polyphase::PolyphaseFilterbank;
pub use windows::ANALYSIS_PROTOTYPE_FILTER;
