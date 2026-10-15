# ADR 0126 — Controller battery via SDL, UPower, and BlueZ

Status: Accepted for Phase 9.5.44.71 (runtime verification pending)

## Context

Some controllers, including the 8BitDo Ultimate 2 Wireless, expose battery
percentage to Linux over Bluetooth, but not through SDL3's gamepad power API.
The 2.4 GHz dongle may report no percentage in Xbox/XInput mode, while SDL
HIDAPI/DInput can report battery data when accessible.

## Decision

Preserve the host-first status policy. When exactly one gamepad is connected and SDL
does not return valid charge, use UPower devices explicitly typed `Gaming Input`
or connected BlueZ gamepads exposing `Battery1`, queried in the existing
system-status worker on the system bus. Multiple eligible reports from a
single source are ambiguous; ignore those instead of assigning a random
peripheral's percentage. Filter BlueZ by connected gamepad device identity,
never by mere presence of a battery service. No invented charge percentage,
charging state, or vendor-specific raw USB/HID commands.

The new BlueZ Flatpak system-bus grant is read-only at the API level and
limited to the named service. No broad `/dev/hidraw*`, sysfs writes, host
process execution, or extra crates.

## Limitations

UPower gaming device enumeration may be incomplete, Bluetooth device icon/name
classification is heuristic, and a 2.4 GHz dongle must expose a valid SDL
percentage or its battery remains hidden. Multi-controller setups with
ambiguous peripheral readings are intentionally not guessed. Test against
actual Fedora/Flatpak installations and both controller connection modes.
