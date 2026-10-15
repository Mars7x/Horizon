# Development

Minimum Rust: `1.92`
Rust edition: `2024`
Flatpak runtime: `26.08`

[Cargo.toml](../Cargo.toml) and the
[Flatpak manifest](../flatpak/io.github.Mars7x.Horizon.yml) are authoritative.
Use GNOME Builder with that manifest for the normal iterative build. Native
builds also need SDL3, a C compiler, pkg-config, FFmpeg development libraries
(libavformat, libavcodec, libavutil, libswscale, libswresample) and PipeWire
(libpipewire-0.3). Slint uses Winit/Skia, not Qt.

## Checks

The offline repository gate requires Python 3.11+ and Bash, with no additional
Python packages or network access:

```sh
bash scripts/check-repository.sh
```

It validates current documentation, source/manifest contracts, migration
integrity, its own regression tests, retirement SQL and attribution. CI runs
this gate on pushes and pull requests. It does not compile Rust or Slint.

The full developer gate requires a supported Rust toolchain, rustfmt, Clippy
and native dependencies:

```sh
bash scripts/check.sh
```

That runs the repository gate plus:

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

A missing tool, old edition support or missing native library is a failed or
unavailable check, never a pass. Do not weaken edition/minimum-version settings
to accommodate an outdated local installation.

## Flatpak and architecture verification

[Packaging notes](../flatpak/README.md) describe SDK setup and source-generation
limits. `bash scripts/build-flatpak.sh` builds and installs the development
Flatpak for the local architecture. It can install SDK dependencies, so read
the helper before invoking it. Successful SDK installation is not compilation.

Build and exercise both x86_64 and aarch64 before claiming support. Record the
first compiler/Slint error when a build fails; DNS/download failures are separate
from compilation. Historic build claims, including the legacy tuple-comparison
fix, require actual build evidence rather than inference from source cleanup.

## Runtime review

Check startup, missing/private data, existing-database upgrades, controller
reconnection, keyboard/mouse parity, Back/Home/Forward, rapid route changes,
720p/1080p/ultrawide resizing, fullscreen, high contrast and reduced motion.
Verify profile account switching, private friends, SteamGridDB failures and
Album seeking/deletion without exposing credentials in logs or screenshots.
