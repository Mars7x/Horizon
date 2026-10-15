# ADR 0125 — Stable network status and shared observation cache

## Problem

Phase 9.5.44.67 published new NetworkManager snapshots directly to the UI but
never updated the shared host-status cache read by the controller status callback.
Each controller status update therefore could re-publish the initial default
`Offline` network status, despite a connected Ethernet link.

The worker also collapsed a temporary D-Bus read failure and an incomplete
active-connection enumeration into a synthetic disconnected reading. And it
interpreted NetworkManager `Connectivity=none` as unplugged even when an active
Ethernet/Wi-Fi connection was reported.

## Decision

- Update the shared host-status cache before publishing each new system snapshot.
  Controller status events now use the same latest observation.
- Treat a confirmed *physical* Ethernet/Wi-Fi connection separately from
  NetworkManager's Internet-connectivity assessment. Internet restrictions add
  the limited marker but do not masquerade as an unplugged cable.
- Keep the last known physical link for one confirmed offline poll or two failed
  polling attempts. Two consecutive confirmed offline observations or three
  consecutive failed reads show disconnected, so stale links do not remain
  indefinitely.
- Distinguish failed/incomplete D-Bus property reads (unknown) from a valid
  report containing no active physical connections (disconnected).
- Keep existing 5-second worker cadence, source boundaries, network icons,
  battery precedence, permissions and UI bindings. No UI-thread D-Bus work.

## Limitations

The network indicator follows NetworkManager's local connection state; it is not
an end-to-end Internet reachability test. A confirmed disconnect has up to one
poll interval of debounce, while sustained observer failures expire after three
polls. Rust unit tests cover temporal behavior, but GNOME/Flatpak hardware
behavior must still be validated on a real system.
