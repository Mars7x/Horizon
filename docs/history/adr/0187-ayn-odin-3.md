# ADR 0187: The AYN Odin 3's own controls

Date: 2026-10-12

## Status

Accepted. Extends ADR 0185 (glyphs) and ADR 0186 (footer drawing). Not yet
tested on the device.

## Context

The Odin 3 (Snapdragon 8 Elite) runs Linux: ArmadaOS (SteamOS-style, with
Flatpak) and ROCKNIX. Its buttons wear Nintendo's labels (A on the right), but
on Linux its controls reach SDL positionally: the kernel's RSInput driver
reports them as "AYN Odin3 Gamepad" (`2020:3001`, from the device tree), and
on ArmadaOS InputPlumber hides that pad behind a virtual Xbox Elite 2
(`045e:0b00`, InputPlumber's Odin 3 profile; ArmadaOS doesn't override its
target). Horizon took either for an Xbox pad: the button printed B confirmed
while the hint said A.

## Decision

- **Detection:** the device tree's model, "AYN Odin 3". Flatpak doesn't show
  `/sys/firmware`, but sysfs repeats the model as the SoC's machine
  (`/sys/devices/soc0/machine`); `/sys/firmware/devicetree/base/compatible`
  (`ayn,odin3`) is read too, for builds outside the sandbox.
- **Its controls:** on an Odin 3, the raw pad and InputPlumber's Elite 2 are
  the Odin family. Other pads keep their own. InputPlumber's Elite 2 has a real Elite 2's name and IDs, so it
  counts only when it's virtual: its evdev node's sysfs device is under
  `/sys/devices/virtual/input` (uinput). A real Elite 2 over USB is on the USB
  bus, and over Bluetooth it has other product IDs.
- **Behind Steam Input:** SDL names the real controller behind each Steam
  virtual pad from Steam's `SteamVirtualGamepadInfo` file (readable through
  the existing Steam grants), so the Odin's controls arrive as InputPlumber's
  Elite 2 (still virtual) and other pads as themselves. Only a virtual pad SDL
  can't resolve (`28de:11ff`) is taken for the Odin's, as on the Deck, which
  this also fixes: a Deck took every Valve-vendor pad for its own, a real
  Steam Controller included. If Steam's variable is missing, Horizon points
  it at Steam's file itself before SDL starts.
- **A real Elite 2 behind Steam:** it and the Odin's Elite 2 look the same to
  SDL, so sysfs (which Steam doesn't hide) decides: no real Elite 2 means the
  Odin's; no InputPlumber pad means the real one; with both, the Odin's has
  Steam's first slot.
- **One pad, one press:** if the raw pad and InputPlumber's are both open
  (ArmadaOS shares raw input), the raw one is ignored.
- **Glyphs and buttons:** Nintendo's convention (right confirms, bottom goes
  back, top is secondary) with A/B/X letters and L1/R1 shoulders.
- **Drawing:** Kenney's pack has no Odin, so Horizon draws one in its style
  (`ui/assets/controllers/horizon_odin3.svg`, Horizon-owned): the same 64-unit
  canvas and handheld corners as Kenney's Deck, a hollow 16:9 screen, the
  left stick above the D-pad and the face buttons above the right stick.

## Consequences

- A real Xbox Elite 2 on an Odin 3 shows Xbox glyphs, behind Steam Input
  too. With both connected behind Steam, the slot order is assumed: if the
  Elite 2 was connected before InputPlumber started, the two swap.
- If Steam's file can't be read at all, every pad behind Steam Input on a
  Deck or an Odin 3 counts as the handheld's own.
- If InputPlumber's target is changed (to a DualSense, say), the Odin shows
  that pad's glyphs.
- If ArmadaOS's "share raw input" lets Horizon open the raw pad as well as
  InputPlumber's, both are the Odin family, but a press could count twice;
  that needs checking on a device.
