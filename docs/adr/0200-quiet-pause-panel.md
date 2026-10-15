# ADR 0200 — Quiet pause panel and first game actions

Status: Accepted. The user chose the quiet side panel, deferred save states and
explicitly requested working 2× speed, restart and screenshots.

Use a dimmed frozen game and a compact side panel built with shared focus,
glyph, theme and motion components. Shared compact ListRows provide action
geometry and marquee; Theme.background respects Black, hints share the shell
footer baseline, and actions use shared UI sounds. Speed changes have no status
text when successful. Rust owns selection and confirmation state.
Continue is first; save/load-state placeholders remain disabled and are skipped
by navigation. They create no files and imply no save-state support.

Speed is session-local: run the core at twice its reported frame pace for 2×,
play audio twice as fast at a higher pitch, and clear buffered samples on change.
A streaming stereo low-pass filter precedes 2:1 decimation to prevent aliasing;
the application queue retains at most 50 ms of recent samples.
Show the effective speed in the panel and a small 2× indicator while playing.
Speed changes do not introduce a permanent core configuration API.

Restart requires confirmation with Cancel initially focused. Save battery RAM
before the core reset; retain/restore battery memory and resume under the
background policy in [ADR 0202](0202-background-freeze-and-return-blend.md). Storage failures leave a paused core for retry. A reset stays
within the existing SourceRuntime session; observed wall time is never multiplied.

Screenshots copy the native, unfiltered core frame. Encode PNG on the worker and
publish complete files without replacing an existing capture, using HorizonAlbum's
shared source/hash path allocation and partial-file convention. UI and speed
indicators are excluded. No external capture source, host API or new permission
is introduced. Album discovers captures on its next visit.

Returning to Horizon retains the existing save barrier and card return animation.
No core/process architecture, BIOS management, dependency or migration is added.
