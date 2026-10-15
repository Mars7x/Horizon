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

## First-run runtime smoke test

In a Wayland-enabled SDK shell, run the opt-in setup test with throwaway paths:

```sh
onboarding_test_dir=$(mktemp -d /tmp/horizon-setup-test.XXXXXX)
env XDG_DATA_HOME="$onboarding_test_dir/data" \
    XDG_CONFIG_HOME="$onboarding_test_dir/config" \
    XDG_CACHE_HOME="$onboarding_test_dir/cache" \
    HORIZON_SETUP_TEST_DIR="$onboarding_test_dir" \
    cargo test --lib controller_setup_keyboard_and_restart_smoke \
    --offline --locked -- --ignored --nocapture --test-threads=1
```

Use a fresh directory for each run. The test writes synthetic profile/library
state and PNG captures into it; it does not launch a ROM or use real credentials.
`HORIZON_SETUP_TEST_DARK=1` exercises dark appearance and
`HORIZON_SETUP_TEST_REDUCED=1` disables motion. Hardware gamepad input, reconnects,
portal interaction and packaged Flatpak behavior still need manual checks.
