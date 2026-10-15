# ADR 0170: Album Save, Steam screenshot deletion, hardware video decoding

> **Save removed.** The user decided the Album doesn't need Save to Pictures.
> The Y/Insert action, `services::album::export`, the Pictures lookup and both
> Flatpak grants (`xdg-pictures/Horizon`, `user-dirs.dirs`) are gone. The rest
> of this ADR stands.

**Status:** Implemented. The build and gate pass, including a release build in GNOME Builder's Flatpak environment. The live Album harness covers saving and deleting a source's capture. Hardware decoding is untested on real VA-API hardware: this machine (Snapdragon X Elite, Adreno) has no VA-API decoder, so only the software fallback ran here.

## Context

The user asked for three things:
- saving captures to the Pictures folder;
- deleting Steam screenshots from the Album, reversing ADR 0166's "source captures are never deletable here";
- fixing the jump when a video opens.

Hardware video decoding was planned next (ADR 0169).

## Decision

**Save (Y / Insert).**
- `UiAction::Tertiary` (the Y button) copies the capture on screen, or the ticked ones while selecting, to `Pictures/Horizon`.
- Names are readable and never replace an existing file. The copy goes to `.part` first and runs off the UI thread (`services::album::export`).
- The Pictures folder comes from the desktop's `user-dirs.dirs`, since it may be localized (`data_paths::pictures_export_dir`).
- The Flatpak grants only `xdg-pictures/Horizon:create`, plus `xdg-config/user-dirs.dirs:ro`. Tested: writes outside that folder never reach the host.

**Steam deletion.**
- `CaptureSource::delete`, with each capture's `deletable` set by its owner.
- Steam lists its screenshots as deletable when their folder is writable. It checks with `access()`, because a read-only sandbox mount keeps its permission bits.
- `SteamCaptures::delete` accepts only a media file directly inside `userdata/<active account>/760/remote/<appid>/screenshots/` of a known root, after resolving links, and removes Steam's thumbnail with it.
- The Flatpak makes `Steam/userdata` writable (native and Flatpak Steam); the rest of Steam stays read-only. Flatpak can't narrow the grant further because the account id varies, so the code does.
- Steam's `screenshots.vdf` index is left to Steam.
- The confirmation says the files go "from Steam too".

**Album hints from Rust.** Y, X, B, A depend on the viewer, the selection and each capture's owner. The controller publishes `album-hints`, and only when the list changes.

**Thumbnail jump.** A video's thumbnail was a frame a second in, but playback starts at 0:00, so the picture jumped while the viewer grew open. The thumbnail is now the first frame, unless it's black (a fade-in), which keeps the old one-second frame. `ALBUM_CACHE_VERSION` moves to 2, which rebuilds thumbnails.

**Hardware decoding.**
- Playback asks FFmpeg for VA-API (AMD and Intel, the Steam Deck included), through one VA-API device shared by every video.
- `get_format` takes the GPU format only when FFmpeg offers it for the stream; otherwise it decodes in software on every core. A GPU decoder that fails to open also falls back.
- GPU frames are copied back (`av_hwframe_transfer_data`) and scaled as before.
- Posters stay in software: one frame isn't worth starting the GPU.

## Consequences

- Two new write grants, both narrow and documented in the manifest and in STORAGE.
- Steam deletion is permanent and visible in Steam.
- A zero-copy GPU path (scaling on the GPU, or handing textures to Slint) remains possible behind the same trait.
