# Clock integration

Horizon renders a live local clock in the home footer. The display format is
host-owned: the application reads it through the XDG Settings portal rather
than directly querying GNOME settings from inside the Flatpak sandbox.

## Portal setting

On GNOME, the Settings portal exposes:

```text
namespace: org.gnome.desktop.interface
key:       clock-format
```

Expected values are `12h` and `24h`. Horizon subscribes to the portal's
`SettingChanged` signal, so changing the desktop clock format while Horizon is
running updates the footer without restarting the app.

If the portal/backend does not expose the setting, Horizon uses a deterministic
12-hour fallback. This fallback lives in the platform layer and is not a UI
special case.

## Dependency direction

```text
org.freedesktop.portal.Settings
            |
            v
platform::clock::ClockFormatMonitor
            |
            v
presentation::clock::ClockController
            |
            v
AppWindow clock-time / clock-suffix
            |
            v
Footer
```

The UI never calls GSettings or DBus directly. The controller owns formatting
of the current local time; Slint only renders the two presentation strings.

## 12/24-hour rendering

- 12-hour mode: `1:42 PM`
- 24-hour mode: `13:42`

The AM/PM suffix is omitted entirely in 24-hour mode, and the complete clock
group remains horizontally centered in both modes.
