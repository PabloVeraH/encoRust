//! Bit-level output, side info, and the bit reservoir. See
//! `docs/mp3-encoder/10-phase7-bit-reservoir-and-rate-control.md` and
//! `docs/mp3-encoder/11-phase8-bitstream-multiplexing.md`.

pub mod reservoir;
pub mod side_info;
pub mod writer;

pub use reservoir::{BitReservoir, RateControl};
pub use side_info::SideInfo;
pub use writer::BitWriter;
