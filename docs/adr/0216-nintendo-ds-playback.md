# ADR 0216 — Nintendo DS playback

Status: Accepted. The user explicitly selected melonDS DS in-process after the
per-core hosting review required by ADR 0198.

Bundle melonDS DS 1.4.0 through libretro, on the existing single emulation worker.
A core crash can terminate Horizon; a helper process would isolate crashes but
requires another lifecycle and transport boundary. This decision approves this
core only, not arbitrary downloadable cores or a general process framework.

Raw `.nds` cartridges use the same automatic folder importer, full-file SHA-1
plus size catalog recognition, SHA-256 identity and launch-time verification.
Allow DS-enhanced cartridges in DS mode; reject DSi-exclusive headers. Cap DS
files at 512 MiB, retaining the existing 32 MiB cap for GB/GBC/GBA.

Use built-in BIOS/firmware replacements, direct boot and software rendering
(GPU upscaling was added later: [ADR 0218](0218-ds-gpu-upscaling-and-dev-speed.md)).
The initial layout stacks two 256×192 screens with no gap. Mouse touch maps to
the fitted composite image; the right stick moves the core cursor and R2 touches.
Four DS face buttons use fixed device-family assignments with the global A/B
swap. Existing mGBA remapping remains mGBA-specific. No DSi, firmware manager,
network play, microphone recording, virtual SD card or slot-2 cartridge support
is exposed. DS layout, filter and remapping
preferences are in [ADR 0217](0217-ds-screen-layouts-and-mapping.md).

Keep game-named cartridge saves in the existing per-game folder. DS states live
under `states/melonds-ds`, separate from `states/mgba`, and retain the existing
four manual slots, automatic resume, JXL previews and compatibility validation.
Unsupported state serialization must not prevent an otherwise successful exit.

Pin all core/dependency source archives and retain upstream notices. No new
runtime permission or Rust dependency is required. Validate actual x86_64 and
aarch64 builds separately; an SDK installation is not architecture verification.
