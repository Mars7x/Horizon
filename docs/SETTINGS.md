# Settings

[SettingsController](../src/presentation/settings.rs) and
[the page](../ui/pages/settings.slint) expose Appearance, Controller and
Third-Party. A fresh visit selects Appearance. Back from a sub-page restores its
parent row; full departure/re-entry resets the root selection.

## Appearance and controller

Theme is System/Light/Dark/Black. Accent is independent: System plus nine color
presets. UI Sounds is independent of both. Choices apply immediately and persist
atomically; a failed save must not silently replace the previous preference.
Focus is distinct from the selected value. See [UI standards](UI_STANDARDS.md).

Controller has Swap Select and Back, plus Swap Hints. The first changes controller
actions only; the second determines whether hints follow the swap. Keyboard
Enter/Escape remains unchanged. Values live in `controller.json`.

## Third-party services

SteamGridDB and Steam Account have separate service pages, credentials and
storage. SteamGridDB offers its API key, artwork preference and cover refresh.
With no active games there are no cover jobs. Refresh runs off-thread and does
not overwrite explicit user choices. See [Artwork](ARTWORK.md).

Steam Account stores a validated SteamID64 and personal Web API key. It supplies
profile pictures and read-only friends only; no Steam screenshots or achievements
remain. This is manual account configuration, not OAuth or interactive login.
Disconnecting removes the saved account and updates retained account features;
inherited developer environment credentials may still exist.

Keys are masked during editing and never placed in general Slint state, logs,
request-URL errors or documentation. Private permissions are not encryption.
Service workers receive only short-lived copies needed for requests. Replacing an
account must reject stale results from the old account.

Remove/Disconnect is a trailing secondary action, with X as its controller
alternative, not a separate settings row. Confirm with Cancel initially focused.
Editors use the normal text field and [on-screen keyboard](INPUT.md). Invalid
values, failed saves and network errors must preserve useful prior state and
other preferences. See [Storage](STORAGE.md).
