# Third-Party Notices

Horizon includes or derives small portions of artwork and other resources from third-party projects. This file records the source, attribution, license, and modification status for those materials.

## GNOME symbolic artwork

### Friends — `emote-smile-symbolic.svg`
- Source: GNOME Icon Library / GNOME Icon Development Kit
- Creator credited in the supplied SVG metadata: Jakub Steiner
- License: CC0 1.0 Universal (`CC0-1.0`)
- Source file in Horizon: `ui/assets/friends-smiley.svg`
- Derived use: vector path geometry is reproduced in `ui/components/utility-icons.slint` for crisp Slint rendering and runtime recoloring.
- Changes: geometry is reformatted for Slint; color is applied by Horizon's theme.

### Album — `landscape-symbolic.svg`
- Source: GNOME Icon Library / GNOME Icon Development Kit
- Creator credited in the supplied SVG metadata: Jakub Steiner
- License: CC0 1.0 Universal (`CC0-1.0`)
- Source file in Horizon: `ui/assets/album-landscape.svg`
- Derived use: vector path geometry is reproduced in `ui/components/utility-icons.slint` for crisp Slint rendering and runtime recoloring.
- Changes: geometry is reformatted for Slint; color is applied by Horizon's theme.

### Activity — `dictionary-symbolic.svg`
- Source: GNOME Icon Library / GNOME Icon Development Kit
- Creator credited in the supplied SVG metadata: Jakub Steiner
- License: CC0 1.0 Universal (`CC0-1.0`)
- Source file in Horizon: `ui/assets/dictionary-symbolic.svg`
- Derived use: vector path geometry is reproduced in `ui/components/utility-icons.slint` for crisp Slint rendering and runtime recoloring.
- Changes: geometry is reformatted for Slint; color is applied by Horizon's theme.

### Controller — `applications-games-symbolic.svg`
- Source: GNOME Symbolic Icon Theme, surfaced through GNOME Icon Library
- Attribution: GNOME Project
- License: Creative Commons Attribution-ShareAlike 3.0 United States (`CC-BY-SA-3.0-US`)
- Upstream attribution guidance: attribution as “GNOME Project” is sufficient; link to https://www.gnome.org/ where practical.
- Source file in Horizon: `ui/assets/applications-games-symbolic.svg`
- Derived use: vector path geometry is reproduced in `ui/components/system-icons.slint` for crisp Slint rendering and theme coloring.
- Changes: geometry is reformatted for Slint; color is applied by Horizon's theme.

### Settings — `org.gnome.Settings-system-symbolic.svg`
- Source: GNOME Settings (`gnome-control-center`)
- Upstream project: https://gitlab.gnome.org/GNOME/gnome-control-center
- Copyright: GNOME Project contributors
- License used for this asset record: GNU General Public License v2.0 or later (`GPL-2.0-or-later`), matching the upstream project license where the supplied SVG does not carry a separate embedded license declaration.
- Source file in Horizon: `ui/assets/settings-gear.svg`
- Derived use: vector path geometry is reproduced in `ui/components/utility-icons.slint`.
- Changes: geometry is reformatted for Slint; color is applied by Horizon's theme.

### Web — `org.gnome.Epiphany-symbolic.svg`
- Source: GNOME Web / Epiphany
- Upstream project: https://gitlab.gnome.org/GNOME/epiphany
- Attribution: GNOME Project contributors; the upstream icon import credited Jakub Steiner for the artwork.
- License used for this asset record: GNU General Public License v3.0 or later (`GPL-3.0-or-later`), matching the upstream project license where the supplied SVG does not carry a separate embedded license declaration.
- Source file in Horizon: `ui/assets/web-epiphany.svg`
- Derived use: vector path geometry is reproduced in `ui/components/utility-icons.slint`.
- Changes: geometry is reformatted for Slint; color is applied by Horizon's theme.

## Typography

## License texts

Corresponding license texts are included in `LICENSES/`:

- `CC0-1.0.txt`
- `CC-BY-SA-3.0-US.txt`
- `GPL-2.0-or-later.txt`
- `GPL-3.0-or-later.txt`

## Rust and native dependencies

Horizon also uses third-party software dependencies declared in `Cargo.toml` and the Flatpak manifest. Their own upstream licenses remain applicable. Before a public release, Horizon must commit a resolved `Cargo.lock` and generate a complete dependency-license report from that exact dependency graph. See `docs/LICENSING.md`.

This notice is intended to preserve provenance and attribution. It does not replace the applicable license texts.


### Presentation derivatives
Horizon uses lighter outline presentation derivatives for the Web and Settings utility glyphs so their optical stroke weight matches the other header utilities. The original supplied GNOME SVG files remain unchanged in `ui/assets/`; source, copyright, and license obligations continue to apply to the derivatives.

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
