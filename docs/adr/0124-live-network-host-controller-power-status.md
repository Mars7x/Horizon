# ADR 0124 — Live network and battery status

## Context

Horizon 9.5.44.66 drew an always-full Wi-Fi glyph and an always-present,
always-full battery on the shell top bar. This was misleading on desktops and
systems without Wi-Fi or batteries.

## Decision

- Use the system NetworkManager D-Bus API to observe active physical links,
  Ethernet preference, Wi-Fi access point strength, and limited connectivity.
- Use UPower's system display battery, not enumerated peripheral batteries, for
  laptop and handheld host power. If absent, use SDL3's already-open gamepad
  power report. Host battery is the only indicator when both are present.
- Hide battery completely if neither host nor gamepad has a reliable numeric
  percentage. Never show full by default on an unknown battery.
- Poll system services on a 5-second dedicated worker; SDL battery no more than
  every 10 seconds (plus hotplug). Publish compact immutable values to Slint.
- Render 3 Wi-Fi signal bands, a wired connector, an offline mark, discrete
  battery fill and charging bolt. Keep the original top-right placement but
  collapse empty battery spacing.
- Add just two system-bus Flatpak names. No unrestricted host filesystem,
  process scanning, new crate dependency, privileged helper, or direct UI D-Bus.

## Limitations

NetworkManager's connectivity assessment does not prove that every destination
is reachable. Unsupported adapters or absent D-Bus services display offline.
SDL3 gamepad power is optional hardware metadata; many controllers cannot
report percentages. Power readings are estimates; unknown is not the same as
0 percent. The visual updates are polled, not instant change signals.
