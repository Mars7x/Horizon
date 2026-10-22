# ADR 0225 — DS fast-forward pacing and compact toggle

Status: Accepted. The user reported Mario 64 DS remaining at 30 FPS with 2×
selected and requested a smaller, aligned quick-menu switch.

DS fast-forward must retain the selected emulation frame target. Adaptive audio
can emit more samples per frame when the core/rewind work temporarily runs slow;
feeding that larger sample count back into the audio-clocked pacer can turn a
2× request toward a 1× deadline. Cap DS fast-forward's interval at the selected
frame period while retaining bounded audio resampling and queue handling. Reset
the work estimate on speed changes. Normal-speed DS and both mGBA speeds retain
the existing audio-clocked pacing, needed for Game Boy's variable audio blocks.

The selected rendering resolution remains unchanged. None/Smooth DS presentation
copies the original stacked buffer once, retaining per-screen bounds and exact
pixels rather than allocating per-screen copies and reassembling them. This
reduces high-resolution memory traffic without changing core APIs or filters.

SettingsRow provides an explicit compact switch variant for the quick menu:
40×22 track, 3 px inset, vertically centered, with the normal trailing row inset.
The whole row remains the focus/click target, accessibility state is retained,
and the animation follows shared Motion/Reduced Motion. Normal Settings rows
keep their existing switch dimensions.

Regression tests cover DS's doubled deadline under adaptive audio, return to
normal pacing, Game Boy block timing, audio queue behavior, and None/Smooth
pixels/screen bounds at Native through 8×. These establish scheduling and pixel
contracts, not measured Mario 64 DS FPS: core/GPU throughput, display refresh and
the game's own rendering rate still govern frames actually shown. Real packaged
runtime and each supported architecture require their own verification.
