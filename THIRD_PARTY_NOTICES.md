# Third-Party Notices

Horizon includes or derives small portions of artwork and other resources from third-party projects. This file records the source, attribution, license, and modification status for those materials.

## GNOME symbolic artwork

### Controller — `applications-games-symbolic.svg`
- Source: GNOME Symbolic Icon Theme, surfaced through GNOME Icon Library
- Attribution: GNOME Project
- License: Creative Commons Attribution-ShareAlike 3.0 United States (`CC-BY-SA-3.0-US`)
- Upstream attribution guidance: attribution as “GNOME Project” is sufficient; link to https://www.gnome.org/ where practical.
- Source file in Horizon: `ui/assets/applications-games-symbolic.svg`
- Derived use: vector path geometry is reproduced in `ui/components/system-icons.slint` for crisp Slint rendering and theme coloring.
- Changes: geometry is reformatted for Slint; color is applied by Horizon's theme.

## Typography

## License texts

Corresponding license texts are included in `LICENSES/`:

- `CC0-1.0.txt` (also used for project metadata licensing)
- `CC-BY-SA-3.0-US.txt`
- `GPL-3.0-or-later.txt`

## Rust and native dependencies

Horizon also uses third-party software dependencies declared in `Cargo.toml` and the Flatpak manifest. Their own upstream licenses remain applicable. Before a public release, Horizon must commit a resolved `Cargo.lock` and generate a complete dependency-license report from that exact dependency graph. See `docs/LICENSING.md`.

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


### Phase 5 database dependencies

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

### Phase 7 Steam metadata parser

- **steam-vdf-parser 0.1.2**
  - Upstream: https://github.com/mexus/steam-vdf-parser
  - Copyright: Copyright 2026 steam-vdf-parser contributors
  - License: Apache-2.0 OR MIT
  - Use in Horizon: parse Steam text VDF metadata and current binary `appinfo.vdf` v40/v41 data inside the Steam source adapter.
  - Modification: none; consumed as a Cargo dependency.

The Apache-2.0 and MIT license texts are included as `LICENSES/Apache-2.0.txt` and `LICENSES/MIT.txt`. The exact resolved Cargo dependency-license inventory remains a release gate and will be generated from the committed `Cargo.lock` before public release.

### Phase 9 metadata format dependencies

- **serde 1.x / serde_json 1.x**
  - Upstream: https://github.com/serde-rs/serde and https://github.com/serde-rs/json
  - License: Apache-2.0 OR MIT
  - Use in Horizon: deserialize provider-owned local metadata for the Heroic and Bottles adapters.
  - Modification: none; consumed as Cargo dependencies.
- **yaml_serde 0.10**
  - Upstream: https://github.com/yaml/yaml-serde
  - Maintainer: YAML Organization; actively maintained fork of serde-yaml.
  - License: Apache-2.0 OR MIT
  - Use in Horizon: deserialize Bottles `bottle.yml` metadata inside the Bottles adapter.
  - Modification: none; consumed as a Cargo dependency.

The existing Apache-2.0 and MIT license texts cover these dual-licensed dependencies. The exact resolved Cargo dependency-license inventory remains a release gate and will be generated from the committed `Cargo.lock` before public release.


### Phase 9.5 D-Bus dependency

- **zbus 5.x**
  - Upstream: https://github.com/z-galaxy/zbus
  - License: MIT
  - Use in Horizon: narrow user-session D-Bus transport between the Flatpak and
    the optional same-user `horizon-session-helper`.
  - Modification: none; consumed as a Cargo dependency.

The existing MIT license text is included as `LICENSES/MIT.txt`.

**Gamescope is not bundled by Horizon.** Phase 9.5 may use an independently
installed host Gamescope executable through the optional helper. Horizon neither
redistributes Gamescope nor copies its source in this phase.


### Phase 9.5.29 artwork dependencies

- **image 0.25.x**
  - Upstream: https://github.com/image-rs/image
  - License: MIT OR Apache-2.0
  - Use in Horizon: decode provider-owned local Steam JPG/PNG/ICO artwork and
    normalize presentation pixels to Horizon's square-artwork contract.
  - Modification: none; consumed as a Cargo dependency.
- **zip 8.6.x**
  - Upstream: https://github.com/zip-rs/zip2
  - License: MIT
  - Use in Horizon: read local Steam `linuxclienticon` ZIP containers and feed
    their PNG representations into the generic artwork decoder.
  - Modification: none; consumed as a Cargo dependency with Deflate support.

Steam game artwork is not bundled with Horizon. Horizon reads images already
present in the user's Steam installation/cache and displays them locally. The
artwork remains owned/licensed by its respective game publisher/developer.

The existing MIT and Apache-2.0 license texts cover these dependencies. The exact
resolved Cargo dependency-license inventory remains a release gate.
