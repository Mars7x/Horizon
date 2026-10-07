# ADR 0025: Bundle LINE Seed JP as Horizon's application typeface

## Status
Accepted. Supersedes the active typography decisions in ADR 0010, ADR 0013, ADR 0023, and ADR 0024.

## Context
UD Shin Go NT matched the desired visual direction but is a commercial Morisawa typeface and Horizon could not safely redistribute it. Relying on a host-installed copy also made Flatpak rendering non-deterministic. LINE Seed JP provides a similarly soft, rounded Gothic UI character and is released by LY Corporation under the SIL Open Font License 1.1.

## Decision
Horizon uses `LINE Seed JP` as its application-wide font family. The Flatpak builds upstream revision `0564884c67ebdb3b31e90e2106c2d061f953b6a0` from `https://github.com/line/seed` and installs the resulting Thin, Regular, Bold, and ExtraBold TTF files into `/app/share/fonts/truetype/line-seed-jp`.

Weight mapping:

- general UI: Regular 400
- game titles and emphasis: Bold 700
- clock digits: ExtraBold 800
- clock suffix: Bold 700

The font files are not modified or renamed. Horizon retains the upstream copyright notice and OFL-1.1 license.

## Consequences

- Packaged Flatpak builds no longer depend on host fonts.
- Typography is deterministic inside the sandbox.
- Development builds performed directly on the host still need LINE Seed JP installed locally; the Flatpak/Builder workflow is the canonical development target.
- The font build adds an upstream Python/fontmake build step. This is acceptable during the current network-enabled development packaging stage and must be converted to reproducible offline sources before a Flathub release.
