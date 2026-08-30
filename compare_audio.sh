#!/bin/bash
# ═════════════════════════════════════════════════════════════════════
# Compare original WAV with encoded MP3
# Generates spectrograms, waveforms, loudness stats, and -- the part
# that actually matters for tuning the encoder -- a lag-aligned
# time-domain correlation scan plus per-band distortion profiling.
#
# WHY: spectrograms and waveform-envelope views can look "basically
# right" even when the encoder output is almost completely uncorrelated
# with the source at the sample level (see docs/investigation-log.md §11 for the
# investigation that discovered this the hard way). Treat the images
# below as a sanity check only -- the correlation/per-band section is
# the number to actually chase while tuning.
# ═════════════════════════════════════════════════════════════════════

set -uo pipefail

WAV_FILE="${1:?Usage: $0 <input.wav> <output.mp3>}"
MP3_FILE="${2:?Usage: $0 <input.wav> <output.mp3>}"

# Tunables (override via env, e.g. `MAX_LAG=5000 ./compare_audio.sh ...`).
# Defaults match the methodology validated in docs/investigation-log.md §11.
SEG_SECONDS="${SEG_SECONDS:-0.2}"     # length of each correlation window
MAX_LAG="${MAX_LAG:-3000}"           # samples searched either side (±68ms @44.1kHz)
N_SEGMENTS="${N_SEGMENTS:-4}"        # how many points across the file to sample
DECAY_RADIUS="${DECAY_RADIUS:-6}"    # samples either side of the peak to show its shape
SKIP_IMAGES="${SKIP_IMAGES:-0}"      # set to 1 to skip waveform/spectrogram PNGs (faster reruns)

# NOTE: deliberately NOT system /tmp -- a sandboxed (snap) ffmpeg can
# reject it outright ("No such file or directory" despite the file
# genuinely existing; see docs/investigation-log.md §11's reproduction notes for
# the same gotcha hit during the investigation this script supports).
# A dir under the current working directory is always writable and
# always in ffmpeg's reach.
TMPDIR=$(mktemp -d "./.compare_audio_tmp.XXXXXX")
trap 'rm -rf "$TMPDIR"' EXIT

echo "==> Analyzing audio quality..."
echo ""

# === FILE INFO ===
echo "=== FILE INFO ==="
echo "WAV: $(du -h "$WAV_FILE" | cut -f1) - $(ffprobe -v quiet -show_entries format=duration -of csv=p=0 "$WAV_FILE" 2>/dev/null || echo 'N/A')s"
echo "MP3: $(du -h "$MP3_FILE" | cut -f1) - $(ffprobe -v quiet -show_entries format=duration -of csv=p=0 "$MP3_FILE" 2>/dev/null || echo 'N/A')s"
echo ""

if [ "$SKIP_IMAGES" != "1" ]; then
    echo "==> Generating waveforms (sanity check only -- see note above)..."
    ffmpeg -y -i "$WAV_FILE" -t 5 -filter_complex "showwavespic=s=1200x300:colors=#d4944c" -frames:v 1 "comparison_original.png" 2>/dev/null
    ffmpeg -y -i "$MP3_FILE" -t 5 -filter_complex "showwavespic=s=1200x300:colors=#4cd494" -frames:v 1 "comparison_encoded.png" 2>/dev/null
    echo "   Saved: comparison_original.png, comparison_encoded.png"

    echo "==> Generating spectrograms (sanity check only -- see note above)..."
    ffmpeg -y -i "$WAV_FILE" -t 5 -filter_complex "showspectrumpic=s=1200x300:mode=combined:color=intensity" -frames:v 1 "spectrogram_original.png" 2>/dev/null
    ffmpeg -y -i "$MP3_FILE" -t 5 -filter_complex "showspectrumpic=s=1200x300:mode=combined:color=intensity" -frames:v 1 "spectrogram_encoded.png" 2>/dev/null
    echo "   Saved: spectrogram_original.png, spectrogram_encoded.png"
    echo ""
fi

# === LEVEL STATS ===
# astats' own end-of-stream "Overall" block (no metadata=1/ametadata
# dance -- that was the previous script's bug: it printed the same
# per-reset-window line 5 times and never captured the real summary).
echo "=== LEVEL STATS ==="
echo "Original WAV:"
ffmpeg -hide_banner -i "$WAV_FILE" -af astats -f null - 2>&1 | grep -E "Peak level|RMS level|Flat factor|Noise floor|Dynamic range" || echo "  (astats unavailable)"
echo ""
echo "Encoded MP3:"
ffmpeg -hide_banner -i "$MP3_FILE" -af astats -f null - 2>&1 | grep -E "Peak level|RMS level|Flat factor|Noise floor|Dynamic range" || echo "  (astats unavailable)"
echo ""

# === LOUDNESS COMPARISON ===
# Grab the whole Summary block (-A 10), not just the header line the
# previous script grepped for -- the actual I:/LRA:/Peak: values are on
# the lines *after* "Integrated loudness:", so the old grep pattern
# always printed a blank result.
echo "=== LOUDNESS COMPARISON ==="
echo "Original WAV:"
ffmpeg -hide_banner -i "$WAV_FILE" -af ebur128 -f null - 2>&1 | grep -A 10 "Summary:" || echo "  (ebur128 unavailable)"
echo ""
echo "Encoded MP3:"
ffmpeg -hide_banner -i "$MP3_FILE" -af ebur128 -f null - 2>&1 | grep -A 10 "Summary:" || echo "  (ebur128 unavailable)"
echo ""

# === FIDELITY: lag-aligned correlation + per-band distortion profile ===
echo "=== FIDELITY (lag-aligned correlation + per-band profile) ==="
if ! command -v python3 >/dev/null 2>&1 || ! python3 -c "import numpy" >/dev/null 2>&1; then
    echo "  SKIPPED: python3 + numpy required for this section (pip install numpy)."
else
    ORIG_RATE=$(ffprobe -v quiet -show_entries stream=sample_rate -of csv=p=0 "$WAV_FILE" 2>/dev/null | head -1)
    ORIG_RATE="${ORIG_RATE:-44100}"

    # Normalize both sides to the same canonical format (mono, 16-bit
    # PCM, original's own sample rate) so the comparison is apples-to-
    # apples and readable by Python's stdlib `wave` module with no extra
    # dependencies. This also decodes the MP3 back to PCM, which is the
    # actual signal a listener hears.
    ffmpeg -y -hide_banner -loglevel error -i "$WAV_FILE" -ac 1 -ar "$ORIG_RATE" -c:a pcm_s16le "$TMPDIR/orig_mono.wav"
    ffmpeg -y -hide_banner -loglevel error -i "$MP3_FILE" -ac 1 -ar "$ORIG_RATE" -c:a pcm_s16le "$TMPDIR/dec_mono.wav"

    python3 "$(dirname "$0")/scripts/audio_fidelity.py" \
        "$TMPDIR/orig_mono.wav" "$TMPDIR/dec_mono.wav" \
        "$SEG_SECONDS" "$MAX_LAG" "$N_SEGMENTS" "$DECAY_RADIUS" \
        || echo "  (fidelity scan failed -- see error above)"
fi
echo ""

echo "==> Analysis complete!"
echo ""
if [ "$SKIP_IMAGES" != "1" ]; then
    echo "PNG files (visual sanity check only, not a fidelity measurement):"
    echo "  - comparison_original.png vs comparison_encoded.png (waveforms)"
    echo "  - spectrogram_original.png vs spectrogram_encoded.png (spectrograms)"
    echo ""
fi
echo "The FIDELITY section above is the number to trust while tuning:"
echo "  - peak correlation > 0.8 at a sharp, symmetric lag  -> solid/reference-quality"
echo "  - 0.5 - 0.8                                          -> audible but ordinary lossy loss"
echo "  - < 0.5, or no single dominant lag (flat/scattered)  -> severe/structural distortion"
echo "  - the 'weakest band' line tells you which frequency range to look at first."
