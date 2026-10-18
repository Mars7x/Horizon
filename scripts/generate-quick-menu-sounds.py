#!/usr/bin/env python3
"""Generate Horizon's original UI cues: navigation, quick menu, launch and
screenshot, OK and Back (stdlib only).

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
    """A quick, clean shutter: one tight click and a tiny second-curtain echo."""
    noise = random.Random(44)
    body = [0.0, 0.0]
    clicks = [(0.0, 1.0, 3800), (0.022, 0.45, 3500)]
    samples = []
    for frame in range(round(0.06 * RATE)):
        time = frame / RATE
        raw = noise.random() * 2 - 1
        body[0] += 0.65 * (raw - body[0])
        body[1] += 0.65 * (body[0] - body[1])
        value = 0.0
        for start, gain, tone in clicks:
            since = time - start
            if since >= 0:
                value += gain * (0.5 * body[1] * math.exp(-since / 0.001)
                                 + math.sin(2 * math.pi * tone * since) * math.exp(-since / 0.003))
        samples.append(value)
    # Fade the last 6 ms so the cue ends at silence.
    fade = int(0.006 * RATE)
    for index in range(fade):
        samples[-1 - index] *= index / fade
    return encode_level(samples)


class Noise:
    """Seeded noise through two smoothing stages."""

    def __init__(self, seed: int, smoothing: float) -> None:
        self.random = random.Random(seed)
        self.smoothing = smoothing
        self.first = self.second = 0.0

    def next(self) -> float:
        raw = self.random.random() * 2 - 1
        self.first += self.smoothing * (raw - self.first)
        self.second += self.smoothing * (self.first - self.second)
        return self.second


def navigation() -> bytes:
    """A warm, soft-edged snap. Its ring sits at 2.6 kHz, below the 3-5 kHz band
    ears are most sensitive to, and its first half millisecond is softened, so
    it stays comfortable on every focus move."""
    noise = Noise(65, 0.4)
    samples = []
    for frame in range(round(0.05 * RATE)):
        time = frame / RATE
        texture = noise.next()
        edge = min(1.0, time / 0.0005)
        snap = 0.35 * texture * math.exp(-time / 0.0008) + math.sin(2 * math.pi * 2600 * time) * math.exp(-time / 0.0022)
        body = 0.45 * math.sin(2 * math.pi * 850 * time) * math.exp(-time / 0.006) * min(1.0, time / 0.001)
        samples.append(edge * (snap + body))
    fade = int(0.006 * RATE)
    for index in range(fade):
        samples[-1 - index] *= index / fade
    return encode_level(samples)


def warm_snap(time: float, texture: float) -> float:
    """The navigation snap: a 2.6 kHz ring (below the 3-5 kHz band ears are
    most sensitive to), smoothed noise and a softened first half millisecond."""
    edge = min(1.0, time / 0.0005)
    snap = 0.35 * texture * math.exp(-time / 0.0008) + math.sin(2 * math.pi * 2600 * time) * math.exp(-time / 0.0022)
    body = 0.45 * math.sin(2 * math.pi * 850 * time) * math.exp(-time / 0.006) * min(1.0, time / 0.001)
    return edge * (snap + body)


def note(since: float, tone: float, gain: float, decay: float) -> float:
    if since < 0:
        return 0.0
    rounded = math.sin(min(1.0, since / 0.001) * math.pi / 2) ** 2
    return gain * math.sin(2 * math.pi * tone * since) * rounded * math.exp(-since / decay)


def confirm() -> bytes:
    """OK: the warm snap with a clear A6 note."""
    noise = Noise(121, 0.4)
    samples = []
    for frame in range(round(0.05 * RATE)):
        time = frame / RATE
        samples.append(warm_snap(time, noise.next()) + note(time, 1760.0, 0.9, 0.012))
    fade = int(0.006 * RATE)
    for index in range(fade):
        samples[-1 - index] *= index / fade
    return encode_level(samples)


def back() -> bytes:
    """Back: the warm snap with two quick notes stepping down (E6 to A5), so it
    shares OK's family but moves the other way."""
    noise = Noise(203, 0.4)
    samples = []
    for frame in range(round(0.07 * RATE)):
        time = frame / RATE
        samples.append(warm_snap(time, noise.next()) + note(time, 1318.5, 0.7, 0.008)
                       + note(time - 0.022, 880.0, 0.9, 0.014))
    fade = int(0.006 * RATE)
    for index in range(fade):
        samples[-1 - index] *= index / fade
    return encode_level(samples)


def launch() -> bytes:
    """A crisp onset over a smooth, low "whoom" that swells and settles with
    the card's zoom into the game."""
    noise = Noise(74, 0.75)
    samples = []
    for frame in range(round(0.15 * RATE)):
        time = frame / RATE
        texture = noise.next()
        crisp = 0.8 * (0.35 * texture * math.exp(-time / 0.0008) + math.sin(2 * math.pi * 3600 * time) * math.exp(-time / 0.0025))
        pitch = 220 * (1 + 0.8 * min(1.0, time / 0.09))
        whoom = math.sin(2 * math.pi * pitch * time) * math.sin(math.pi * min(1.0, time / 0.14)) ** 2
        samples.append(crisp + whoom)
    fade = int(0.012 * RATE)
    for index in range(fade):
        samples[-1 - index] *= (index / fade) ** 2
    return encode_level(samples)


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


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="verify bundled bytes without writing")
    args = parser.parse_args()
    for name, data in [("horizon-navigation", navigation()),
                       ("horizon-ok", confirm()),
                       ("horizon-back", back()),
                       ("horizon-launch", launch()),
                       ("horizon-quick-menu-open", cue(True)),
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
