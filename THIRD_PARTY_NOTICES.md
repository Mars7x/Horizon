# Third-Party Notices

Horizon includes or derives small portions of artwork and other resources from third-party projects. This file records the source, attribution, license, and modification status for those materials.

## Kenney Input Prompts

### Controllers — `ui/assets/controllers/`
- Source: **Input Prompts 1.5A** by **Kenney** (www.kenney.nl),
  https://kenney.nl/assets/input-prompts (`kenney_input-prompts_1.5.zip`).
- License: Creative Commons Zero (`CC0-1.0`), `LICENSES/CC0-1.0.txt`. Credit
  isn't required; it's given here anyway.
- Files, copied byte-for-byte from the pack's `Vector/` folders:
  `ui/assets/controllers/controller_xbox360.svg`,
  `ui/assets/controllers/controller_xboxseries.svg`,
  `ui/assets/controllers/controller_playstation3.svg`,
  `ui/assets/controllers/controller_playstation4.svg`,
  `ui/assets/controllers/controller_playstation5.svg`,
  `ui/assets/controllers/controller_switch_pro.svg`,
  `ui/assets/controllers/switch_joycon_left.svg`,
  `ui/assets/controllers/switch_joycon_right.svg`,
  `ui/assets/controllers/switch_joycon.svg`,
  `ui/assets/controllers/controller_steamdeck.svg`,
  `ui/assets/controllers/controller_generic.svg`.
- Rendering: the footer's controller icon (`ui/components/system-icons.slint`)
  shows the model of the controller used last, tinted with Horizon's theme
  colour at the same UI-scale-aware raster resolution as the utility icons.
  The files are not modified. See ADR 0186.
- Not Kenney's: `ui/assets/controllers/horizon_odin3.svg` (the AYN Odin 3) is
  Horizon's own drawing in the same style, as the pack has no Odin (ADR 0187).

## GNOME symbolic artwork

### Controller — retired

The footer's controller icon was GNOME's `applications-games-symbolic.svg`
(GNOME Project, CC-BY-SA-3.0-US). Kenney's controller drawings replaced it
(ADR 0186) and the file is no longer bundled. Kept as history.

### Network status — retired

The network status glyphs are now Horizon's own native drawings
(`ui/components/status-glyphs.slint`, ADR 0171), in the style of its battery
indicator; no GNOME status artwork is bundled any more. The record below is
kept as history.

#### Formerly: GNOME Symbolic / Adwaita icons (Phase 9.5.44.72)

Only eight supplied status glyphs were bundled, corresponding to observable
network states; no unused acquiring or wired-disconnected symbols are copied.

- **Wi-Fi:** `ui/assets/status/radiowaves-1-symbolic.svg`,
  `ui/assets/status/radiowaves-2-symbolic.svg`,
  `ui/assets/status/radiowaves-3-symbolic.svg`,
  `ui/assets/status/radiowaves-4-symbolic.svg`,
  `ui/assets/status/radiowaves-x-symbolic.svg`, and
  `ui/assets/status/radiowaves-question-symbolic.svg`.
  These six user-supplied GNOME Symbolic SVGs identify **Jakub Steiner** in their
  embedded metadata and explicitly declare **CC0 1.0 Universal**.
  License: `LICENSES/CC0-1.0.txt`, https://creativecommons.org/publicdomain/zero/1.0/.
- **Ethernet:** `ui/assets/status/network-wired-symbolic.svg` and
  `ui/assets/status/network-wired-no-route-symbolic.svg`. From the GNOME Adwaita Icon Theme,
  attributed to the **GNOME Project**. License choice: **CC BY-SA 3.0 US**
  (Adwaita dual-license terms also permit LGPL v3+). Existing project license
  text: `LICENSES/CC-BY-SA-3.0-US.txt`.
  Upstream: https://gitlab.gnome.org/GNOME/adwaita-icon-theme.
- **Modification:** All eight supplied SVGs remain bundled byte-for-byte, without
  geometry changes. Phase 9.5.44.76 replaces Phase 9.5.44.74's Slint Path
  transcription with direct SVG source rendering using the same adaptive
  raster scale as the utility icons. Runtime theme tint preserves source alpha;
  at small UI scales no oversized intermediate bitmap is downsampled, while
  fullscreen enlargement requests appropriately higher raster resolution.
  Original assets and GNOME/CC0/Adwaita license notices remain unchanged.
- **Phase 9.5.44.78 adaptation:** The normal `network-wired-symbolic.svg`
  topology is redrawn with pixel-aligned Slint rectangles at its 22px display
  size for small-scale clarity. This is a derivative of the attributed GNOME
  Project CC BY-SA 3.0 US wired artwork, not new original icon authorship.
  The original GNOME SVG remains byte-for-byte intact. The complex
  `network-wired-no-route` variant continues to render directly from SVG.
- **Purpose:** Wi-Fi signal strengths, limited/offline Wi-Fi, wired link, and
  wired no-route indication. No assets for unobserved states are included.

## Typography

## License texts

Corresponding license texts are included in `LICENSES/`:

- `CC0-1.0.txt` (also used for project metadata licensing)
- `CC-BY-SA-3.0-US.txt`
- `GPL-3.0-or-later.txt`

## Rust and native dependencies

Horizon also uses third-party software dependencies declared in `Cargo.toml` and the Flatpak manifest. Their own upstream licenses remain applicable. Cargo.lock is committed. Before a public release, Horizon must generate a complete dependency-license report from that exact dependency graph. See `docs/LICENSING.md`.

This notice is intended to preserve provenance and attribution. It does not replace the applicable license texts.


## Bundled typeface: LINE Seed JP

- **Typeface:** LINE Seed JP
- **Copyright:** © LY Corporation
- **Design/project credits:** LY Corporation, RixFont, Fontworks
- **Upstream:** https://github.com/line/seed
- **Bundled binary source:** Google Fonts, pinned commit `874ec71eac706dd23900d1305449abed6767b7df`
- **License:** SIL Open Font License 1.1 (`OFL-1.1`)
- **Use in Horizon:** Application-wide UI typeface, including game titles, placeholder artwork text, and the clock.
- **Distribution:** The four unmodified Google Fonts static TTF faces are downloaded from the pinned commit during the development Flatpak build and bundled into `/app/share/fonts`.
- **Modification:** None. Horizon installs the published font faces without changing their names or outlines.

The applicable license text is included as `LICENSES/OFL-1.1.txt`.


### Database dependencies

- **rusqlite / libsqlite3-sys**
  - Upstream: https://github.com/rusqlite/rusqlite
  - License: MIT
  - Use in Horizon: Rust SQLite bindings for the persistence adapter.
  - Modification: none; consumed as Cargo dependencies.
- **SQLite core**
  - Upstream: https://www.sqlite.org/
  - Status: public domain.
  - Use in Horizon: embedded database engine compiled through rusqlite's `bundled` feature.
  - Modification: none by Horizon.

The exact resolved Cargo dependency-license inventory remains a release gate and will be generated from the committed `Cargo.lock` before public release.

### Formerly: Steam metadata parser

- **steam-vdf-parser 0.1.2**
  - Upstream: https://github.com/mexus/steam-vdf-parser
  - Copyright: Copyright 2026 steam-vdf-parser contributors
  - License: Apache-2.0 OR MIT
  - Historical use: parsed local Steam metadata. The dependency and local Steam reader were removed in ADR 0191; attribution and license texts are retained.
  - Modification: none; consumed as a Cargo dependency.

The Apache-2.0 and MIT license texts are included as `LICENSES/Apache-2.0.txt` and `LICENSES/MIT.txt`. The exact resolved Cargo dependency-license inventory remains a release gate and will be generated from the committed `Cargo.lock` before public release.

### Serialization dependencies

- **serde 1.x / serde_json 1.x**
  - Upstream: https://github.com/serde-rs/serde and https://github.com/serde-rs/json
  - License: Apache-2.0 OR MIT
  - Use in Horizon: serialize configuration, cache records and retained service API data.
  - Modification: none; consumed as Cargo dependencies.

The existing Apache-2.0 and MIT license texts cover these dual-licensed dependencies. The exact resolved Cargo dependency-license inventory remains a release gate and will be generated from the committed `Cargo.lock` before public release.


### D-Bus dependency

- **zbus 5.x**
  - Upstream: https://github.com/z-galaxy/zbus
  - License: MIT
  - Use in Horizon: read-only system-bus status queries (network, battery).
  - Modification: none; consumed as a Cargo dependency.

The existing MIT license text is included as `LICENSES/MIT.txt`.

### Video and sound libraries (not bundled)

- **FFmpeg** (`libavformat`, `libavcodec`, `libavutil`, `libswscale`, `libswresample`)
  - Upstream: https://ffmpeg.org/
  - License: LGPL-2.1-or-later, or GPL-2.0-or-later for builds with GPL parts
  - Use in Horizon: decodes Album videos, their poster frames and their sound.
- **PipeWire** (`libpipewire-0.3`)
  - Upstream: https://pipewire.org/
  - License: MIT
  - Use in Horizon: plays UI sounds and video sound.

Horizon bundles neither. It links dynamically against the copies the Flatpak
runtime (`org.freedesktop.Platform`) ships, including FFmpeg's full codec set
from the runtime's `codecs-extra` extension, under their own licenses; all are
compatible with Horizon's GPL-3.0-or-later. Horizon's own C shims over them
(`csrc/`) are part of Horizon. The build uses the **cc** and **pkg-config**
crates (MIT OR Apache-2.0), consumed as Cargo build dependencies without
modification.

**Gamescope is not bundled by Horizon.** The former optional session integration
was removed. Its provenance remains in [documentation history](docs/history/INDEX.md);
Horizon no longer launches or depends on a compositor/session helper.

### Image decoding and retired artwork containers

- **image 0.25.x**
  - Upstream: https://github.com/image-rs/image
  - License: MIT OR Apache-2.0
  - Use in Horizon: decode supported profile, Album and artwork images and
    normalize presentation pixels to the relevant square-artwork contract.
  - Modification: none; consumed as a Cargo dependency.
- **zip 8.6.x (retired direct dependency)**
  - Upstream: https://github.com/zip-rs/zip2
  - License: MIT
  - Historical use: read local Steam `linuxclienticon` ZIP containers and feed
    their PNG representations into the generic artwork decoder.
  - Modification: none; consumed as a Cargo dependency with Deflate support.

Steam game artwork is not bundled with Horizon. Earlier releases read images
from local Steam caches; that integration is removed. The
artwork remains owned/licensed by its respective game publisher/developer.

The existing MIT and Apache-2.0 license texts cover these dependencies. The exact
resolved Cargo dependency-license inventory remains a release gate.

### Album image formats (GIF, JPEG XL)

- **image 0.25.x, `gif` feature** (the `gif` crate, MIT OR Apache-2.0):
  decodes GIF captures in the Album, including animated GIFs frame by frame.
- **jxl-oxide 0.12.x**
  - Upstream: https://github.com/tirr-c/jxl-oxide
  - License: MIT OR Apache-2.0 (its `jxl-*` crates likewise)
  - Use in Horizon: decode JPEG XL captures in the Album through `image`'s
    decoding hook, under the same size and memory limits as every other format.
  - Modification: none; consumed as a Cargo dependency with only its `image`
    integration enabled.
  - Brings **brotli-decompressor 5.x** (BSD-3-Clause OR MIT, used under MIT) and
    **alloc-no-stdlib / alloc-stdlib** (BSD-3-Clause).

The existing MIT and Apache-2.0 license texts cover these dependencies; the
BSD-3-Clause allocator crates carry their notice in their own sources. The exact
resolved Cargo dependency-license inventory remains a release gate.

### Formerly: multi-resolution ICO dependency

- **ico 0.5.x (retired direct dependency)**
  - Upstream: https://github.com/mdsteele/rust-ico
  - License: MIT
  - Historical use: enumerate and decode every representation in Steam's local
    `clienticon` ICO containers so Horizon could explicitly choose the largest
    true-square frame instead of relying on a generic decoder's implicit frame
    selection.
  - Modification: none; consumed as a Cargo dependency.

The existing MIT license text covers this dependency. The exact resolved Cargo
dependency-license inventory remains a release gate.

### SteamGridDB API transport

- **reqwest 0.12.x**
  - Upstream: https://github.com/seanmonstar/reqwest
  - License: MIT OR Apache-2.0
  - Use in Horizon: blocking Rust HTTPS transport with rustls TLS for optional,
    personal-key-authenticated SteamGridDB API v2 metadata requests and
    separate unauthenticated CDN image requests.
  - Modification: none; consumed as a Cargo dependency.
- **SteamGridDB API v2**
  - Upstream: https://www.steamgriddb.com/api/v2
  - Use in Horizon: retrieve game IDs, titles and metadata for square-grid
    candidates and download eligible square artwork into the user’s own cache.
    No SteamGridDB artwork is bundled or redistributed with Horizon.
  - Third-party image authors and ownership: user-submitted images may belong
    to separate contributors or game publishers. Local cache entries retain source URLs, game/asset IDs, and author credits
    in sidecar JSON. API access is not a blanket permission to redistribute
    artwork; attribution and rights remain with original authors/owners.

The existing MIT and Apache-2.0 license texts cover the dual-licensed Rust
transport dependency. The exact resolved dependency-license inventory remains
a release gate.

### Slint's Linux clipboard backend (Phase 9.5.44.5)

- **arboard 3.6.x (transitive dependency of Slint's winit backend)**
  - Upstream: https://github.com/1Password/arboard
  - License: MIT OR Apache-2.0
  - Use in Horizon: Slint's built-in text editing. The API-key editor now requests a focus-scoped standard Wayland selection through smithay-clipboard. Horizon neither enables X11 nor grants X11 socket access.
  - Modification: none; Horizon does not directly call arboard or SDL3 for API-key clipboard access.

- **smithay-clipboard 0.7.3**
  - Upstream: https://github.com/Smithay/smithay-clipboard
  - License: MIT
  - Use in Horizon: focused-window, standard Wayland `wl_data_device` text
    selection for the Settings API-key editor, reusing Winit's display handle.
    Requests are dispatched to a worker thread and do not invoke X11.
  - Modification: none; consumed as a Cargo dependency.

The existing MIT and Apache-2.0 license texts apply. Horizon stores the key
only on explicit Save; key content and clipboard data must not be logged.


## User-provided navigation sound (Phase 9.5.44.56, updated 9.5.44.58)

- **Asset:** `ui/assets/horizon-navigation.wav`
- **Origin:** Replacement WAV supplied by the Horizon project owner in the development conversation (October 8, 2026). The earlier `horizon-navigation-soft.wav` was retired in Phase 9.5.44.58.
- **Modification:** None; replacement PCM WAV bytes are bundled unmodified. No gain adjustment, normalizing, or transcode.
- **Ownership/license:** Not independently established. This is not presented as an
  externally licensed or public-domain asset. Confirm redistribution rights
  with the project owner before any public/Flathub release.


## User-supplied menu confirmation audio (Phase 9.5.44.61)

- **Assets:** `ui/assets/horizon-ok.wav`, `ui/assets/horizon-back.wav`
- **Origin:** WAV files supplied by the Horizon project owner (October 8, 2026). The Back cue was replaced by a revised owner-supplied recording in Phase 9.5.44.64; the OK cue remains unchanged.
- **Modification:** None. Original 48 kHz mono 16-bit PCM bytes are preserved.
- **Rights:** Redistribution rights have not been independently established.
  Confirm permission with the asset owner before a public release.
- **Use:** Successful menu confirmation and actual back/dismissal only. The
  existing navigation sound is unchanged. The UI Sounds setting mutes all cues.

## Local ROM recognition

- **quick-xml 0.41.x** (MIT): parses user-selected XML DAT catalogs.
- **ring 0.17.x**: computes catalog-compatible SHA-1 and content-identity SHA-256.
  This existing transitive dependency is now used directly; its upstream license
  and bundled third-party notices remain applicable.

No ROMs or No-Intro catalogs are bundled. Catalog names are provenance supplied
by the selected local DAT, not a statement of redistribution rights. The exact
resolved dependency-license inventory remains a release gate.
