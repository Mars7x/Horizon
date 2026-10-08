# ADR 0120 — Preserve Home focus and prepare UI audio before interaction

Status: proposed for Phase 9.5.44.63; requires Slint build, focus visual check,
first-cue listening test, and controller/keyboard tests on the target system.

## Problem

Phase 9.5.44.62 replaced the established Home game-card brackets with an
alpha-only colour pulse. The user specifically wanted the **existing Home
animation left alone** and the other menus to copy its brightness cadence.
Furthermore, Settings rows and Appearance choices used direct static
`Theme.focus` brushes; `animate border-color` can continuously restart its
tween when an idle clock drives the value, visually hiding the colour cycle.

The SDL3 sound player lazily initialized the audio device on the first cue,
including the first navigation press. The command stream cleared pending data
before every cue but did not flush at the end of short PCM effects. Some OK/Back
cues were dispatched after synchronous screen updates.

## Decision

1. Restore the **unmodified** original Phase 9.5.44.38 `FocusFrame` source,
   including moving vector brackets and Gaussian emissions. Keep the later
   3200 ms colour cycle. Do not substitute menu border geometry.
2. Derive a single menu-focused accent-to-highlight brush using the same
   cosine cadence and highlight strength as that existing Home frame.
   The Settings root and Appearance's direct borders bind to that live brush;
   their focus *strength*, not the dynamically changing colour, eases in and out.
   Other menu and utility rings use the same shared brush.
3. Keep the utility icon and transparent ring rising together by 2 px and
   preserve all fresh-press-only wrapping and navigation semantics.
4. Open the existing SDL audio stream once during early Slint idle. Submit
   10 ms of silence before resuming the device to warm the backend; keep the
   original WAV sample arrays exactly untouched. Explicitly flush each queued
   short cue, retaining interruption/queue-clearing semantics.
5. Deliver known-successful OK/Back cues before expensive route publishing
   while preserving failure/ignored-action silence. No per-action device open,
   volume boosts, extra audio dependencies, or new permissions.

## Limits and verification

The first-action problem is mitigated, not guaranteed eliminated: the actual
PipeWire device, Bluetooth output, backend wake-up behaviour, and unusually
rapid input before the idle preparer runs can add latency. UI Sounds Off never
opens the device in the preparer. Error retry remains bounded and nonfatal.

Run Cargo check/test; check Home focus matches pre-9.5.44.62 visually, verify
continuous *visible* colour breathing on every non-Home focus, verify 2 px
utility lift, test the first navigation/OK/Back cue after launch, fast repeats,
toggle Off/On, and controller/keyboard/mouse action parity. The target stack
must be tested because this patch was produced without a local Cargo/Slint
compiler or physical audio backend.
