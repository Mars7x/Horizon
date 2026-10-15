# ADR 0121 — Replace the Back cue without changing playback semantics

Status: prepared for Phase 9.5.44.64; awaiting Cargo and audible runtime validation.

## Context

The owner supplied a revised `horizon-back(2).wav` with unwanted delay trimmed. Its
length is 6,306 samples (131.375 ms) at 48 kHz, down from 9,600 samples
(200 ms). Peak sample amplitude is unchanged.

## Decision

- Replace only the bundled `ui/assets/horizon-back.wav` with the supplied file,
  byte for byte. Do not normalize, resample, amplify, fade, or otherwise modify it.
- Update `src/audio.rs` bundled-audio assertions from 9,600 to 6,306 frames.
- Keep existing SDL3 playback, early device preparation, audio flush, UI Sounds
  toggle, cue dispatch semantics, and navigation/OK WAVs unchanged.
- Record the asset replacement in `THIRD_PARTY_NOTICES.md` and confirm
  redistribution rights before public release.

## Validation

The new file is PCM16, mono, 48 kHz and passes the production WAV decoder's
format and non-empty-data requirements. Integration compile/audio timing needs
verification in a real Horizon build; shortening a WAV does not independently
prove that SDL3/backend output latency has been eliminated.
