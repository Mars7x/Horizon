# ADR 0171: Native status glyphs, signal-driven power status

**Status:** Implemented. The build and gate pass. Every network state and battery level was rendered in light and dark. On this laptop's real system bus the battery reads once and stays quiet. A charger plug-in (the charge signal) still needs confirming in the running app.

## Context

The user asked for the Wi-Fi and Ethernet indicators to use the battery's style instead of GNOME's icons, and for the battery's workings to be optimized.

- The network glyphs were eight GNOME SVGs plus a hand-snapped copy of the wired one.
- The power worker re-read UPower and BlueZ every 2 s. Each read built new D-Bus proxies, which fetch every property and subscribe to changes, and it enumerated devices. It also woke the UI with every reading, changed or not.

## Decision

**Glyphs.** One component file, `status-glyphs.slint`, draws the battery and the network glyphs in a shared style (`StatusStyle`):
- a 1.8 px stroke with round caps;
- foreground ink, and a faint ink for unfilled parts;
- discrete steps on one 24x20 grid at the battery's scale.

The glyphs:
- **Wi-Fi:** a dot and three arcs lit by strength (25/50/75 %).
- **Ethernet:** a port with pins.
- **Limited:** `!`.
- **Other connection:** a globe.
- **Offline:** struck-through faint Wi-Fi.

The GNOME status SVGs, their component and their attribution checks are removed. The notice is kept as history, and the controller icon is still GNOME's.

**Power.**
- UPower and BlueZ signals wake the power worker (`watch_signals`, now shared with NetworkManager). It settles for 100 ms and re-reads once.
- A 60 s recheck covers missed signals and bus restarts.
- Reads use uncached proxies (`proxy()`), for network reads too.
- Only a changed reading is sent to the UI.

## Consequences

- An idle Horizon makes no power D-Bus calls between changes, down from about 10 calls every 2 s.
- A charge or level change shows as soon as UPower signals it.
- No third-party artwork remains in the status bar.
