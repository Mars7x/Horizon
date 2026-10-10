# Flatpak build notes

Horizon is Flatpak-first. The active manifest is [`io.github.Mars7x.Horizon.yml`](io.github.Mars7x.Horizon.yml), with application ID `io.github.Mars7x.Horizon`, `org.freedesktop.Platform`/SDK **26.08**, and `org.freedesktop.Sdk.Extension.rust-stable` on the **matching 26.08 branch**. Rust uses the Slint Winit/Skia backend; Qt is not required. SDL3 is built as a Flatpak module and linked using `pkg-config`.

See [Development](../docs/DEVELOPMENT.md) for build/test commands, including `aarch64` GNOME Builder preparation and how to distinguish missing SDKs/network errors from actual Rust/Slint errors.

## Builder and Rust SDK extension

The manifest appends `/usr/lib/sdk/rust-stable/bin` to PATH at manifest scope. Keep it there: GNOME Builder prepares dependencies with `flatpak-builder --stop-at=horizon` before invoking Cargo, so a module-local PATH change would be too late.

On the target machine:

```sh
flatpak install flathub org.freedesktop.Sdk//26.08 \
  org.freedesktop.Platform//26.08 \
  org.freedesktop.Sdk.Extension.rust-stable//26.08
```

Flatpak chooses the local architecture (`aarch64` on ARM, `x86_64` on x86). Verify with `flatpak list --runtime --columns=application,arch,branch`. If a package download fails due to DNS or timeout, finish the dependency install before investigating compilation.

## Development Cargo sources vs release policy

- `Cargo.lock` **is already present**. Older phase notes saying it is not committed are obsolete.
- The current development manifest still permits **build-time** Cargo network access so GNOME Builder can fetch missing crates while compiling. That is separate from the application's intended runtime network permissions.
- The repository also has `scripts/update-flatpak-sources.sh` for generating offline crate sources. A release-quality manifest must use a lockfile-pinned, checksum-pinned source set, stop relying on build-network access and use reproducible/locked Cargo invocation. Do not claim that release-hardening is complete just because `Cargo.lock` exists.
- Verify the source archive/checksums and module availability on both `x86_64` and `aarch64` before distribution. Development packaging is not a Flathub-readiness guarantee.

## Flatpak permissions and source access

Respect the narrow declared filesystem/input/D-Bus permissions. Steam and Heroic local discovery paths are source-specific; do not replace them with broad host filesystem or `--device=all` access to solve an import problem. Treat achievement Web API network access and optional Steam-account storage separately from local Steam installation detection.

For local build/install, see `bash scripts/build-flatpak.sh` from the repository root. Use GNOME Builder for the normal iterative developer workflow.
