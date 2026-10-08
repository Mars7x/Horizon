# ADR 0113 — Optional navigation feedback (Phase 9.5.44.56)

## Decision

Horizon includes the project-owner-supplied 85 ms navigation WAV, without
transcoding, and plays it using the existing SDL3 audio dependency. The
`NavigationSound` adapter (Rust `src/audio.rs`) lazily opens the default device,
queues decoded PCM and drops pending samples on another movement. It never
blocks an input action when audio fails. Audio lifecycle is independent from
Slint and from source launch/session ownership.

The application compares Rust-published Slint semantic focus state before/after
keyboard/gamepad directional actions and only plays on actual focus movement.
This intentionally excludes mouse activation, acceptance, game launches and
other effect types in this initial slice. User preference is a `bool` in the
existing atomically stored appearance JSON; the existing serde default preserves
On for older configs. The Settings page adds a toggle after the accent colours
and extends controller row navigation accordingly.

The Flatpak grants only the existing host PulseAudio-compatible socket for
playback; no extra daemon/process launcher or standalone audio dependency.

## Distribution note

The audio was provided by the project owner. Permission to redistribute
remains to be confirmed before publishing. See `THIRD_PARTY_NOTICES.md`.

## Deferred

Separate Back/Accept/launch/dialog sounds, volume slider, spatial UI feedback,
mouse hover audio and accessibility-specific audio settings.
