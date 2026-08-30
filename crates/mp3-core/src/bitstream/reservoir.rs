//! The bit reservoir and CBR/ABR/VBR rate control. See
//! `docs/mp3-encoder/10-phase7-bit-reservoir-and-rate-control.md`.

use crate::types::{Bitrate, MpegVersion, SampleRate};

// `no_std`-safe: `f32::round` is `std`-only (backed by the platform's
// libm) and doesn't exist under `core` -- call as a free function, never
// as `x.round()` method syntax. See
// `docs/mp3-encoder/verification/manifest.yaml`'s build-wasm note; this
// exact regression class already broke M2/M3 and M4 before this file.
use libm::roundf;

/// How the encoder allocates bits across frames. See
/// `docs/mp3-encoder/10-phase7-bit-reservoir-and-rate-control.md` §4.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub enum RateControl {
    /// Fixed nominal bitrate per frame; the reservoir only smooths local
    /// complexity spikes — average output bitrate equals the target
    /// exactly.
    Cbr(Bitrate),
    /// Nominal bitrate is a target *average*, not a per-frame guarantee.
    Abr(Bitrate),
    /// No fixed nominal bitrate — each frame's bitrate is whichever
    /// standard value is the smallest that fits the bits the
    /// quality-driven quantization loop actually produced.
    Vbr(VbrQuality),
}

impl RateControl {
    /// Returns the nominal bitrate for this rate-control mode.
    /// For CBR and ABR, this is the target bitrate. For VBR, returns a
    /// default of 128 kbps (used for frame-size calculation; actual
    /// per-frame bitrate varies).
    #[must_use]
    pub fn nominal_bitrate(self) -> Bitrate {
        match self {
            Self::Cbr(br) | Self::Abr(br) => br,
            Self::Vbr(_) => Bitrate::Kbps128,
        }
    }
}

/// VBR quality target. Scale/meaning TBD at implementation time (a
/// LAME-`-V`-style 0-9 scale is a reasonable, well-precedented choice —
/// see `docs/mp3-encoder/02-standards-and-prior-art.md` §4 on borrowing
/// approach, not code).
#[derive(Debug, Clone, Copy)]
pub struct VbrQuality(pub u8);

/// Tracks banked bits available for a future frame to spend beyond its
/// own nominal allocation. See
/// `docs/mp3-encoder/10-phase7-bit-reservoir-and-rate-control.md` §1-2.
pub struct BitReservoir {
    /// Bits currently "banked" — available to be spent by a future frame
    /// beyond that frame's own nominal allocation. Bounded above by
    /// `max_reservoir_bits`.
    available_bits: u32,
    /// The cap follows from `main_data_begin`'s width: 511 bytes (4088
    /// bits) for MPEG-1 (9-bit field), 255 bytes (2040 bits) for MPEG-2
    /// LSF (8-bit field). Construct per [`crate::types::MpegVersion`],
    /// see `docs/mp3-encoder/10-phase7-bit-reservoir-and-rate-control.md`
    /// §1-2.
    max_reservoir_bits: u32,
}

impl BitReservoir {
    /// Creates an empty reservoir with the given cap (in bits).
    #[must_use]
    pub const fn new(max_reservoir_bits: u32) -> Self {
        Self {
            available_bits: 0,
            max_reservoir_bits,
        }
    }

    /// Returns the reservoir cap for the given MPEG version (in bits).
    ///
    /// MPEG-1: 511 bytes x 8 = 4088 bits (9-bit `main_data_begin`).
    /// MPEG-2 LSF: 255 bytes x 8 = 2040 bits (8-bit `main_data_begin`).
    #[must_use]
    pub const fn max_for_version(version: MpegVersion) -> u32 {
        match version {
            MpegVersion::Mpeg1 => 511 * 8,
            MpegVersion::Mpeg2Lsf => 255 * 8,
        }
    }

    /// How many bits this frame's granules may spend in total, including
    /// the frame's own nominal allocation. The returned value is the
    /// **total bit budget** for the frame's audio data (both granules
    /// combined).
    ///
    /// The frame can borrow from the reservoir up to the lesser of what's
    /// currently banked and the reservoir's cap. Since
    /// `available_bits <= max_reservoir_bits` by construction, the total
    /// budget simplifies to `nominal_frame_bits + available_bits`.
    #[must_use]
    pub fn available_for_frame(&self, nominal_frame_bits: u32) -> u32 {
        nominal_frame_bits.saturating_add(self.available_bits)
    }

    /// Current banked bits (for inspection / tests).
    #[must_use]
    pub fn banked_bits(&self) -> u32 {
        self.available_bits
    }

    /// Updates the bank after a frame's actual usage is known (called
    /// once Huffman coding has produced the real bit count).
    ///
    /// If the frame used **fewer** bits than its nominal allocation, the
    /// surplus is banked for future frames (capped at
    /// `Self::max_reservoir_bits`).
    ///
    /// If the frame used **more** bits than its nominal allocation, the
    /// deficit is drawn from the bank (cannot go below zero — the
    /// quantization inner loop must never request more than
    /// [`Self::available_for_frame`] returns).
    pub fn record_frame_usage(&mut self, nominal_frame_bits: u32, actual_bits_used: u32) {
        if actual_bits_used <= nominal_frame_bits {
            let surplus = nominal_frame_bits - actual_bits_used;
            self.available_bits = (self.available_bits + surplus).min(self.max_reservoir_bits);
        } else {
            let deficit = actual_bits_used - nominal_frame_bits;
            self.available_bits = self.available_bits.saturating_sub(deficit);
        }
    }
}

/// Computes the frame size in **bytes** (Layer III "slots") for a given
/// bitrate, sample rate, and padding state. See
/// `docs/mp3-encoder/10-phase7-bit-reservoir-and-rate-control.md` §4.
///
/// ```text
/// MPEG-1:      frame_bytes = floor(144000 * bitrate_kbps / sample_rate_hz) + padding
/// MPEG-2 LSF:  frame_bytes = floor( 72000 * bitrate_kbps / sample_rate_hz) + padding
/// ```
///
/// The constants `144000` (= 1152/8 * 1000) and `72000` (= 576/8 * 1000)
/// follow from the number of samples per frame (two granules for MPEG-1,
/// one for MPEG-2 LSF).
#[must_use]
pub fn frame_bytes_for_bitrate(bitrate: Bitrate, sample_rate: SampleRate, padding: bool) -> u32 {
    let version = sample_rate.version();
    let kbps = bitrate.as_kbps();
    let hz = sample_rate.as_hz();

    let base = match version {
        MpegVersion::Mpeg1 => 144_000 * kbps / hz,
        MpegVersion::Mpeg2Lsf => 72_000 * kbps / hz,
    };

    base + u32::from(padding)
}

/// Determines whether a frame should be padded, given the running
/// fractional-sample accumulator. Returns `(padding, updated_accumulator)`.
///
/// The MP3 frame-size formula rarely yields an integer: e.g. 128 kbps @
/// 44.1 kHz gives 417.9592... bytes. The encoder alternates padded and
/// unpadded frames so the long-run average hits the nominal rate exactly.
///
/// Call this once per frame, threading the accumulator through:
///
/// ```text
/// let (pad, acc) = padding_bit_for_frame(version, kbps, hz, accumulator);
/// accumulator = acc;
/// // bytes_this_frame = frame_bytes_for_bitrate(bitrate, sample_rate, pad)
/// ```
///
/// The accumulator tracks the running fractional remainder from the
/// frame-size division, carried forward frame to frame. Initialize it to
/// `0`.
///
/// # Why this compares a *carry*, not the remainder's magnitude
///
/// `numerator` is fixed per (bitrate, sample_rate) pair, so
/// `base = numerator / sample_rate_hz` (the same floor value
/// [`frame_bytes_for_bitrate`] always uses) never changes across frames.
/// Padding is needed on a frame exactly when accumulating this frame's
/// share of the running fractional remainder pushes the *running total*
/// past the next multiple of `sample_rate_hz` -- i.e. when
/// `(numerator + accumulator) / sample_rate_hz` computes to `base + 1`
/// instead of `base`. That is a carry-out-of-the-division test, not a
/// "is the remainder more than half of `sample_rate_hz`" test.
///
/// An earlier version of this function used `remainder * 2 >=
/// sample_rate_hz` (comparing the *new* remainder's magnitude to a
/// half-cycle threshold) instead of comparing against the carry. That
/// converges to the wrong long-run average -- verified by simulating
/// 100,000 frames at 128 kbps / 44.1 kHz: the old formula settles at an
/// average frame size of ~417.49 bytes against a true target of
/// 417.9592, i.e. a real output bitrate measurably below the configured
/// 128 kbps. The carry-based formula below converges to 417.95918,
/// matching the true value to 5 decimal places -- see
/// `padding_long_run_average_matches_true_fractional_size` below.
#[must_use]
pub fn padding_bit_for_frame(
    version: MpegVersion,
    bitrate_kbps: u32,
    sample_rate_hz: u32,
    accumulator: u32,
) -> (bool, u32) {
    let numerator = match version {
        MpegVersion::Mpeg1 => 144_000 * bitrate_kbps,
        MpegVersion::Mpeg2Lsf => 72_000 * bitrate_kbps,
    };

    let base = numerator / sample_rate_hz;
    let total = numerator + accumulator;
    let new_base = total / sample_rate_hz;
    let remainder = total - new_base * sample_rate_hz;

    let padding = new_base > base;

    (padding, remainder)
}

/// Split a frame's total bit budget between its two granules. See
/// `docs/mp3-encoder/10-phase7-bit-reservoir-and-rate-control.md` §3.
///
/// The default policy is an even 50/50 split, adjusted by each granule's
/// perceptual entropy (PE) from the psychoacoustic model: a granule with
/// higher PE (more complex content) gets a larger share. This is a
/// **named policy function** — easy to find and retune once chapter 13's
/// quality feedback loop exists; do not bury the split ratio as an inline
/// arithmetic expression in `encoder.rs`.
///
/// # Arguments
///
/// * `total_bits` — total bit budget for the frame's audio data (both
///   granules combined), from [`BitReservoir::available_for_frame`].
/// * `pe_granule0` — perceptual entropy of granule 0.
/// * `pe_granule1` — perceptual entropy of granule 1.
///
/// # Returns
///
/// `(bits_granule0, bits_granule1)`.
pub fn split_bits_for_granules(total_bits: u32, pe_granule0: f32, pe_granule1: f32) -> (u32, u32) {
    let total = total_bits as f32;
    let pe_sum = pe_granule0 + pe_granule1;

    if pe_sum <= 0.0 {
        // Equal split when both PE values are zero or negative.
        let half = total_bits / 2;
        return (half, total_bits - half);
    }

    let share0 = roundf(pe_granule0 / pe_sum * total) as u32;
    let share0 = share0.min(total_bits);
    (share0, total_bits - share0)
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)] // f32::abs() in test assertions -- see docs/investigation-log.md §7 item 6
mod tests {
    use super::*;

    // -------------------------------------------------------------------
    // BitReservoir tests
    // -------------------------------------------------------------------

    #[test]
    fn empty_reservoir_lends_nothing() {
        let r = BitReservoir::new(4088);
        assert_eq!(r.available_for_frame(1000), 1000);
        assert_eq!(r.banked_bits(), 0);
    }

    #[test]
    fn surplus_is_banked() {
        let mut r = BitReservoir::new(4088);
        // Nominal = 5000, used 3000 → surplus 2000 banked.
        r.record_frame_usage(5000, 3000);
        assert_eq!(r.banked_bits(), 2000);
        assert_eq!(r.available_for_frame(5000), 7000);
    }

    #[test]
    fn deficit_draws_from_bank() {
        let mut r = BitReservoir::new(4088);
        // First frame banks 2000.
        r.record_frame_usage(5000, 3000);
        assert_eq!(r.banked_bits(), 2000);
        // Second frame uses 1000 more than nominal → draws 1000.
        r.record_frame_usage(5000, 6000);
        assert_eq!(r.banked_bits(), 1000);
    }

    #[test]
    fn bank_never_goes_negative() {
        let mut r = BitReservoir::new(4088);
        // Empty reservoir, frame uses 1000 more than nominal.
        r.record_frame_usage(5000, 6000);
        assert_eq!(r.banked_bits(), 0);
        // Available should still be nominal (no bank to borrow from).
        assert_eq!(r.available_for_frame(5000), 5000);
    }

    #[test]
    fn bank_never_exceeds_cap() {
        let mut r = BitReservoir::new(100);
        // Frame uses way less than nominal: surplus = 5000, but cap = 100.
        r.record_frame_usage(5000, 0);
        assert_eq!(r.banked_bits(), 100);
        // Multiple cheap frames should not exceed cap.
        r.record_frame_usage(5000, 0);
        assert_eq!(r.banked_bits(), 100);
    }

    #[test]
    fn alternating_over_under_usage() {
        let mut r = BitReservoir::new(4088);
        let nominal: u32 = 5000;

        // Frame 0: under budget (+1000).
        r.record_frame_usage(nominal, 4000);
        assert_eq!(r.banked_bits(), 1000);

        // Frame 1: over budget (-1500).
        r.record_frame_usage(nominal, 6500);
        assert_eq!(r.banked_bits(), 0); // 1000 - 1500 = saturate to 0.

        // Frame 2: under budget (+2000).
        r.record_frame_usage(nominal, 3000);
        assert_eq!(r.banked_bits(), 2000);

        // Frame 3: under budget (+500).
        r.record_frame_usage(nominal, 4500);
        assert_eq!(r.banked_bits(), 2500);

        // Frame 4: over budget (-3000, but banked only 2500).
        r.record_frame_usage(nominal, 8000);
        assert_eq!(r.banked_bits(), 0);
    }

    #[test]
    fn max_for_version_correct() {
        assert_eq!(BitReservoir::max_for_version(MpegVersion::Mpeg1), 4088);
        assert_eq!(BitReservoir::max_for_version(MpegVersion::Mpeg2Lsf), 2040);
    }

    // -------------------------------------------------------------------
    // Frame-size formula tests
    // -------------------------------------------------------------------

    #[test]
    fn frame_size_128kbps_44100hz() {
        // 128 kbps @ 44.1 kHz → floor(144000 * 128 / 44100) = 417
        assert_eq!(
            frame_bytes_for_bitrate(Bitrate::Kbps128, SampleRate::Hz44100, false),
            417
        );
        assert_eq!(
            frame_bytes_for_bitrate(Bitrate::Kbps128, SampleRate::Hz44100, true),
            418
        );
    }

    #[test]
    fn frame_size_192kbps_48000hz() {
        // 192 kbps @ 48 kHz → floor(144000 * 192 / 48000) = 576 exactly.
        assert_eq!(
            frame_bytes_for_bitrate(Bitrate::Kbps192, SampleRate::Hz48000, false),
            576
        );
        assert_eq!(
            frame_bytes_for_bitrate(Bitrate::Kbps192, SampleRate::Hz48000, true),
            577
        );
    }

    #[test]
    fn frame_size_64kbps_22050hz_lsf() {
        // 64 kbps @ 22.05 kHz (LSF) → floor(72000 * 64 / 22050) = 208
        assert_eq!(
            frame_bytes_for_bitrate(Bitrate::Kbps64, SampleRate::Hz22050, false),
            208
        );
        assert_eq!(
            frame_bytes_for_bitrate(Bitrate::Kbps64, SampleRate::Hz22050, true),
            209
        );
    }

    #[test]
    fn frame_size_320kbps_44100hz() {
        // 320 kbps @ 44.1 kHz → floor(144000 * 320 / 44100) = 1044
        assert_eq!(
            frame_bytes_for_bitrate(Bitrate::Kbps320, SampleRate::Hz44100, false),
            1044
        );
    }

    #[test]
    fn frame_size_32kbps_32000hz() {
        // 32 kbps @ 32 kHz → floor(144000 * 32 / 32000) = 144
        assert_eq!(
            frame_bytes_for_bitrate(Bitrate::Kbps32, SampleRate::Hz32000, false),
            144
        );
    }

    // -------------------------------------------------------------------
    // Padding-bit accumulator tests
    // -------------------------------------------------------------------

    #[test]
    fn padding_pattern_for_128kbps_44100hz() {
        // 144000 * 128 = 18,432,000. base = 18,432,000 / 44,100 = 417
        // (the constant floor value -- frame_bytes_for_bitrate always
        // uses this). Traced by direct computation (not simulation) for
        // the first 4 frames, verifying the *carry* condition frame by
        // frame -- see padding_bit_for_frame's doc comment.
        //
        // frame 0: acc_in=0.      total=18,432,000. new_base=417 (no
        //          carry, since total == numerator exactly here) -> no
        //          pad. remainder = 42,300.
        let (pad0, acc0) = padding_bit_for_frame(MpegVersion::Mpeg1, 128, 44100, 0);
        assert!(!pad0, "frame 0 should not be padded");
        assert_eq!(acc0, 42_300);

        // frame 1: acc_in=42,300. total=18,474,300. new_base=418 (carry)
        //          -> pad. remainder = 40,500.
        let (pad1, acc1) = padding_bit_for_frame(MpegVersion::Mpeg1, 128, 44100, acc0);
        assert!(
            pad1,
            "frame 1 should be padded (fractional part is 0.96, not 0.5)"
        );
        assert_eq!(acc1, 40_500);

        // Given how close the fractional part (0.9592...) is to 1, the
        // overwhelming majority of frames should carry (pad) -- only a
        // rare frame doesn't, when the accumulator briefly resets near 0.
        let mut acc = acc1;
        let mut pad_count = 0u32;
        for _ in 0..100 {
            let (p, a) = padding_bit_for_frame(MpegVersion::Mpeg1, 128, 44100, acc);
            if p {
                pad_count += 1;
            }
            acc = a;
        }
        assert!(
            pad_count > 90,
            "expected ~96 padded frames per 100 (true value is 417.9592...), got {pad_count}"
        );
    }

    #[test]
    fn padding_long_run_average_matches_true_fractional_size() {
        // The actual property padding_bit_for_frame exists to guarantee:
        // over many frames, the average frame size converges to the
        // *true* (fractional) frame size -- not just "pads sometimes".
        // This is what makes a CBR stream's real average bitrate match
        // its configured target; the bug this test guards against
        // (comparing remainder magnitude instead of testing for a division
        // carry) converged to a visibly wrong average (417.49 vs.
        // 417.9592...) despite "looking reasonable" in isolated samples.
        for &(kbps, hz) in &[
            (128u32, 44_100u32),
            (192, 48_000), // exact division -- average must be exactly 576
            (256, 44_100),
            (96, 32_000),
        ] {
            let numerator = 144_000u64 * u64::from(kbps);
            let base = (numerator / u64::from(hz)) as u32;
            let true_value = numerator as f64 / f64::from(hz);

            let mut acc = 0u32;
            let mut total_bytes: u64 = 0;
            let n: u64 = 20_000;
            for _ in 0..n {
                let (pad, new_acc) = padding_bit_for_frame(MpegVersion::Mpeg1, kbps, hz, acc);
                acc = new_acc;
                total_bytes += u64::from(base + u32::from(pad));
            }
            let average = total_bytes as f64 / n as f64;
            assert!(
                (average - true_value).abs() < 0.001,
                "{kbps}kbps@{hz}Hz: average frame size {average} should \
                 converge to {true_value}"
            );
        }
    }

    #[test]
    fn no_padding_when_division_is_exact() {
        // 192 kbps @ 48 kHz: 144000 * 192 / 48000 = 576 exactly.
        let (pad, acc) = padding_bit_for_frame(MpegVersion::Mpeg1, 192, 48000, 0);
        assert!(!pad);
        assert_eq!(acc, 0);
    }

    // -------------------------------------------------------------------
    // Granule split tests
    // -------------------------------------------------------------------

    #[test]
    fn equal_pe_gives_equal_split() {
        let (a, b) = split_bits_for_granules(1000, 5.0, 5.0);
        assert_eq!(a, 500);
        assert_eq!(b, 500);
    }

    #[test]
    fn zero_pe_gives_equal_split() {
        let (a, b) = split_bits_for_granules(1000, 0.0, 0.0);
        assert_eq!(a, 500);
        assert_eq!(b, 500);
    }

    #[test]
    fn higher_pe_gets_larger_share() {
        let (a, b) = split_bits_for_granules(1000, 8.0, 2.0);
        assert!(a > b, "granule 0 has higher PE ({a} > {b})");
        assert_eq!(a + b, 1000);
    }

    #[test]
    fn extreme_pe_skew() {
        let (a, b) = split_bits_for_granules(1000, 100.0, 1.0);
        assert!(a > 900, "expected strong skew, got a={a}");
        assert_eq!(a + b, 1000);
    }
}
