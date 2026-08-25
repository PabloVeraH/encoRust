//! End-to-end tests for the `encorust` binary: write a real WAV file,
//! run the compiled CLI against it as a subprocess, and check the
//! output. See `docs/mp3-encoder/12-phase9-cli-and-wasm.md` §1/§4 (M9
//! Definition of Done: "encodes a WAV file end-to-end at CBR, ABR, and
//! VBR").

use std::path::{Path, PathBuf};
use std::process::Command;

/// Writes a small WAV file of deterministic broadband pseudo-noise --
/// not silence or a pure tone, which (per the project's own experience
/// across M7/M8's reviews) compress to near-zero bits and don't
/// meaningfully exercise the encoding pipeline.
fn write_test_wav(path: &Path, sample_rate: u32, channels: u16, samples_per_channel: u32) {
    let spec = hound::WavSpec {
        channels,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).expect("create WAV");
    let mut seed: u32 = 12345;
    for _ in 0..(samples_per_channel * u32::from(channels)) {
        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
        let sample = ((seed >> 16) as i32 - 32768) / 4; // moderate amplitude
        writer.write_sample(sample as i16).expect("write sample");
    }
    writer.finalize().expect("finalize WAV");
}

fn run_cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_encorust"))
        .args(args)
        .output()
        .expect("run encorust binary")
}

/// Very small, self-contained MPEG frame header sanity check: syncword
/// (11 bits, all 1s) plus version/layer bits -- mirrors the checks
/// `mp3-core`'s own `tests/m8_bitstream.rs` does on raw bytes, applied
/// here to the CLI's actual file output rather than in-process buffers.
fn assert_looks_like_mpeg1_layer3(bytes: &[u8]) {
    assert!(
        bytes.len() >= 4,
        "output too short to contain a frame header"
    );
    let header = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    assert_eq!(header >> 21, 0x7FF, "invalid syncword");
    assert_eq!((header >> 19) & 0x3, 0b11, "expected MPEG-1");
    assert_eq!((header >> 17) & 0x3, 0b01, "expected Layer III");
}

fn scratch_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("encorust-cli-test-{}-{name}", std::process::id()))
}

#[test]
fn encodes_cbr_stereo_wav_end_to_end() {
    let input = scratch_path("cbr-in.wav");
    let output = scratch_path("cbr-out.mp3");
    write_test_wav(&input, 44_100, 2, 4096);

    let result = run_cli(&[
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--bitrate",
        "128",
    ]);
    assert!(
        result.status.success(),
        "CBR encode failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );

    let mp3_bytes = std::fs::read(&output).expect("read CBR output");
    assert_looks_like_mpeg1_layer3(&mp3_bytes);
    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(&output);
}

#[test]
fn abr_vbr_rejected_with_clear_message() {
    // ABR and VBR are not yet implemented — verifying the CLI surfaces
    // this clearly rather than silently producing fixed-bitrate CBR
    // output (see docs/mejoras.md §2.2). An intermediate version of this
    // branch accepted `--abr` while it behaved identically to CBR under
    // the hood — reintroducing exactly the silent-no-op anti-pattern
    // this test exists to catch; see docs/plus.md's review notes.
    let input = scratch_path("reject-in.wav");
    let output = scratch_path("reject-out.mp3");
    write_test_wav(&input, 44_100, 2, 4096);

    let abr = run_cli(&[
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--abr",
        "128",
    ]);
    assert!(
        !abr.status.success(),
        "ABR must be rejected (not yet implemented)"
    );
    let stderr = String::from_utf8_lossy(&abr.stderr);
    assert!(
        stderr.contains("not yet implemented"),
        "ABR rejection must surface clearly, got: {stderr}"
    );

    let vbr = run_cli(&[
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--vbr-quality",
        "4",
    ]);
    assert!(
        !vbr.status.success(),
        "VBR must be rejected (not yet implemented)"
    );
    let stderr = String::from_utf8_lossy(&vbr.stderr);
    assert!(
        stderr.contains("not yet implemented"),
        "VBR rejection must surface clearly, got: {stderr}"
    );

    let _ = std::fs::remove_file(&input);
}

#[test]
fn joint_stereo_flag_encodes_successfully() {
    let input = scratch_path("joint-stereo-in.wav");
    let output = scratch_path("joint-stereo-out.mp3");
    write_test_wav(&input, 44_100, 2, 4096);

    let result = run_cli(&[
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--bitrate",
        "128",
        "--joint-stereo",
    ]);
    assert!(
        result.status.success(),
        "--joint-stereo must encode successfully, got: {}",
        String::from_utf8_lossy(&result.stderr)
    );

    let mp3_bytes = std::fs::read(&output).expect("read joint-stereo output");
    assert_looks_like_mpeg1_layer3(&mp3_bytes);

    let mono_input = scratch_path("joint-stereo-mono-in.wav");
    write_test_wav(&mono_input, 44_100, 1, 4096);
    let mono_result = run_cli(&[
        mono_input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--bitrate",
        "128",
        "--joint-stereo",
    ]);
    assert!(
        !mono_result.status.success(),
        "--joint-stereo with mono input must be rejected"
    );

    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(&mono_input);
    let _ = std::fs::remove_file(&output);
}

#[test]
fn encodes_mono_wav_end_to_end() {
    let input = scratch_path("mono-in.wav");
    let output = scratch_path("mono-out.mp3");
    write_test_wav(&input, 44_100, 1, 4096);

    let result = run_cli(&[
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--bitrate",
        "96",
    ]);
    assert!(
        result.status.success(),
        "mono encode failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );

    let mp3_bytes = std::fs::read(&output).expect("read mono output");
    assert_looks_like_mpeg1_layer3(&mp3_bytes);
    let mode = (u32::from(mp3_bytes[3]) >> 6) & 0x3;
    assert_eq!(mode, 0b11, "expected mono channel mode in the header");
    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(&output);
}

#[test]
fn rejects_bitrate_invalid_for_sample_rate() {
    // 144 kbps is MPEG-2 LSF-only; 44.1 kHz is MPEG-1 -- locks in the
    // `mp3_core::Encoder::new` validation added during the M9 review
    // (previously this bitrate/version mismatch wasn't caught until
    // deep inside the first frame's encode call).
    let input = scratch_path("bad-bitrate-in.wav");
    let output = scratch_path("bad-bitrate-out.mp3");
    write_test_wav(&input, 44_100, 2, 4096);

    let result = run_cli(&[
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--bitrate",
        "144",
    ]);
    assert!(
        !result.status.success(),
        "144 kbps at 44.1 kHz must be rejected, not silently mis-encoded"
    );
    let _ = std::fs::remove_file(&input);
}

#[test]
fn rejects_when_no_rate_control_flag_given() {
    let input = scratch_path("no-flag-in.wav");
    let output = scratch_path("no-flag-out.mp3");
    write_test_wav(&input, 44_100, 2, 1152);

    let result = run_cli(&[input.to_str().unwrap(), "-o", output.to_str().unwrap()]);
    assert!(
        !result.status.success(),
        "must require one of --bitrate/--abr/--vbr-quality"
    );
    let _ = std::fs::remove_file(&input);
}

#[test]
fn rejects_missing_input_file() {
    let output = scratch_path("missing-input-out.mp3");
    let result = run_cli(&[
        "/nonexistent/path/does-not-exist.wav",
        "-o",
        output.to_str().unwrap(),
        "--bitrate",
        "128",
    ]);
    assert!(
        !result.status.success(),
        "must fail loudly on a missing input file"
    );
}
