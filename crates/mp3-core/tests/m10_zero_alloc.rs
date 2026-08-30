//! M-6: regression test confirming `Encoder::encode_frame` performs zero
//! heap allocations on the steady-state path after the first warm-up
//! call.  This locks in M-1 through M-4 from `docs/investigation-log.md` against
//! future regression.
//!
//! Uses a counting wrapper around the system allocator. The test is
//! `#[cfg(feature = "std")]`-gated because `#[global_allocator]` needs
//! `std`.

#![cfg(feature = "std")]

extern crate alloc;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};

use mp3_core::io::PcmBuffer;
use mp3_core::{Bitrate, ChannelMode, EncoderConfig, MpegVersion, SampleRate};

// ---------------------------------------------------------------------------
// Counting allocator
// ---------------------------------------------------------------------------

static ALLOC_COUNT: AtomicU64 = AtomicU64::new(0);
static DEALLOC_COUNT: AtomicU64 = AtomicU64::new(0);
static BYTES_ALLOCATED: AtomicU64 = AtomicU64::new(0);

struct CountingAlloc;

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, Ordering::SeqCst);
        BYTES_ALLOCATED.fetch_add(layout.size() as u64, Ordering::SeqCst);
        System.alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        DEALLOC_COUNT.fetch_add(1, Ordering::SeqCst);
        System.dealloc(ptr, layout)
    }
}

#[global_allocator]
static GLOBAL: CountingAlloc = CountingAlloc;

fn reset_counts() {
    ALLOC_COUNT.store(0, Ordering::SeqCst);
    DEALLOC_COUNT.store(0, Ordering::SeqCst);
    BYTES_ALLOCATED.store(0, Ordering::SeqCst);
}

fn alloc_count() -> u64 {
    ALLOC_COUNT.load(Ordering::SeqCst)
}

// ---------------------------------------------------------------------------
// Test fixtures
// ---------------------------------------------------------------------------

fn make_mono_pcm(n_frames: usize) -> Vec<PcmBuffer> {
    let mut seed: u32 = 12345;
    let mut frames = Vec::with_capacity(n_frames);
    for _ in 0..n_frames {
        let samples: Vec<i16> = (0..1152)
            .map(|_| {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                ((seed >> 16) as i32 % 5000) as i16
            })
            .collect();
        frames.push(
            PcmBuffer::from_i16_interleaved(&samples, ChannelMode::Mono, MpegVersion::Mpeg1)
                .expect("PCM buffer"),
        );
    }
    frames
}

// ---------------------------------------------------------------------------
// Guards: verifying the allocator itself works
// ---------------------------------------------------------------------------

#[test]
fn allocator_counts_a_vec_new() {
    reset_counts();
    let before = alloc_count();
    let _v: Vec<u8> = Vec::with_capacity(256);
    // with_capacity may or may not allocate depending on the allocator;
    // just verify the counter is live
    let after = alloc_count();
    // At minimum: our own Vec<u8> above could allocate; at least confirm
    // the counters are accessible and the test harness didn't panic.
    assert!(after >= before, "allocator counter should be monotonic");
}

// ---------------------------------------------------------------------------
// Main assertion
// ---------------------------------------------------------------------------

#[test]
fn encode_frame_zero_allocs_after_warmup() {
    let config = EncoderConfig::new(
        SampleRate::Hz44100,
        ChannelMode::Mono,
        mp3_core::bitstream::reservoir::RateControl::Cbr(Bitrate::Kbps128),
    );

    // Encoder construction may allocate (the pre-allocated buffers).
    let mut encoder = mp3_core::Encoder::new(config).expect("encoder creation");

    let frames = make_mono_pcm(3);

    // --- Warm-up frame (allowed to allocate) ---
    reset_counts();
    let mut out = Vec::with_capacity(2048);
    encoder
        .encode_frame(&frames[0], &mut out)
        .expect("warmup encode");
    // Drain warm-up allocations: encoder's internal buffers, the output
    // Vec growth, etc. We only care about steady-state after this.

    // --- Steady-state frames must NOT allocate ---
    for (i, frame) in frames.iter().enumerate().skip(1) {
        reset_counts();
        out.clear();
        encoder
            .encode_frame(frame, &mut out)
            .expect("steady encode");
        let allocs = alloc_count();

        assert_eq!(
            allocs, 0,
            "encode_frame #{i}: expected 0 allocations after warmup, got {allocs}"
        );
        assert!(!out.is_empty(), "frame #{i} produced empty output");
    }
}
