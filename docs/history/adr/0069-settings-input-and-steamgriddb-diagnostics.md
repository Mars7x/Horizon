# ADR 0069 — API editor correctness and SteamGridDB visibility (9.5.44.12)

## Decision

- Preserve normal Slint text-editing semantics: the application-only native
  Wayland clipboard reader returns text, which is injected as a Slint keyboard
  text event at the currently focused TextInput. Unlike the previous
  whole-draft replacement, repeated Ctrl+V inserts repeatedly and selections
  are replaced in-place. Retain the asynchronous generation/draft checks.
- Keep the entire API-key editor native Wayland; do not enable X11 or a
  privileged data-control protocol. No clipboard contents are logged.
- Drop the independently animated custom caret. Password dots and revealed
  glyphs have different widths; Slint's native caret correctly uses their
  rendered positions. Keep smooth horizontal scroll and clip-edge fades.
- Make controller focus visible on filled accent-colour action buttons with
  a separated high-contrast outline outside the actual button geometry.
- Explain artwork lookup outcomes in Settings: show progress and counts for
  successful/cached images, unmatched titles, unsupported squares, and negative
  cache hits. Preserve source-first/SteamGridDB-first precedence.
- When no suitable native-square grid exists, try SteamGridDB native 512px or
  1024px PNG icons (still require image dimension validation, HTTPS CDN
  allowlist, no redirect, contributor provenance). Ignore old negative cache
  entries from before this extra fallback was available.

## Limits

No API key is bundled or transmitted for testing. Live SteamGridDB content,
key validity, and Flatpak/Wayland behavior must be checked on the user's GNOME
installation. An external provider can lack suitable square art even for a
valid game. Slint/Rust compilation also requires GNOME Builder's toolchain.
