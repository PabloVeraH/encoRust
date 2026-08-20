//! M0 smoke test: confirms the crate's public API surface compiles and
//! links, without exercising any of the (currently `todo!()`) DSP logic.
//!
//! Per-milestone integration tests belong in sibling files here
//! (`m1_framing.rs`, `m2_filterbank.rs`, ...) as each milestone lands —
//! see `docs/mp3-encoder/01-architecture.md` §6 and
//! `docs/mp3-encoder/14-roadmap-and-milestones.md`.

#[test]
fn public_types_are_reachable() {
    use mp3_core::{ChannelMode, MpegVersion, SampleRate};

    let rate = SampleRate::Hz44100;
    assert_eq!(rate.as_hz(), 44_100);
    assert_eq!(rate.version(), MpegVersion::Mpeg1);
    assert_eq!(ChannelMode::Stereo.channel_count(), 2);
}

#[test]
fn frame_size_is_version_dependent() {
    // The structural fact chapters 04/10/11 hinge on: MPEG-1 frames
    // carry 2 granules (1152 samples/channel), MPEG-2 LSF frames carry
    // only 1 (576). See docs/mp3-encoder/04-phase1-pcm-io-and-framing.md §1.
    use mp3_core::MpegVersion;

    assert_eq!(MpegVersion::Mpeg1.granules_per_frame(), 2);
    assert_eq!(MpegVersion::Mpeg1.samples_per_frame(), 1152);
    assert_eq!(MpegVersion::Mpeg2Lsf.granules_per_frame(), 1);
    assert_eq!(MpegVersion::Mpeg2Lsf.samples_per_frame(), 576);
}
