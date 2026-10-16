# Licensing

Horizon code is GPL-3.0-or-later, with complete text in [LICENSE](../LICENSE)
and [LICENSES](../LICENSES/). [REUSE annotations](../REUSE.toml) and
[third-party notices](../THIRD_PARTY_NOTICES.md) preserve asset/dependency provenance.
Do not remove old attribution or license texts because a feature was retired.

Every new external asset/code fragment needs upstream identity/URL, author where
known, exact SPDX license, modification/derivation status, the required license
text and a human-readable notice. Unknown licensing is a release blocker, not
permission to guess. New dependencies need a demonstrated consumer and review.

## Bundled assets

- Utility/application SVGs are project-owned authored artwork; preserve their
  geometry, colors and rendering contracts unless editing was requested.
- Controller drawings from Kenney Input Prompts are CC0 and bundled unmodified;
  the Odin drawing is project-owned. Keep the asset-level annotations/notices.
- LINE Seed JP static fonts are unmodified OFL-1.1 assets, with © LY Corporation
  and credited designers retained. Font URLs/revisions live in the packaging
  installer/manifest, not duplicated across feature guides.
- UI WAVs and native media test fixtures retain their documented attribution.
- The offline GBA hash/title catalog is derived from pinned No-Intro/Libretro
  metadata under CC-BY-SA-4.0, with its license and derivation in the notices.
- FFmpeg, PipeWire, libjxl, SDL3, SQLite and all resolved Rust dependencies need their
  upstream license obligations respected. Hand-written notices are not a full
  lockfile-derived release inventory.
- The separately bundled mGBA 0.10.5 core is built from unchanged source under
  MPL-2.0, with its inih, blip-buf and libretro-header notices/licenses retained.
  Source release and build checksum are recorded in the manifest and notices.
  The dynamic libloading dependency is used only for the packaged core ABI.

[The attribution check](../scripts/check-third-party.sh) verifies required texts
and bundled-asset coverage. Removed direct dependencies may keep historical
notices; that does not make them active build requirements.

The Smooth pixel art filter uses unmodified xbrz-rs 0.1.0 under GPL-3.0-only,
compatible with distribution of the combined application under GPLv3. The GBA
sRGB colour profile is a CPU adaptation of the credited public-domain shader;
its original license statement is retained alongside the GPLv3 text. See the
video-treatment entries in the notices for authors, source and modifications.

## Distribution

Flatpak installs project licenses, bundled license texts and notices alongside
the binary. Before release, generate an inventory from the exact committed
Cargo.lock, check compatibility, include required native-module notices, and use
checksum-pinned source artifacts. Never package user artwork, cached credentials
or media from the development machine. Current development packaging is not a
claim of completed release hardening.
