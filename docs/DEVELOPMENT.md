# Developing and testing Horizon

This document gives reproducible commands for the source snapshot described in [PROJECT_STATE.md](PROJECT_STATE.md). It does not replace GNOME Builder's build output or claim that all targets have been verified.

## Toolchain and location

- `Cargo.toml` specifies Rust edition **2024** and `rust-version = "1.92"`; the project uses Slint **~1.18** with Winit/Skia, SDL3 (`pkg-config`), SQLite and XDG portals.
- The primary Flatpak manifest is `flatpak/io.github.Mars7x.Horizon.yml`: Freedesktop Platform/SDK **26.08** plus the **26.08 Rust stable SDK extension**. The Flatpak build includes SDL3; direct host builds need its development package and headers.
- The application ID is `io.github.Mars7x.Horizon`. Keep app IDs, data paths and desktop-entry values coordinated; do not rename them incidentally.
- Use the actual repository root containing `Cargo.toml`. The intended workflow is GNOME Builder with the Flatpak manifest; no Qt, GTK or Electron shell is involved.

## Quality gate

From the repository root:

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
bash scripts/check-third-party.sh
```

Or invoke all of the above with `bash scripts/check.sh`. The first three require a working host toolchain/dependencies; the final attribution check can be run independently. A `cargo fmt` invocation **without** `--check` modifies Rust files; do not run it as a documentation-only cleanup unless requested.

Build the Flatpak with `bash scripts/build-flatpak.sh` when the host has `flatpak-builder` and its dependencies. The helper currently installs locally (`--user`) and uses the manifest. GNOME Builder's own Flatpak build/run also works when the matching SDK is present.

## ARM (`aarch64`) GNOME Builder setup

The runtime architecture is chosen by Flatpak/Builder on the machine; do **not** edit the manifest's SDK branch to silence a missing-SDK error. On the ARM laptop, install the matching runtimes from Flathub:

```sh
flatpak install flathub org.freedesktop.Sdk//26.08 \
  org.freedesktop.Platform//26.08 \
  org.freedesktop.Sdk.Extension.rust-stable//26.08
flatpak list --runtime --columns=application,arch,branch
```

Look for `aarch64` on that machine. Download failures such as `Could not resolve hostname` or `Timeout was reached` are network/Flathub issues, not evidence of a Rust build failure. Retry once connectivity is restored. If SDK installation succeeds but compilation fails, capture the **first compiler error** and the source line, not only Builder's final failure summary.

## Architecture-sensitive testing

- **Rust/controller/navigation changes:** test Home/Back history, repeated D-pad/analog input, disconnected/reconnected controllers, pointer vs Accept parity, rapid route changes and page focus restoration.
- **Slint/layout changes:** test 720p, 1080p, ultrawide, and short windows, active vs contracted focus geometry, selected cover scaling, long-title fade/marquee, full-shell boundaries, and Reduced Motion. Respect Slint 1.18 syntax; static brace checks do not compile Slint.
- **Source/database changes:** test no-installed-games state, partial/unavailable snapshots, duplicates, historical/uninstalled titles, existing data upgrades and lifetime-vs-observed time separation. Released SQL migration files remain immutable.
- **Achievements/account changes:** test no credentials, invalid SteamID/key, private/unavailable profiles, account replacement/removal, stale in-flight responses, offline mode, and preservation of other settings. Never paste actual credentials in issues or screenshots.
- **Assets/licenses:** verify SVG crispness at fullscreen scaling, native colors, and `bash scripts/check-third-party.sh`.

## Known boundaries for contributors

- SDL3 comes from the Flatpak module; a direct host `cargo` build is not equivalent to a Flatpak build.
- The Flatpak development recipe may allow network access during **build** to fetch Cargo crates; this should be hardened into locked/offline sources before a public release. Network used by the running application for supported services is separate from build-network access.
- No build passed simply because source formatting, ZIP integrity, documentation links, or grep checks passed. Record exactly which tests ran and on which architecture.
- Do not add a token, Steam Web API key, private config JSON, `target/`, Builder output, or `.git/` to a patch archive.
