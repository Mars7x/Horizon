#!/usr/bin/env python3
"""Install pinned prebuilt LINE Seed JP fonts for Horizon's Flatpak build.

Horizon intentionally does not build LINE Seed JP from source in the Freedesktop
26.08 SDK: the upstream font-build Python dependency stack currently fails on
Python 3.14. Instead, development builds download the four canonical static TTF
files published in Google Fonts at one immutable commit.

The URLs are commit-pinned and the payload is validated as a real SFNT font
before installation. For a release/Flathub manifest, move these four downloads
into flatpak-builder sources with SHA-256 checksums so downloads are cached and
verified before the module runs.
"""

from __future__ import annotations

import argparse
import struct
import sys
import urllib.request
from pathlib import Path

GOOGLE_FONTS_REVISION = "874ec71eac706dd23900d1305449abed6767b7df"
FONT_BASE_URL = (
    "https://raw.githubusercontent.com/google/fonts/"
    f"{GOOGLE_FONTS_REVISION}/ofl/lineseedjp"
)

FONT_FILES = (
    "LINESeedJP-Thin.ttf",
    "LINESeedJP-Regular.ttf",
    "LINESeedJP-Bold.ttf",
    "LINESeedJP-ExtraBold.ttf",
)

# The canonical Google Fonts files are roughly 3.5-3.7 MiB each. A much
# smaller payload is almost certainly a subset, error page, or truncated file.
MIN_FONT_BYTES = 2_000_000
SFNT_SIGNATURES = {b"\x00\x01\x00\x00", b"OTTO", b"true", b"typ1"}


def _download(url: str) -> bytes:
    request = urllib.request.Request(
        url,
        headers={"User-Agent": "Horizon Flatpak development build"},
    )
    with urllib.request.urlopen(request, timeout=180) as response:
        payload = response.read()

    if len(payload) < MIN_FONT_BYTES:
        raise RuntimeError(
            f"font payload from {url} is unexpectedly small ({len(payload)} bytes)"
        )
    if payload[:4] not in SFNT_SIGNATURES:
        raise RuntimeError(
            f"font payload from {url} has invalid SFNT signature {payload[:4]!r}"
        )

    # Minimal TTF/OTF table-directory sanity check. This catches HTML/error
    # payloads even when a proxy returns HTTP 200 and guards truncated fonts.
    if len(payload) < 12:
        raise RuntimeError(f"font payload from {url} is truncated")
    num_tables = struct.unpack(">H", payload[4:6])[0]
    directory_end = 12 + num_tables * 16
    if num_tables == 0 or directory_end > len(payload):
        raise RuntimeError(f"font payload from {url} has an invalid table directory")

    return payload


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--prefix", default="/app", help="Flatpak install prefix")
    args = parser.parse_args()

    target = Path(args.prefix) / "share/fonts/truetype/line-seed-jp"
    target.mkdir(parents=True, exist_ok=True)

    for filename in FONT_FILES:
        url = f"{FONT_BASE_URL}/{filename}"
        print(f"Downloading pinned {filename}…", flush=True)
        payload = _download(url)
        destination = target / filename
        with destination.open("wb") as output:
            output.write(payload)
        print(f"Installed {filename} ({len(payload)} bytes)")

    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as exc:
        print(f"LINE Seed JP installation failed: {exc}", file=sys.stderr)
        raise
