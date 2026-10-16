#!/usr/bin/env python3
"""Generate Horizon's original paired quick-menu cues (stdlib only).

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
    duration = 0.160 if opening else 0.140
    # A soft, continuous shuffle with broad, overlapping brush strokes.
    # Seeded noise keeps the texture reproducible, without recordings or tones.
    noise = random.Random(0x485A01 if opening else 0x485A02)
    grains = []
    for index in range(10):
        center = (index + noise.random() * 0.7) / 10 * duration
        width = 0.006 + noise.random() * 0.005
        grains.append((center, width, 0.15 + noise.random() * 0.25))
    samples = []
    low = 0.0
    body = 0.0
    softened = 0.0
    for frame in range(round(duration * RATE)):
        time = frame / RATE
        position = time / duration
        # Two low-pass stages soften the scratchy high end. Opening brightens
        # gently; closing brushes downward, without a pitched sweep.
        cutoff = 1100 + 500 * (position if opening else 1 - position)
        raw = noise.random() * 2 - 1
        smoothing = 1 - math.exp(-2 * math.pi * cutoff / RATE)
        body += smoothing * (raw - body)
        softened += smoothing * (body - softened)
        low += (1 - math.exp(-2 * math.pi * 180 / RATE)) * (softened - low)
        texture = 0.7 + sum(
            gain * math.exp(-0.5 * ((time - center) / width) ** 2)
            for center, width, gain in grains
        )
        attack = math.sin(min(1.0, time / 0.022) * math.pi / 2) ** 2
        release = math.sin(min(1.0, (duration - time) / 0.055) * math.pi / 2) ** 2
        weight = math.exp(-position * (1.8 if opening else 2.3))
        samples.append((softened - low) * texture * attack * release * weight)
    # Keep the pair quiet alongside the existing owner-supplied menu cues. Their
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
    for name, opening in [("open", True), ("close", False)]:
        path = ROOT / f"ui/assets/horizon-quick-menu-{name}.wav"
        data = cue(opening)
        if args.check:
            if not path.is_file() or path.read_bytes() != data:
                raise SystemExit(f"Quick-menu sound differs from its source: {path.name}")
        else:
            path.write_bytes(data)


if __name__ == "__main__":
    main()
