# ADR 0177: The Profile page

**Status:** Implemented. The build, the gate and the new tests pass (avatar focus in the top row, the left edge, focus memory, the Profile route and its history, names and monograms, preferences). The page, the picture dialog, the name editor, Home's focused avatar and Settings were rendered offscreen, light and dark. Not yet used in the running app.

## Context

ADR 0176 put the profile picture in Settings → Profile. The user wanted it to work differently: click the avatar at the top left and reach a profile page where the name and picture are changed, designed well and consistent with the rest of Horizon. Asked, they chose a typed name only (not Steam's), and only name and picture on the page.

## Decision

- **A route of its own.** `AppRoute::Profile` (AppRouteView `profile`) is a full-shell page with history, entered fresh. It is not a `TopUtility`.
- **The avatar is focusable.** `ShellFocus::profile` marks it within the `TopUtilities` region, as the row's left end.
  - Left from Friends reaches it, held or fresh; Left on it does nothing; Right returns to Friends. So the row no longer wraps at the left, while the right end still wraps to Friends.
  - Down goes to the games, and a later Up returns to Friends. A opens the page, and a click does too.
  - `FocusSnapshot` records it. The avatar uses the utilities' focus ring and lift, and while it has focus the published utility index is -1.
- **The page** (`pages/profile.slint`) shows the profile, not its options: the avatar large (184 px), the name, where the picture comes from, and two buttons, Change picture and Edit name. A first version opened straight onto Settings-style option rows beside a profile card; the user didn't want the picture's settings to be the first thing on the page, so they moved into a dialog.
  - **Change picture** opens a dialog: a live preview beside Choose a picture, Use Steam picture and Remove chosen picture, then Done. Its rows are `SettingsRow`, which moved to `components/settings-row.slint` so Settings and Profile share it. Up and Down skip options that can't be used now.
  - **Edit name** opens the name editor (text, Cancel, Save).
  - `ProfileController` owns the page selection, which dialog is open and the selection inside it, and reports `ProfileFeedback`, so navigation plays the sounds. Hints are Back/Select on the page, Close/Select in the picture dialog and Cancel/Select in the name editor.
- **No square picture, no Steam frame.** The avatar's focus ring is a circle, and a square framed Steam picture didn't fit it. Rather than give the avatar a second shape, the square option and the Steam avatar frame were removed: `ProfileSource::Steam` is a unit variant (old files with `square` still read), `SteamWebApiClient::animated_avatar` replaces `profile_items`, and `profile-steam-frame` is no longer written. Animated pictures now decode at 184 px, Steam's own animated avatar size, so the page's larger avatar stays sharp.
- **Name.** `profile.json` gains `name` beside the flattened `source`, so old files still read. `clean_name` collapses whitespace and keeps at most 32 characters. `monogram` gives the avatar its first letter, else "H".
- **Settings** goes back to Appearance and Third-Party. Its Profile section from ADR 0176 is removed (superseded here).

## Consequences

- The Wayland paste workaround used by Settings' key editor is not applied to the name field. Typing and the toolkit's own paste work; Ctrl+V may not on some compositors.
- The navigation sound's focus snapshot gains the avatar focus, the page selection, the picture dialog's selection and the editor target.
- An animated picture now takes up to about 80 MB at the 600-frame cap (184 px frames), up from about 40 MB; typical Steam avatars are under 100 frames.
- A `profile-steam-frame` file left from before stays in the data folder, unused.
