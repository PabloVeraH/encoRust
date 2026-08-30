#!/usr/bin/env python3
"""Lag-aligned correlation + per-band distortion profile between an
original WAV and a decoded WAV (originally an MP3), both already
normalized by the caller (compare_audio.sh) to mono / 16-bit PCM / the
same sample rate.

Methodology follows docs/investigation-log.md §11's validated approach: raw
time-domain Pearson correlation, scanned over a wide lag window, no
windowing or spectral tricks for the headline number. Reference point
from that investigation: ffmpeg/libmp3lame on comparable material scores
~0.99+ at the correct (encoder+decoder algorithmic delay) lag, decaying
sharply and symmetrically around it.

Usage:
    audio_fidelity.py <orig.wav> <decoded.wav> <seg_seconds> <max_lag> \
        <n_segments> <decay_radius>
"""
import sys

import numpy as np
import wave


def load_mono(path):
    with wave.open(path, "rb") as w:
        n, sr, sw, ch = (
            w.getnframes(),
            w.getframerate(),
            w.getsampwidth(),
            w.getnchannels(),
        )
        raw = w.readframes(n)
    if sw != 2:
        raise SystemExit(f"error: expected 16-bit PCM WAV, got {sw * 8}-bit ({path})")
    data = np.frombuffer(raw, dtype=np.int16).astype(np.float64)
    if ch > 1:
        data = data.reshape(-1, ch).mean(axis=1)
    return data, sr


def pearson(a, b):
    a_c = a - a.mean()
    b_c = b - b.mean()
    denom = np.linalg.norm(a_c) * np.linalg.norm(b_c)
    if denom < 1e-9:
        return 0.0
    return float(np.dot(a_c, b_c) / denom)


def scan_lags(o_seg, dec, seg_start, max_lag):
    """o_seg is a fixed original-signal window; slide it against `dec`
    over [-max_lag, +max_lag] and return every (lag, correlation) pair
    found, sorted best-first."""
    length = len(o_seg)
    o_c = o_seg - o_seg.mean()
    o_norm = np.linalg.norm(o_c)
    results = []
    for lag in range(-max_lag, max_lag + 1):
        start = seg_start + lag
        if start < 0 or start + length > len(dec):
            continue
        d_seg = dec[start : start + length]
        d_c = d_seg - d_seg.mean()
        denom = o_norm * np.linalg.norm(d_c)
        corr = float(np.dot(o_c, d_c) / denom) if denom > 1e-9 else 0.0
        results.append((lag, corr))
    results.sort(key=lambda x: -x[1])
    return results


def band_profile(o_seg, d_seg, sr, bands):
    """Rectangular FFT band-pass both segments identically, then report
    per-band Pearson correlation and energy ratio (dB, decoded/original).
    This is what actually answers "where does the remaining distortion
    concentrate" -- a plain broadband correlation number can't localize
    it to a frequency range."""
    n = len(o_seg)
    freqs = np.fft.rfftfreq(n, d=1.0 / sr)
    orig_spec = np.fft.rfft(o_seg)
    dec_spec = np.fft.rfft(d_seg)
    out = []
    for lo, hi, label in bands:
        mask = (freqs >= lo) & (freqs < hi)
        if not mask.any():
            continue
        o_band = np.fft.irfft(np.where(mask, orig_spec, 0), n=n)
        d_band = np.fft.irfft(np.where(mask, dec_spec, 0), n=n)
        o_energy = float(np.sum(o_band**2)) + 1e-12
        d_energy = float(np.sum(d_band**2)) + 1e-12
        ratio_db = 10 * np.log10(d_energy / o_energy)
        out.append((label, pearson(o_band, d_band), ratio_db))
    return out


def main():
    orig_path, dec_path = sys.argv[1], sys.argv[2]
    seg_seconds = float(sys.argv[3])
    max_lag = int(sys.argv[4])
    n_segments = int(sys.argv[5])
    decay_radius = int(sys.argv[6])

    orig, sr = load_mono(orig_path)
    dec, sr2 = load_mono(dec_path)
    if sr != sr2:
        print(f"warning: sample rate mismatch orig={sr} dec={sr2}, results may be meaningless")

    duration = len(orig) / sr
    seg_len = int(seg_seconds * sr)
    if duration < 2 * seg_seconds:
        print("error: file too short for the requested segment size")
        sys.exit(1)

    # Evenly spaced segment starts, staying clear of the first/last 5%:
    # encoder priming (bit reservoir fill, MDCT overlap history) and the
    # final flush aren't representative of steady-state fidelity.
    margin = duration * 0.05
    usable = duration - 2 * margin
    starts_sec = [margin + usable * (i + 1) / (n_segments + 1) for i in range(n_segments)]

    bands = [
        (0, 1000, "0-1kHz"),
        (1000, 2000, "1-2kHz"),
        (2000, 4000, "2-4kHz"),
        (4000, 8000, "4-8kHz"),
        (8000, min(16000, sr / 2), "8-16kHz"),
    ]

    print(f"{'segment(s)':>11}  {'lag(smp)':>9}  {'lag(ms)':>8}  {'peak_corr':>9}  decay shape (±{decay_radius} samples around peak)")
    all_peaks = []
    all_band_corrs = {label: [] for _, _, label in bands}

    for start_sec in starts_sec:
        seg_start = int(start_sec * sr)
        o_seg = orig[seg_start : seg_start + seg_len]
        if len(o_seg) < seg_len:
            continue
        results = scan_lags(o_seg, dec, seg_start, max_lag)
        if not results:
            print(f"{start_sec:11.2f}  (no valid lag in range -- file too short near this point)")
            continue
        best_lag_val, best_corr = results[0]
        all_peaks.append(best_corr)

        # Shape around the peak: a real, well-aligned encoder delay shows
        # a sharp, symmetric falloff; a structurally broken encoder shows
        # a flat/scattered profile with no single dominant lag (see
        # docs/investigation-log.md §11's "before" vs "after" numbers).
        by_lag = dict(results)
        decay = [
            f"{by_lag.get(best_lag_val + d, float('nan')):.2f}"
            for d in range(-decay_radius, decay_radius + 1)
        ]
        lag_ms = 1000 * best_lag_val / sr
        print(f"{start_sec:11.2f}  {best_lag_val:9d}  {lag_ms:8.2f}  {best_corr:9.4f}  {' '.join(decay)}")

        # Per-band profile at this segment's own best-aligned pair.
        d_seg = dec[seg_start + best_lag_val : seg_start + best_lag_val + seg_len]
        for label, corr, _ratio_db in band_profile(o_seg, d_seg, sr, bands):
            all_band_corrs[label].append(corr)

    if not all_peaks:
        print("No usable segments -- file too short, or paths wrong.")
        sys.exit(1)

    print()
    print(
        f"Overall peak correlation: min={min(all_peaks):.4f} "
        f"max={max(all_peaks):.4f} avg={sum(all_peaks) / len(all_peaks):.4f}"
    )
    print(
        "(reference: libmp3lame on comparable material scores ~0.99+ at the "
        "correct lag; >0.8 solid, 0.5-0.8 audible-but-ordinary, <0.5 severe distortion)"
    )

    print()
    print("Per-band correlation (averaged across segments, each at its own best lag):")
    band_avgs = {}
    for _lo, _hi, label in bands:
        vals = all_band_corrs[label]
        if vals:
            band_avgs[label] = sum(vals) / len(vals)
            print(f"  {label:>8}: {band_avgs[label]:.4f}")

    if band_avgs:
        worst = min(band_avgs, key=band_avgs.get)
        print(
            f"\n  => weakest band: {worst} (corr={band_avgs[worst]:.4f}) -- "
            "distortion concentrates here; look at this subband range first."
        )


if __name__ == "__main__":
    main()
