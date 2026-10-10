# Settings — Phase 9.5.44.57

> **Current-state guide:** **Settings → Third-Party** lists one row per service, and each opens its own page: **SteamGridDB** (API key, prefer artwork, refresh, remove key) and **Steam Account** (SteamID64, Web API key, disconnect). These are independent services laid out the same way. Back from either returns to its row in Third-Party. Both use Settings navigation; the Achievements page does not collect credentials. See [ACHIEVEMENTS.md](ACHIEVEMENTS.md) and [PROJECT_STATE.md](PROJECT_STATE.md). Later phase-specific notes below supersede earlier Settings behavior.


Horizon supports SteamGridDB square artwork for Steam and Heroic.
Settings places **Appearance** before **Third-Party**. In Phase 9.5.44.57, the
category rows use a bounded vertical layout and no longer overlap. In
Appearance, selected Theme/System choices use accent-coloured text instead of
both a checkmark and an outline. The nine accent presets appear as a compact
spectrum of colour circles, retaining accessible names for screen readers.
The White swatch changes from white in Dark mode to the same readable grey
used throughout Horizon in Light mode, with an animated transition.

Appearance contains the independent theme, accent, and UI Sounds preferences.
The existing Third-Party API-key and artwork controls remain under their original
category. See `docs/APPEARANCE.md`, ADR 0112, and ADR 0114 for preference,
layout, persistence, and contrast rules.

## SteamGridDB key and preferences

Enter a personal key at **Settings → Third-Party → SteamGridDB → API key**. Generate keys at
https://www.steamgriddb.com/profile/preferences/api. Horizon stores keys in
`config/steamgriddb.json` (see [Files on disk](STORAGE.md); the Flatpak
config directory is sandbox-local), with mode 0600 under a mode-0700 directory.
Keys are not encrypted; same-user programs can read this file. Key entry uses
Horizon-styled masked input with normal Slint text-editing shortcuts, including
Ctrl+V requests paste at the caret and Ctrl+A selects all, using Slint's
built-in TextInput shortcuts. The old Rust-side clipboard reader and extra
Paste button have been removed. **Clipboard paste is currently unverified and
may not work on GNOME Wayland**: Slint 1.18 uses arboard, which does not offer
an appropriate standard focused-window Wayland clipboard path there. Horizon
will not work around this by granting X11 access. The
field uses the placeholder “Enter an API key”, and its horizontal scroll is
clamped to never push a short masked key to the right when the caret moves.
The dialog has no keyboard-shortcut hint strip. An eye/eye-slash icon controls
visibility of the unsaved draft.
The saved key is never disclosed. Enter/controller confirm saves; Esc/Back cancels. The input
receives focus when the editor appears, and new sessions start empty. Saved
values are never shown or read back into Slint. The current draft is cleared
on save/cancel/navigation away. Remove/replace the key at any time.
The Flatpak has **only** `--socket=wayland` for windowing. Its Cargo features
include `backend-winit-wayland`, not `backend-winit-x11`. Neither `--socket=x11`
nor `--socket=fallback-x11` should be reintroduced for clipboard access.
On GNOME Mutter, the native follow-up must use the ordinary, focus-scoped
Wayland `wl_data_device` clipboard path integrated with the active Slint/winit
window or an upstream Slint fix, not a separate background clipboard client.
Until that works and is tested in the Flatpak, users can type the key manually.
Save errors preserve the previous settings. Online verification is deferred;
invalid keys result in background authorization failures, not startup failure.

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

Steam games match by exact AppID (`/games/steam/{appid}`), with no name-search
fallback. Heroic uses a unique exact title after whitespace/case
normalization. Ambiguity means no automatic match. The UI does not yet allow
manual disambiguation or per-game artwork selection.

## Network, safety and offline behavior

SteamGridDB API v2: https://www.steamgriddb.com/api/v2 . HTTPS bearer
credentials are sent only to the fixed API endpoint, never to artwork CDNs.
Only HTTPS `steamgriddb.com` subdomains are eligible for images. All redirects
are disabled. Metadata is limited to 1 MiB, artwork to 12 MiB, and image
content must actually decode as PNG or JPEG at native 512×512 or 1024×1024.
Metadata requests have a bounded retry for transient gateway errors; connect
and total timeouts avoid indefinitely waiting for network access. Work occurs
on a dedicated worker thread; controller input/UI are never blocked by API IO.

Cache: `cache/artwork/steamgriddb/` (see [Files on disk](STORAGE.md)). Entries use
non-identifying stable filenames with a normalized 512×512 PNG and companion
JSON containing SteamGridDB game and grid IDs, matching method, original HTTPS
URL, author, a pixel-art rendering flag, and a checksum over normalized pixels.
No source-owned data is overwritten. Cached art is usable offline even if old.
Cache entries normally recheck after 30 days; missing matches are throttled
for 24 hours. Corrupt/mismatched cache pairs are skipped. The worker attempts
up to three eligible grids per game when downloads fail or decode invalidly.

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

## UI Sounds (Phase 9.5.44.56)

The Appearance page ends with a keyboard/controller-accessible **UI Sounds**
switch, initially **On**. Confirm toggles; Left sets Off and Right sets On.
Preference changes apply immediately and persist without affecting the selected
theme or accent. This switch controls all three bundled action cues: Navigation
(confirmed focus movement), OK (successful menu confirmation), and Back
(successful menu return/dismissal). Ignored input remains silent. The on-screen
subtitle reflects these three sounds rather than referring only to navigation.

## Appearance focus navigation (Phase 9.5.44.59)

System accent and the nine colour dots form **one row**: Right from System
focuses Red, and Left from Red focuses System. Left/Right continue through all
nine swatches and wrap between White and System. Theme System/Light/Dark retains
its own three-option horizontal row. Vertical moves preserve the nearest visual
column and remember the last accent when returning from Theme or UI Sounds.
Only Accept/click changes a theme or accent preference; the navigation sound is
played only for real focus movement. UI Sounds continues to use Left/Right as
Off/On rather than changing focus. See ADR 0116.

## Universal focus and directional wrapping (Phase 9.5.44.60)

Every menu control uses a shared animated accent outline with one visible
focus stroke, including the Settings root category rows, Appearance choices,
API-key editor actions, and the utility strip. Focus animation uses the same
140 ms duration (0 ms in Reduced Motion). Selected options and controller focus
are distinct states.

**Repeat clamp:** held Up/Down/Left/Right never wraps at a row/list edge. Once
an item reaches its last position, it remains there during repeat. To wrap,
release and press again while at that edge. This also applies to the two
Settings categories, Third-Party items and Appearance rows; editor grids use
natural finite 2-D connections. See ADR 0117.


### Phase 9.5.44.62 — shared focus brightness rhythm

Settings categories, Appearance theme options, accent swatches, UI Sounds and
Third-Party actions now share a single outline that pulses uniformly between
86% and 100% of the same accent colour. There is no rotating highlight,
second border, or selection fill. The existing 140ms focus reveal remains;
Reduced Motion/High Contrast use a full-strength static outline. Directional
focus mapping, held-input edge clamping, and semantic UI sounds are unchanged.

## Phase 9.5.44.63 — visible focus breathing

Settings category rows, Theme options, and accent swatches use the same
continuous accent-to-highlight colour rhythm as Home's original focus frame.
A focus-reveal property handles the 140 ms entry/exit; the colour binding
is **not** itself repeatedly tweened, which would mask the idle breathing.
Modal fields/buttons and utility rings use the shared focus surface with the
same live color. The selected accent text and swatch selection are unchanged.

### Phase 9.5.44.65 — Single active focus and page-safe activation

Settings root, Appearance theme choices, accent swatches, and Third-Party
settings use a neutral border when **unfocused** and the existing shared
3.2-second focus brightness-breathing accent stroke only for the **one**
Rust-selected control. Slint `color.mix(other, factor)` gives its *first*
argument weight `factor`, so focus reveals use `1.0 - focus-reveal`, not
`focus-reveal`. Selection (saved choice) does not count as focus.

Outgoing page content may remain visible briefly for the crossfade, but its
pointer callbacks are gated to the currently active Settings page. In the
Settings category root, Appearance is index 0 and Third-Party is index 1;
mouse and controller activation dispatch those distinct destinations. The
existing game cover focus, utility lift and menu-audio behaviour are unchanged.

### Phase 9.5.44.66 — separated accent focus ring and sound description

Each circular accent swatch is 31 px at rest and 36 px when selected or
focused, inside its original 48 × 50 px hit area. One 48 px outer
focus/selection ring surrounds it with a 4 px clear gap at full size to the
inside edge of its normal 2 px stroke (3 px in High Contrast). Only the ring
receives the existing 3.2-second breathing focus colour and 140 ms reveal. Selection
continues to use the existing neutral ring when unfocused, and the focus ring
uses the accent colour when focused. The settings subtitle now describes
Navigation, OK and Back cues, all controlled by the existing UI Sounds switch.

## Phase 10.4.1 — Steam account in Third-Party settings

**Settings → Third-Party → Steam Account** provides a dedicated, controller-
accessible screen to enter a numeric SteamID64, set or replace the personal
Steam Web API key, and remove locally saved credentials. The editor reuses the
SteamGridDB masked entry and the existing Back, Enter, paste and cancel logic.
The SteamID64 is visible; the API-key draft is masked by default, and the saved
secret never appears in Slint. The SteamGridDB artwork integration remains
separate and uses its original settings file.

A new Steam Account row remains reachable even when SteamGridDB is unconfigured.
Disabled SteamGridDB refresh/remove rows and the unavailable Steam disconnect
row are excluded from controller focus traversal. Leaving the nested account
settings screen returns to Third-Party settings rather than to the root page.
The Steam account service is shared with Achievements and reserved for a future
Friends integration; Friends' UI and behavior are unchanged.

Credentials are stored with mode 0600 in an application-private XDG config
file, not in SQLite, the source tree, or the achievement UI. This is **not
an encrypted or Steam OpenID login**. The application never asks for a Steam
password. User-supplied keys must be managed according to Steam's API terms.

## Phase 10.4.2 — Separate third-party sections

*(Superseded: SteamGridDB now has its own sub-page like Steam Account; see the current-state guide above.)* Third-Party settings visibly groups the four SteamGridDB artwork actions together, then separates the **Steam** account entry with a heading and divider. Focus order, persistence, editor handling, and disabled-row skipping remain unchanged. Future providers should use their own named sections rather than appearing as another SteamGridDB setting.

## Phase 10.4.3.3 — Parent-row focus restoration (pending build verification)

Back within Settings is route-local navigation, not a new Settings visit.
Returning from **Appearance** focuses the Appearance row on the root page;
returning from **Third-Party** focuses the Third-Party row; returning from
**Steam Account** focuses its entry under Third-Party. A genuinely fresh
Settings visit (after leaving the Settings route) still resets to Appearance.
The existing editor cancel and refresh-modal Back behavior is unchanged.
