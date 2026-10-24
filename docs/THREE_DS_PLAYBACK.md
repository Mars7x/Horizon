# Nintendo 3DS playback

Horizon bundles unmodified Azahar 2126.2, pinned to
`219d743a1e698101fe1ab8bef43e6563126c3c27`, built with Vulkan enabled and
OpenGL/software rendering disabled. Vulkan 1.1 or newer is required. Existing
mGBA and melonDS renderers keep their current behavior. No additional Flatpak
permissions, standalone emulator, keys, system-file manager or CIA installer
are introduced.

## Import and launch

The automatic folder importer recognizes uncompressed `.3ds`, `.cci`, `.cxi`
and `.3dsx` files. Cartridge/application headers must identify decrypted NCCH
content; encrypted files are rejected. Maximum content size is 4 GiB. 3DS has
filename-derived titles, not a bundled exact-match metadata catalog. Existing
SHA-256 identities, catalog indices, sessions and artwork boundaries apply.

Launch holds the file open and verifies it against the imported SHA-256 while
Azahar loads (it reads through its own reopened description, so neither moves
the other's position); nothing runs until the check passes. After a successful
hash, the file's identity (device, inode, size, modification and change times
to the nanosecond) is recorded in the game's `verified.json`; a later launch of
the same unchanged file skips the multi-second hash, and any difference hashes
again. A first launch of a 4 GiB cartridge is ready in about 3 s, later ones in
about 0.5 s ([ADR 0232](adr/0232-3ds-verified-file-identity.md)). Azahar
receives `/proc/self/fd/<fd>` rather than a multi-gigabyte memory copy. The descriptor stays open through core teardown, so path replacement does
not redirect the launched content. Concurrent in-place modification of the ROM
is not supported. Only the bundled pinned core can consume that descriptor.

## Controls and picture

Initial playback uses native 400×480 combined screens, default arrangement and
no filter. Controller D-pad and face/shoulder/Start/Select buttons retain the
Nintendo mapping; left and right sticks drive Circle Pad and C-stick separately
from D-pad. The global accept/cancel and X/Y layout swap still applies.
Keyboard I/J/K/L drives Circle Pad, arrows drive D-pad; existing Z/X/C/V,
A/S, Enter and Backspace bindings remain. Mouse/touch coordinates follow the
actual contained image, excluding letterbox bars; Azahar maps its lower screen.

Quick menu offers Continue, 1×/2× playback, Screenshot, Restart and Return to
Horizon. 3DS rewind, manual states and automatic resume are deliberately unavailable
in this first integration. In particular, Azahar's expensive state-size query
must never run periodically during gameplay. Upstream manual state capability
is not being described as unsupported by Azahar itself.

## Saves and compatibility

Azahar owns its files under each game's `core-data/Azahar/` folder, including
its isolated NAND/SDMC, system data and shader cache. It flushes in-game saves
through normal core shutdown. Save management opens the game folder, backs up
this tree and includes it in an explicitly confirmed progress reset. ROM
uninstall retains these files and the game's history. Other games' Azahar trees
are independent. No existing external emulator save directory is touched.

Azahar's own log lines carry their level (`<Warning>`, `<Error>`); routine ones
(stubbed services, files a fresh console lacks) are logged at debug, its
Vulkan renderer's errors stay visible.

Some games require user-dumped system data; Horizon does not discover or obtain
it. No 3DS system files or games are distributed. Compatibility is established
per tested game and architecture, not inferred from import success or compilation.

See [storage](STORAGE.md), [licensing](LICENSING.md),
[ADR 0231](adr/0231-azahar-vulkan-playback.md) and the delivery verification report.
