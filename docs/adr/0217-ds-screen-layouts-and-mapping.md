# ADR 0217 — DS screen layouts, swapping and button mapping

Status: Accepted. The user asked for side-by-side, stacked and large-and-small
DS layouts, a quick-menu swap, R3 to swap with animation, DS filters and DS
button mapping.

Horizon composes the two DS screens itself rather than asking melonDS DS for a
layout. The core keeps its stacked 256×384 output, so touch coordinates, states
and screenshots are unchanged. The player positions two images, which lets the
swap slide (Reduced Motion: instant) and lets each screen be filtered at the
exact size it is shown. Side by side is the default.

Layout and video filter are melonDS DS preferences in `cores/melonds-ds.json`,
a new file owned only by that core; older installs get the defaults. The swap
is session state, not a preference: it resets each launch and never races a
Settings write. Swap screens is a DS-only quick-menu action; during a DS game
R3 swaps without pausing and L3 alone opens the quick menu.

DS gets its own eight-action controller mapping (A, B, X, Y, L, R, Start,
Select) in `controller.json`, beside the unchanged Game Boy mapping. Old files
keep the defaults. Both follow the global A/B swap.

Core log output uses the libretro log interface through a small C shim
(variadic callbacks can't be written in Rust) into Horizon's tracing log. No new
dependency or permission is required.
