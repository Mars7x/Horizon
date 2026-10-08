# Settings → Third-Party — Phase 9.5.44.12

Horizon supports SteamGridDB square artwork for Steam, Heroic and Bottles.
Only the Third-Party portion of the Settings utility is implemented.

## SteamGridDB key and preferences

Enter a personal key at **Settings → Third-Party → API key**. Generate keys at
https://www.steamgriddb.com/profile/preferences/api. Horizon stores keys in
`$XDG_CONFIG_HOME/io.github.Mars7x.Horizon/third-party.json` (the Flatpak
config directory is sandbox-local), with mode 0600 under a mode-0700 directory.
Keys are not encrypted; same-user programs can read this file. Key entry is a
single-line, masked editable field with no placeholder. The eye icon only reveals
an *unsaved* draft. Long strings scroll smoothly to keep the caret in view; the
clipping edges receive the same surface-gradient fade treatment as the Home
selected-game title. The cursor uses Slint's built-in password-aware caret
so it repositions correctly when Show/Hide changes glyph widths. The text
scroll respects Reduced Motion. Short strings stay anchored.

Ctrl+V is handled by Horizon's focused-window Wayland adapter, using the ordinary
`wl_data_device` selection through the existing Winit `wl_display`. It does not
use X11, XWayland, `ext-data-control`, a shell command or a new Wayland connection.
The adapter reads text asynchronously, validates a nonempty single ASCII token
(maximum 512 bytes), then dispatches one normal Slint text-input event at
the focused caret. As in a regular text field, paste inserts the text or
replaces the selection; a second paste inserts again. Delayed results are
ignored if the editor was closed or its draft changed. The key is never logged.
Slint's built-in clipboard remains in place for other text inputs; Horizon
intercepts Ctrl+V only inside this API-key editor. No visible paste button or
shortcut hints are shown. **End-to-end GNOME Wayland / Flatpak testing is still
required**; this design is not a claim of a confirmed working paste operation.

Enter/controller confirm saves; Esc/Back cancels. Each edit session starts empty,
so saved keys are never displayed or read back into Slint. Drafts are cleared on
save, cancel, or navigation away. The Flatpak windowing permission remains only
`--socket=wayland`, and Slint only enables `backend-winit-wayland`. No X11
permission or feature should be added as a workaround.

Save errors preserve the previous settings. Online verification is deferred;
invalid keys result in background authorization failures reported on the
Third-Party page, not startup failure. Saving a key alone does not guarantee
that a supported square exists for every game.

**Prefer SteamGridDB artwork** uses an animated switch with a white thumb,
not an On/Off text label. The row is a single focus target: controller confirm or click toggles,
Left selects Off, and Right selects On. The slider motion, focused rows, dialog
and page fades respect Reduced Motion. (Off by default):

- Off: source-owned native-square artwork first, SteamGridDB only when absent.
- On: SteamGridDB first; source-owned artwork remains visible until new art is
  ready and remains the fallback if lookup, download or cache decoding fails.
- With no API key: use source-owned art or Horizon's generated placeholder.

Changes to the key or preference immediately reset visible artwork to the
source fallback and restart eligible background lookups. Old in-flight results
cannot overwrite the new preference. Restart is not required. The artwork
cache is preserved on API-key removal, but is not shown without a saved key.

Steam games try exact AppID (`/games/steam/{appid}`) first. If the API
cannot find that AppID, Horizon can try a unique exact title after whitespace/
case normalization. Heroic and Bottles use that same strict title check. Ambiguity means no automatic match. The UI does not yet allow
manual disambiguation or per-game artwork selection.

## Network, safety and offline behavior

SteamGridDB API v2: https://www.steamgriddb.com/api/v2 . HTTPS bearer
credentials are sent only to the fixed API endpoint, never to artwork CDNs.
Only HTTPS `steamgriddb.com` subdomains are eligible for images. All redirects
are disabled. Metadata is limited to 1 MiB, artwork to 12 MiB, and image
content must actually decode as static PNG/JPEG in supported native-square
sizes. Grids require 512×512 or 1024×1024; eligible square PNG icons may
also be 256×256 or 768×768.
Metadata requests have a bounded retry for transient gateway errors; connect
and total timeouts avoid indefinitely waiting for network access. Work occurs
on a dedicated worker thread; controller input/UI are never blocked by API IO.

Cache: `$XDG_CACHE_HOME/horizon/artwork/square-v6/steamgriddb/`. Entries use
non-identifying stable filenames with a normalized 512×512 PNG and companion
JSON containing SteamGridDB game and grid IDs, matching method, original HTTPS
URL, author, a pixel-art rendering flag, and a checksum over normalized pixels.
No source-owned data is overwritten. Cached art is usable offline even if old.
Cache entries normally recheck after 30 days; missing matches are throttled
for five minutes rather than a full day. Corrupt/mismatched cache pairs are
skipped. The worker attempts up to five eligible images per game when downloads
fail or decode invalidly. When no static 512x512/1024x1024 square grid exists,
a square SteamGridDB icon (256/512/768/1024 PNG) is eligible as a fallback.
Legacy `.miss-v2` entries are ignored on this update. The Third-Party page shows
per-game lookup progress and final counts for cached/loaded covers, missing
matches, unusable assets and recent negative cache entries; no keys are shown.

`--share=network` in the Flatpak allows outbound network access; Flatpak
cannot scope this permission to a specific host. Horizon's client separately
restricts its requests. Home uses the existing source cache and same pixel-art
nearest-neighbour and non-pixel-art Lanczos3 normalization; Slint receives the
same pixelated/smooth rendering bit.

No artwork is shipped in Horizon's package. User-submitted grid artwork may
have third-party copyrights; the user is responsible for their rights of use.
Downloaded art is not licensed for redistribution by virtue of API access.
Contributor provenance is stored per downloaded asset for inspection and
future UI attribution. See `THIRD_PARTY_NOTICES.md` and `docs/ARTWORK.md`.
