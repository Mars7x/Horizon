# ADR 0115 — Replace bundled navigation cue (Phase 9.5.44.58)

## Decision

Use the project-owner-supplied `ui/assets/horizon-navigation.wav` as Horizon's
single navigation feedback sample, replacing the earlier
`ui/assets/horizon-navigation-soft.wav`. Embed the replacement WAV without
transcoding, gain adjustment, or other signal processing.

The WAV remains 48 kHz, mono, signed 16-bit PCM with 4,080 samples (85 ms),
so the existing SDL3 streaming decoder needs no format or timing changes. The
new file has lower peak and RMS amplitude than the original. No playback gain
is added in Horizon; the user's quieter version is intentional.

`NavigationSound` continues to play only for real semantic focus changes
caused by keyboard/controller direction, clears pending audio during rapid
navigation, and obeys Settings → Appearance → UI Sounds, default-on and stored
alongside existing appearance preferences. No changes to the settings UI,
input ownership, Steam/Heroic logic, Flatpak permissions, or dependencies.

## Supersession and distribution

This replaces only the *audio asset* decision in ADR 0113; its SDL3 playback
and preference architecture remains in effect. The retired audio file must be
deleted from the project instead of shipping both versions.

This WAV was provided by the project owner. Redistribution rights have not
been independently confirmed and must be reviewed before release; see
`THIRD_PARTY_NOTICES.md`.
