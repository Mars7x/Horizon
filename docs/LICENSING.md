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

## SVG-derived Slint geometry

Some symbolic SVGs are represented as Slint `Path` geometry because Slint's image path did not preserve the desired crisp recoloring behavior at fullscreen and HiDPI sizes. Reproducing path geometry is still use of the original artwork; the source and license therefore remain relevant even though the SVG is not rendered directly.

The original supplied SVG is retained in `ui/assets/` and the derived use is listed in `THIRD_PARTY_NOTICES.md`.

## Current mixed-license note

`applications-games-symbolic.svg` comes from the GNOME Symbolic Icon Theme under CC-BY-SA-3.0-US and its geometry is represented in `ui/components/system-icons.slint`. Keep that component's provenance intact. Before Horizon 1.0, perform a final compatibility review of this embedded derivative or replace it with an original/CC0 alternative if that simplifies distribution.

The Settings and Epiphany SVGs do not include a separate license declaration in the supplied files. Horizon currently records them under their upstream projects' default licenses (GPL-2.0-or-later for GNOME Settings and GPL-3.0-or-later for GNOME Web). Re-verify these exact assets against the exact upstream revision before 1.0.

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
