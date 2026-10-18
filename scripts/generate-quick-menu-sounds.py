#!/usr/bin/env python3
"""Generate Horizon's original quick-menu and screenshot cues (stdlib only).

SPDX-FileCopyrightText: Horizon contributors
SPDX-License-Identifier: GPL-3.0-or-later
"""
import argparse
import io
import math
from pathlib import Path
import random
import struct
import wave

RATE = 48_000
ROOT = Path(__file__).resolve().parent.parent


def cue(opening: bool) -> bytes:
    """Two tiny soft clicks, like a small latch: brighter opening, duller closing."""
    noise = random.Random(21 if opening else 22)
    body = [0.0, 0.0]
    clicks = [(0.0, 1.0), (0.045, 0.6)] if opening else [(0.0, 0.6), (0.04, 1.0)]
    tone = 2600 if opening else 1700
    smoothing = 0.35 if opening else 0.25
    samples = []
    for frame in range(round(0.09 * RATE)):
        time = frame / RATE
        raw = noise.random() * 2 - 1
        body[0] += smoothing * (raw - body[0])
        body[1] += smoothing * (body[0] - body[1])
        value = 0.0
        for start, gain in clicks:
            since = time - start
            if since >= 0:
                value += gain * (0.5 * body[1] * math.exp(-since / 0.0025)
                                 + math.sin(2 * math.pi * tone * since) * math.exp(-since / 0.006))
        samples.append(value)
    # Fade the last 8 ms so the cue ends at silence.
    fade = int(0.008 * RATE)
    for index in range(fade):
        samples[-1 - index] *= index / fade
    return encode_level(samples)


def shutter() -> bytes:
    """A quiet two-part shutter brush, softened rather than a sharp click."""
    noise = random.Random(0x485A03)
    samples = []
    body = softened = low = 0.0
    duration = 0.120
    for frame in range(round(duration * RATE)):
        time = frame / RATE
        raw = noise.random() * 2 - 1
        body += 0.19 * (raw - body)
        softened += 0.19 * (body - softened)
        low += 0.025 * (softened - low)
        envelope = math.exp(-0.5 * ((time - 0.025) / 0.009) ** 2)
        envelope += 0.55 * math.exp(-0.5 * ((time - 0.066) / 0.013) ** 2)
        attack = math.sin(min(1.0, time / 0.008) * math.pi / 2) ** 2
        release = math.sin(min(1.0, (duration - time) / 0.025) * math.pi / 2) ** 2
        samples.append((softened - low) * envelope * attack * release)
    return encode(samples)


def encode_level(samples: list[float]) -> bytes:
    """Match the owner-supplied menu cues' loudness (RMS about 0.0095, peak at
    most 0.06), so the latch sits level with navigation, OK and Back."""
    rms = math.sqrt(sum(sample * sample for sample in samples) / len(samples))
    scale = min(0.0095 / rms, 0.06 / max(map(abs, samples)))
    return pcm16([max(-1.0, min(1.0, sample * scale)) * 32767 for sample in samples])


def pcm16(scaled: list[float]) -> bytes:
    pcm = [round(sample) for sample in scaled]
    pcm[0] = pcm[-1] = 0
    output = io.BytesIO()
    with wave.open(output, "wb") as wav:
        wav.setparams((1, 2, RATE, 0, "NONE", "not compressed"))
        wav.writeframes(struct.pack(f"<{len(pcm)}h", *pcm))
    return output.getvalue()


def encode(samples: list[float]) -> bytes:
    # Keep the cues quiet alongside the existing owner-supplied menu cues. Their
    # original bytes stay untouched. Every cue begins/ends at digital silence.
    scale = 0.048 * 32767 / max(map(abs, samples))
    pcm = [round(sample * scale) for sample in samples]
    pcm[0] = pcm[-1] = 0
    output = io.BytesIO()
    with wave.open(output, "wb") as wav:
        wav.setparams((1, 2, RATE, 0, "NONE", "not compressed"))
        wav.writeframes(struct.pack(f"<{len(pcm)}h", *pcm))
    return output.getvalue()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="verify bundled bytes without writing")
    args = parser.parse_args()
    for name, data in [("horizon-quick-menu-open", cue(True)),
                       ("horizon-quick-menu-close", cue(False)),
                       ("horizon-screenshot", shutter())]:
        path = ROOT / f"ui/assets/{name}.wav"
        if args.check:
            if not path.is_file() or path.read_bytes() != data:
                raise SystemExit(f"UI sound differs from its source: {path.name}")
        else:
            path.write_bytes(data)


if __name__ == "__main__":
    main()
