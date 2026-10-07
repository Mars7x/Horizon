# Licensing and attribution policy

Horizon is intended to keep third-party provenance explicit from the moment an external asset or code fragment enters the repository.

## Project license

Horizon application code is licensed under GPL-3.0-or-later. The complete GPL v3 text is available in `LICENSE` and `LICENSES/GPL-3.0-or-later.txt`.

## Third-party asset rule

No third-party asset may be merged without recording all of the following:

1. original filename or upstream identity;
2. source project and source URL where known;
3. author or copyright holder where known;
4. exact SPDX license identifier;
5. whether Horizon changed, recolored, reformatted, traced, or otherwise derived from it;
6. the corresponding license text in `LICENSES/` when redistribution requires or benefits from it;
7. a human-readable entry in `THIRD_PARTY_NOTICES.md`.

If the exact asset license cannot be established, it must be marked as a release blocker rather than guessed.

## SVG artwork

The six top-utility icons (Friends, Album, Activity, Web, Settings, and Shop) are Horizon-owned artwork supplied by the project owner. They are stored as SVG files in `ui/assets/` and rendered directly by Slint. Their authored paths, fills, strokes, filters, and colors are not transcribed or recolored at runtime.

The application icon at `data/io.github.Mars7x.Horizon.svg` is also Horizon-owned artwork supplied by the project owner and is covered by the project-owned REUSE annotation.

`applications-games-symbolic.svg` remains third-party GNOME artwork under CC-BY-SA-3.0-US and its geometry is represented in `ui/components/system-icons.slint`. Keep that component's provenance intact. Before Horizon 1.0, perform a final compatibility review of this embedded derivative or replace it with an original/CC0 alternative if that simplifies distribution.

## Software dependency notices

During development, Cargo dependencies are not yet frozen for release. Before a public release:

- commit `Cargo.lock`;
- generate a license inventory from that exact lockfile using a tool such as `cargo-about` or an equivalent audited workflow;
- review every resolved license for GPL-3.0-or-later compatibility;
- include the generated notice with release artifacts;
- keep native Flatpak modules such as SDL3 covered by their upstream license files and source metadata.

This is a release gate, not optional cleanup.

## Flatpak installation

The Flatpak installs Horizon's project license, third-party notice, and bundled license texts under `/app/share/licenses/horizon/` and `/app/share/doc/horizon/` so redistributed binaries retain the same information as the source tree.


## Bundled fonts

LINE Seed JP is bundled in Flatpak builds under the SIL Open Font License 1.1. The manifest pins the upstream source revision, builds the original font sources without modification, and installs all four faces into `/app/share/fonts/truetype/line-seed-jp`. Keep the copyright notice `© LY Corporation`, `THIRD_PARTY_NOTICES.md`, and `LICENSES/OFL-1.1.txt` with distributed builds.


## Phase 5 SQLite dependency

Horizon uses `rusqlite`/`libsqlite3-sys` under the MIT license. Development currently enables rusqlite's `bundled` feature, which compiles the SQLite core into Horizon; the SQLite core is dedicated to the public domain. These dependencies are recorded in `THIRD_PARTY_NOTICES.md`. The final release artifact must still include a license inventory generated from the exact committed `Cargo.lock` rather than treating this hand-written note as the complete Rust dependency report.

## Phase 7 Steam parser dependency

The Steam source adapter uses `steam-vdf-parser` 0.1.2 to read Valve Data Format metadata locally, including current binary `appinfo.vdf` v40/v41 files. Upstream licenses the crate under **Apache-2.0 OR MIT**. Horizon does not copy parser source into its own modules; it consumes the published Cargo crate unchanged.

`THIRD_PARTY_NOTICES.md` records the upstream project and copyright. `LICENSES/Apache-2.0.txt` and `LICENSES/MIT.txt` are installed with Flatpak artifacts. This hand-written record does not replace the lockfile-derived release inventory.

## Phase 9.5.29 artwork dependencies and provider artwork

Horizon now uses `image` 0.25.x directly to decode provider-owned local
JPG/PNG/ICO artwork and normalize it to the square presentation contract.
Upstream licenses the crate under **MIT OR Apache-2.0**.

Horizon also uses `zip` 8.6.x with only Deflate support enabled to read Steam's
local `linuxclienticon` ZIP containers in memory. Upstream licenses `zip` under
**MIT**. Horizon does not extract those archives to disk and consumes the
published crate unchanged.

The existing MIT and Apache-2.0 license texts cover these dependencies, and the
final lockfile-derived inventory remains mandatory.

Steam game artwork is not part of Horizon's source distribution. Horizon reads
artwork already cached by the user's Steam installation and displays it locally.
Copyright and other rights in those game images remain with the respective game
publishers/developers. Horizon must not copy those cached images into release
artifacts or treat them as project-owned artwork.
