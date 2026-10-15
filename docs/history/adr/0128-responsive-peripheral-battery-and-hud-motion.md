# ADR 0128: Responsive peripheral power polling and HUD presence motion

Status: Prepared for validation (Phase 9.5.44.73)

## Context

Bluetooth battery could appear late because the system-status worker queried
NetworkManager, UPower and BlueZ serially on one five-second cadence. The SDL3
status timer refreshed power every ten seconds. Battery and controller symbols
were created with `if`, preventing exit animations.

## Decision

- Keep SDL3 polling on its existing input timer, but read power on a separate
  two-second schedule and immediately when an input device is added/removed.
- Keep network's existing five-second worker and stable link filter; run a
  second background worker for UPower/BlueZ every two seconds.
- Merge tagged network/power updates in `StatusMonitor`, keeping the last
  observed values for the unaffected domain. Do not block Slint/SDL on D-Bus.
- Keep host-battery precedence. SDL, BlueZ and UPower never invent a percentage.
- Keep battery and footer controller visual elements mounted. Animate alpha
  and 7px-or-less directional travel, 220ms entrance / 155ms exit, with
  Reduced Motion resolving everything immediately. Animate top cluster width
  so remaining status icons reflow rather than jump. Do not touch GameCard focus.

## Limitations

The 8BitDo Ultimate 2's 2.4GHz receiver must expose an actual battery reading,
usually via compatible SDL HIDAPI DInput support. Xbox/XInput emulation often
omits battery percentage. The app cannot infer battery level accurately from
controller connectivity or packet traffic. The system-bus observer remains a
periodic snapshot, not a promised sub-second notification API. No new privilege
or helper is added.

## Validation

Independent merge/presence/state checks are performed during packaging. Run
`cargo check && cargo test` and visually test controller Bluetooth/2.4G
connect/disconnect, laptop/desktop precedence and Reduced Motion before release.
