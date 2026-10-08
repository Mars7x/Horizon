# ADR 0129 — Signal-driven network status and native vector symbols

Status: Accepted for Phase 9.5.44.74 (pending runtime validation).

## Context

The NetworkManager worker polled once every five seconds. The top-bar
GNOME symbols were SVG-backed Slint Images rasterized at low logical scale
and subsequently enlarged with Horizon's fullscreen scene; this visibly
softened the Wi-Fi and Ethernet artwork.

## Decision

Listen for NetworkManager system D-Bus signals on a dedicated blocking
connection and send coalesced wake notifications to the existing network
status worker. Re-query NetworkManager on notification, while continuing
five-second fallback polling for missed signals and connection recovery.
Confirm an observed link loss with a second snapshot 150 ms later, avoiding
spurious 'offline' frames on normal connection transitions. Reduce the
non-blocking Slint status receiver timer to 50 ms. Power observation remains
independent; no UI-thread D-Bus access or added Flatpak permissions.

Render the original GNOME path commands through Slint Path elements at the
actual scene scale. Preserve stroke caps, joins and original stroke/fill
opacities for all eight status glyphs. Keep the original SVGs byte-for-byte
for provenance. The generated Path module is an adaptation of GNOME's
original shapes and is covered by the same asset notices/licensing.

## Constraints

Signal delivery is prompt, but system bus scheduling is not hard real time.
Don't interpret a single failed bus query as a physical disconnect.
No changed network/battery priority policies, manual scripts, or new access.
Do not replace the GNOME art with unrelated icon geometry.
