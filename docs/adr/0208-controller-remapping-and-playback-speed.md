# ADR 0208 — Controller remapping and four playback speeds

Status: Accepted. The user requested controller remapping using the established
UI standards and working 3×/4× playback in addition to 1×/2×.

Settings → Controller → Button mapping contains GBA A/B/L/R/Start/Select and a
confirmed Restore defaults action. Use shared headers, breadcrumbs, transitions,
compact ListRows and modal/confirmation components. Named game controls and
controller-input keycaps distinguish the two sides of each mapping. Rust owns selection,
capture, timeouts and page history. Capture ignores the initiating held press,
requires a single eligible button, and cancels on timeout, disconnection,
controller changes or focus loss. Guide/stick-click and Escape remain escape
routes. Physical positions are normalized through the existing SDL family rules.

Mappings are global for GBA and keep each action assigned to a distinct source.
Assigning an occupied source swaps the two assignments. D-pad/stick directions
and shell navigation are fixed. Menu Select/Back swapping continues to exchange
final gameplay A/B, including custom mappings; row values show that effective
assignment. Keyboard gameplay is unchanged. Default restore leaves menu/hint
preferences intact. Extend controller.json compatibly and perform serial atomic
I/O off the UI thread. Invalid files and failed writes preserve prior data.

Session speeds are 1×/2×/3×/4×. Left/Right steps without wrapping at the ends;
Select cycles. Keep the existing animated value, no HUD badge or success message.
Run at the corresponding frame pace and decimate audio through a factor-specific
low-pass filter, preserving stereo and higher-pitched sound. Speed changes clear
obsolete audio; restart and state load retain speed; a new launch starts at 1×.
Observed play time remains wall time. Higher speeds depend on available CPU.

No new core, process abstraction, BIOS policy, dependency or sandbox permission
is introduced. This extends the speed choice in ADR 0200 and controller policy
in ADR 0199; it does not revive retired emulation features.
