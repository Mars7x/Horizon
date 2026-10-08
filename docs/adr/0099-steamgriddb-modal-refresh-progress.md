# ADR 0099 — Modal artwork refresh progress

## Context
A manual SteamGridDB refresh previously showed an inline message below the settings rows. That message did not report a completed/total count, and it remained after refresh. The user requests a popup with spinner and progress instead.

## Decision
- Remove the inline Third-Party status text completely, including the old "Refreshing SteamGridDB artwork…" feedback assignment.
- Open a centered, dimmed modal on manual Refresh. Show a rotating spinner and `completed / total games checked`, plus a determinate progress bar. Keep the previous SteamGridDB preference and key rows unchanged.
- Emit structured `RefreshProgress` events from the existing off-thread worker rather than parsing diagnostic log strings. A job counts as checked even when its lookup is unsuccessful; the total is the number of eligible artwork jobs, not necessarily the entire library.
- A lifetime progress guard sends a terminal event on normal completion and all early returns (authentication, network, metadata or CDN). It discards progress if the generation is obsolete. Thread start failure is handled separately.
- Replace the spinner with a completion/error indicator and a Done action after the refresh. The user may Hide the popup earlier; downloads continue. Back dismisses the modal and cannot navigate Settings beneath it, Home may still navigate normally. Focus stays with the popup while open.
- Preserve artwork cache semantics and game cover fallback. Normal startup/background lookup does not open the modal.
- A motion-sensitive constant-speed spinner is shown only while running; Reduced Motion keeps it stationary.

## Consequences
The modal is manual only and contains real per-job progress. Closing it does not cancel the background worker. The Third-Party page has no lingering refresh status text.
