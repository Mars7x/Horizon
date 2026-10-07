# Flatpak notes

Horizon is developed and tested as a Flatpak from the beginning.

Horizon uses Slint's Winit backend with Wayland and X11 support plus the
Skia renderer. No Qt runtime is required. XDG Settings portal access for
theme, accent, contrast, and reduced-motion preferences does not require broad
D-Bus permissions in the manifest.

## Development Cargo strategy

Phase 3 is still an active development scaffold and does not yet commit a
`Cargo.lock` plus generated `cargo-sources.json`. Therefore the development
manifest explicitly gives the **build sandbox only** network access:

```yaml
build-options:
  append-path: /usr/lib/sdk/rust-stable/bin
  build-args:
    - --share=network
```

This does **not** grant Horizon runtime network access. It only lets Cargo fetch
and resolve crates while GNOME Builder or `flatpak-builder` compiles the app.

Do not set `CARGO_NET_OFFLINE=true` or pass `cargo --offline` until the repository
contains a current `Cargo.lock` and `flatpak/cargo-sources.json`. Doing so causes
fresh Builder environments to fail with errors such as:

```text
error: no matching package named `ashpd` found
note: offline mode ...
```

Before a Flathub/release build, switch back to the reproducible offline path:

1. Generate and commit `Cargo.lock`.
2. Generate `flatpak/cargo-sources.json` with `flatpak-cargo-generator.py`.
3. Add `cargo-sources.json` to the Horizon module sources.
4. Remove `build-args: --share=network`.
5. Build with `cargo --offline build --release --locked`.

`scripts/update-flatpak-sources.sh` exists for step 2 once the dependency graph
has successfully resolved.

## Rust SDK extension and GNOME Builder

The manifest declares `org.freedesktop.Sdk.Extension.rust-stable` and adds
`/usr/lib/sdk/rust-stable/bin` to `PATH` at manifest scope. Keep it there:
GNOME Builder prepares dependency modules with `flatpak-builder --stop-at=horizon`
and then invokes Cargo itself, so a module-local PATH modification is too late.

The Rust extension branch must match the Freedesktop runtime branch (`26.08`).

```bash
flatpak info org.freedesktop.Sdk.Extension.rust-stable//26.08 || \
  flatpak install --user flathub org.freedesktop.Sdk.Extension.rust-stable//26.08
```

After manifest changes, clean/rebuild in GNOME Builder so it recreates the
Flatpak build environment.

## SDL3

Phase 3 builds SDL 3.4.18 as an explicit module. The Rust `sdl3` crate links to
it using `pkg-config`.

The runtime sandbox receives only:

```text
--device=input
```

for `/dev/input` controller access. Do not broaden this to `--device=all` for
normal navigation.

## Steam metadata access and launching

Phase 7 deliberately avoids broad filesystem permissions. The manifest exposes conventional native-Steam XDG data roots read-only and grants the Flatpak Steam app-data root `~/.var/app/com.valvesoftware.Steam:ro`. The latter is intentionally one app-scoped subtree instead of a collection of deeper child grants because Flatpak Steam may resolve its canonical metadata directory across `.local/share` and `data`. Horizon itself still reads only Steam metadata (`libraryfolders.vdf`, `appinfo.vdf`, and accessible app manifests) from that subtree.

External Steam library mount points are not granted automatically. Modern `libraryfolders.vdf` provides installed app IDs, so Horizon can normally discover games on those libraries without reading `/mnt`, `/run/media`, or other arbitrary storage paths.

Do not replace these Steam-scoped rules with `--filesystem=home` or `--filesystem=host` as a convenience fix.

Launching does not require access to `/usr/bin/steam` or the Flatpak Steam command. Horizon passes `steam://rungameid/<appid>` to `org.freedesktop.portal.OpenURI` through `ashpd`; the host session resolves the URI handler.
