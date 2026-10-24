# Flatpak packaging

The [manifest](io.github.Mars7x.Horizon.yml) is authoritative for app ID, runtime,
SDK, permissions and native modules. It builds the Rust/Slint Winit/Skia app,
SDL3 and bundled LINE Seed JP fonts. PipeWire and libjxl come from the runtime.
Use GNOME Builder with this manifest; [development](../docs/DEVELOPMENT.md) covers
checks and verification boundaries.

## Build

Install the matching Platform, SDK and Rust stable SDK extension on the target
architecture. The manifest appends `/usr/lib/sdk/rust-stable/bin` at manifest
scope so Builder dependency preparation and application builds use the same tools.
`bash scripts/build-flatpak.sh` builds and installs locally; it can install missing
SDK dependencies. x86_64 and aarch64 require separate successful builds/tests.

The development build uses Cargo.lock with `--locked`, and allows build-network
access to fetch missing dependencies. That is separate from runtime networking.
[sources generation](../scripts/update-flatpak-sources.sh) validates and consumes
the existing lockfile without updating versions. For release, generate and review
checksum-pinned offline Cargo sources, pin font downloads as Builder sources and
remove development fetch requirements. These release gates are not complete.

## Runtime sandbox

| Permission | Consumer |
| --- | --- |
| IPC and DRI | Windowing/rendering |
| Wayland socket | Normal desktop UI |
| Input devices | SDL controller navigation |
| Network | Steam profile/friends and independent SteamGridDB |
| PipeWire runtime socket | UI sounds and Album audio |
| NetworkManager system bus | Read-only physical-link status |
| UPower system bus | Read-only host/controller battery |
| BlueZ system bus | Read-only controller battery fallback |

No Steam/launcher filesystem grant, arbitrary host command, compositor/session
socket or broad home/host/device grant exists. XDG portals handle file selection
and normal browser links. The explicit finish-args policy is checked by
[repository contracts](../scripts/repository-contracts.json). Permission changes
need a real consumer, documentation and explicit policy review.

License texts/notices are installed with the binary. Never package a developer's
credentials, cached artwork or user media as source/build inputs.

Nintendo 3DS uses the separately approved Azahar Vulkan core; see
[3DS playback](../docs/THREE_DS_PLAYBACK.md) for import formats, controls, isolated saves and initial limits.
