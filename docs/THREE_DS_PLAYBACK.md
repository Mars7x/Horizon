# Nintendo 3DS playback

Horizon bundles Azahar 2126.2, pinned to
`219d743a1e698101fe1ab8bef43e6563126c3c27`, built with Vulkan enabled and
OpenGL/software rendering disabled, with one patch: its libretro core can turn
on Azahar's own asynchronous shader compilation, which Horizon does
([ADR 0238](adr/0238-azahar-async-shaders.md)). A game's new effects then no
longer stop the frame while their shaders compile (one may be missing for a
moment instead), and each game's shader cache keeps them for later. Vulkan 1.1 or newer is required. Existing
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

Azahar draws its default arrangement (the 400×240 top screen above the 320×240
bottom one) with no filter; Horizon cuts out the two screens and places them
like a DS game's. Settings → Cores → Azahar → Screen layout chooses Stacked
(default), Side by side, or Large and small, keeping each screen's own shape.
The quick menu's Screen layout and Swap screens, and L3, change them during
play; as on the DS, the game remembers its layout and swap in `view.json` and
then ignores the default. Screenshots show the screens as arranged. Internal
resolution renders at Native or 2× to 6× (`cores/azahar.json`, next launch). Frames stay on
the graphics card until shown: each screen is then shrunk there to the size it
is shown at (exact halvings, then one linear step) and only that is read back,
into CPU-cached memory. Screenshots read the full frame. At 6× this keeps 60
frames a second where reading every 27 MB frame managed about 50
([ADR 0235](adr/0235-3ds-gpu-viewport-readback.md)).
Remaining one-frame pauses (tens of milliseconds, the same at every
resolution) are the game loading or setting up a scene inside Azahar.

Controller D-pad and face/shoulder/Start/Select buttons retain the
Nintendo mapping; left and right sticks drive Circle Pad and C-stick separately
from D-pad. The global accept/cancel and X/Y layout swap still applies.
The right stick is only the C-stick (Azahar's touch-cursor mode is off); L3
swaps the screens and R3 does nothing. Keyboard I/J/K/L drives Circle Pad, arrows drive D-pad;
existing Z/X/C/V, A/S, Enter and Backspace bindings remain. Mouse
and touch work on the bottom screen wherever the layout puts it; Azahar reads
it once, as a touch.

Quick menu offers Continue, 1×/2× playback, Screen layout, Swap screens, Save
state, Load state, Screenshot, Restart and Return to Horizon (no lid, no
rewind).

## Save states

3DS games have the four manual slots, quick save/load and the automatic resume
point, using Azahar's own states (about 7 MB, compressed) in the shared bundle
format under `states/azahar`, with a 400×480 preview of both screens. A state
takes Azahar about a second to save and a little under a second to load at any
internal resolution: the quick menu says "Saving state…" / "Loading state…"
meanwhile, and the game's audio pauses rather than running dry. Return to
Horizon writes the resume point (exit takes about a second longer), and with
Resume last state on, a launch restores the newest point behind the usual
loading picture.

Because each state costs a second, nothing takes one in the background: no
rewind, no timed or focus-loss autosaves, and Azahar's size query (which builds
a complete state) is never polled. Loading takes no rollback state first; the
bundle is checksummed, Azahar checks the game and build before reading it, and
a load that fails ends the session as on other systems. Azahar rejects states
from any other build, so a core update makes older 3DS states unloadable
([ADR 0211](adr/0211-states-follow-the-core-release.md),
[ADR 0237](adr/0237-3ds-save-states.md)).

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

Mods in Azahar's LayeredFS layout are managed from the game menu; see
[game options](GAME_OPTIONS.md#manage-mods).

Some games require user-dumped system data; Horizon does not discover or obtain
it. No 3DS system files or games are distributed. Compatibility is established
per tested game and architecture, not inferred from import success or compilation.

See [storage](STORAGE.md), [licensing](LICENSING.md),
[ADR 0231](adr/0231-azahar-vulkan-playback.md) and the delivery verification report.
