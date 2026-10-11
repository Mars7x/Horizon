# ADR 0176: Profile picture

> **Partly superseded by ADR 0177:** the controls moved from Settings → Profile to a Profile page opened from Home's avatar. The square Steam picture and the Steam avatar frame were then removed entirely (ADR 0177), so `ProfileSource::Steam` has no `square`, `profile-steam-frame` is no longer written, and animations decode at 184 px. The other picture sources, storage, Steam fetching and animation below still stand.

**Status:** Implemented. The build, the gate and the new tests pass (centre-square crop, scaling, save, load and remove round trip, unreadable files, `file://` URIs to paths). Settings → Profile and Home were rendered offscreen. The FileChooser portal hasn't been exercised in the running app yet.

## Context

The user is redesigning Home's top-left profile after a reference mockup: a round picture in a thick ring, white in light mode and gray in dark. Asked where the picture should come from, the user chose to pick it in Horizon, over the desktop account picture, the Steam avatar, or keeping a monogram.

## Decision

- **Frame.** `components/profile-avatar.slint` draws a circle in a solid ring: white in light mode, #55555e in dark, the foreground in High Contrast. It is lifted by a soft shadow and is 54 px in the top bar, with the ring and monogram scaling with `size`. The notification dot is gone.
  - Without a picture, a monogram sits on a flat accent fill. A gradient was dropped because the offscreen renderer can't draw one in a rounded child, and flat matches the mockup.
- **Choosing.** Settings gains **Profile** as its first row, with **Choose picture** and **Remove picture**.
  - Choosing runs the XDG FileChooser portal (`platform::file_chooser`, ashpd's `file_chooser` feature) on a worker thread. This avoids any filesystem grant, and the dialog filters to PNG, JPEG, GIF and JPEG XL.
- **Horizon's own copy.** `services::profile` decodes the chosen file through `services::image`: by content, under its limits, using the first frame of an animation. It crops the centre square, scales it to 256 px (Lanczos) and saves `data/profile.png` (`data_paths::profile_picture_path`).
  - The file is written to a `.part` and renamed, so a failed save never leaves a broken picture.
  - The original's metadata isn't kept, and the original may move or disappear, which document-portal paths do after the session.
- **Presentation.** `presentation::profile::ProfileController` loads the stored picture on a worker at startup, runs picks one at a time, and publishes `profile-picture` / `profile-has-picture` for the top bar and Settings, with a line of feedback.
  - Removing deletes the file; a missing file is fine.
  - Settings' row indices moved: Profile 0, Appearance 1, Third-Party 2. Back restores the right parent row.

## Consequences

- The portal dialog is not parented to Horizon's window (no window identifier is passed), so the compositor places it on its own.
- No account or network is involved. A future Steam Friends feature could offer the Steam avatar as another source.

## Follow-up: Steam picture, square with frame, built-in avatars, ring depth

- **Source.** `profile.json` (config) records which picture is shown: `ProfileSource::Chosen` (the default; the monogram without a chosen picture), `Steam { square }`, or `Avatar { id }`. Choosing a picture switches back to Chosen. Each source keeps its own copy in data (`profile.png`, `profile-steam.png`, `profile-steam-frame.png`), so switching is instant and Steam's shows offline.
- **Steam picture.** `SteamWebApiClient::profile_avatar` asks `ISteamUser/GetPlayerSummaries` for `avatarfull` (else `avatarmedium`). It downloads only from Steam's avatar hosts, over HTTPS, with no key, under 1 MB and at most 1024 px a side, then squares the picture like any other. Network errors are reported `without_url()`, because the Web API URL holds the key.
  - A switch in Settings turns it on once a Steam Account is set up. It is refreshed quietly at startup, and again when the account is saved.
- **Square, with Steam frame.** A second switch shows the Steam picture square, as Steam does. `profile_frame` asks `IPlayerService/GetProfileItemsEquipped` for `avatar_frame` and accepts only a plain `items/...` path, which it joins onto Steam's community image address. It downloads under 4 MB from the badge hosts and keeps the frame whole (first frame of an animated PNG).
  - The frame is drawn at the avatar's full size, around a picture 1/1.22 of it. A frame that can't be fetched keeps the last one, and one no longer equipped is removed.
- **Built-in avatars.** `services::profile::AVATARS` is an empty, compiled-in catalogue: `id` (stable, saved), `name`, and `picture` bytes via `include_bytes!`. `ProfileSource::Avatar` shows one, decoded once and cached. A test keeps ids unique and pictures decodable. Settings offers no avatar picker until the catalogue has art.
- **Ring.** The user found the flat ring, and the dark gray, too plain next to the mockup. The ring now has a lit top edge, a fine outline, a deeper shadow and an inset rim, and is a light gray in dark mode, chosen from three rendered options while the user was away.

## Follow-up: animated pictures and frames

The user kept the bevelled ring after comparing it with a flat thick ring, a thin ring, and no ring. They asked for animation where Steam supports it, and for chosen GIFs too.

- **What plays.** A Steam animated avatar (`animated_avatar` from GetProfileItemsEquipped, usually a GIF), an animated avatar frame (usually an animated PNG), and a chosen animated GIF or PNG.
- **Storage.** Each is kept as its original file (`profile-steam-animated`, `profile-steam-frame`, `profile-animated`), so it survives offline and restarts. A chosen one also keeps its still `profile.png`.
  - `SteamWebApiClient::profile_items` reports each item as not equipped, downloaded (8 MB cap), or failed. A failed download keeps the earlier copy rather than deleting it.
- **Decoding.** `services::image::small_animation` decodes GIF and APNG whole, through the usual limits, at most 600 frames, each fitted to 128 px (frames 160 px). Chosen pictures are cropped to the centre square. Frame delays under 20 ms play at 100 ms, the same rule as the Album.
  - The decoded animation is a few MB at most, built on a worker. Decoding up front, rather than streaming as the Album viewer does, suits a picture this small that loops forever.
- **Playback.** `AnimatedPicture` (Slint) steps through `[ProfileFrameData]` on a Timer while `playing`: Home's bar while it's shown, or the Settings → Profile preview. It shows the still with Reduced Motion and while Horizon isn't the active window (`Motion.loops-paused`), so a backgrounded Horizon does no work for it.
- **Small, not large.** For both items, Steam's `image_small` is the animated file (a GIF; an animated PNG) and `image_large` a still preview (a JPEG; a single-frame PNG). That was confirmed against a real account: an 85-frame GIF avatar and a 6-frame APNG frame. Horizon first took `image_large`, so nothing moved. It now prefers `image_small` and falls back to `image_large`. Copies already kept are replaced on the next Steam refresh, at startup or when the switch is turned on.
