# Profile — current state

The profile is the user's name and picture on this device, with their Steam friends beside them (ADRs 0176, 0177, 0178). It is not an account: the name and picture never leave the device, and the friends are only read from Steam.

## Opening it

- **Pointer:** click the avatar at the top left of Home.
- **Controller or keyboard:** the avatar is part of the top row, to the left of the first utility. Left from Album reaches it, held or not, and the row ends there (the top row never wraps; Shop is its right end). Right goes back to Album, Down goes to the games, and A opens the Profile page.
  - On the avatar it gets the utilities' single-stroke focus ring and their 2 px lift, and no utility shows focus.
  - Up from a game it was left from returns to Album.
- The **Profile page** (`AppRoute::Profile`) is a full-shell page with history: Back returns where you were, and Forward reopens it. Every visit starts with nothing open and the friends list at its top.

## The page

The page is the profile itself, not a list of options (`pages/profile.slint`). Under the shared header, a card on the left holds the avatar large (168 px) with the name beneath it ("No name yet" without one), and the Steam friends fill the right (see Friends below). A line under the name shows problems only ("Can't reach Steam right now").

The avatar is the page's one focus, with the same single-stroke ring and 2 px lift as Home's. A (or clicking the avatar or the name) opens **Edit profile**; Up and Down scroll the friends list. Hints are Back / Edit profile.

### Edit profile

A dialog with a live preview of the avatar on the left and the options on the right, in Settings' row style (`components/settings-row.slint`), then **Done**:

| Option | Does |
| --- | --- |
| Name | Opens the name editor over the dialog (below). |
| Choose a picture | The FileChooser portal. Any PNG, JPEG, GIF or JPEG XL becomes Horizon's square copy, and an animated one plays. While a chosen picture exists, the row has a small **Remove** button, and X removes it too (hint "X Remove"), after a "Remove your picture?" confirmation, deleting Horizon's copy. |
| Use Steam picture | A switch, usable once the Steam Account is set up in Settings → Third-Party. It uses the Steam avatar, animated if it is. |

The preview changes as soon as an option does, and only problems ("Couldn't use that image") show beside Done; a change that worked shows itself. Up and Down move through the options that can be used now, then Done; an option that stops being usable (Steam turned off without an account) hands the selection on. A does the option; B or Done closes the dialog. Hints are Close/Select.

### The name editor

A dialog with the text field, Cancel and Save, over Edit profile. The field takes focus with the name selected. Enter or Save keeps it; Escape, B or Cancel closes it, back to Edit profile. Names are tidied (single spaces, at most 32 characters); empty means no name. Down from the field goes to Save, Left and Right move between Cancel and Save, and Up returns to the field. Hints are Cancel/Select.

### Friends

The right of the page lists the Steam account's friends (`presentation/friends.rs`, `services/friends.rs`), under the Friends icon with "N online · M friends". Each row shows the friend's Steam avatar (their initial until it arrives), name, and what they're doing: "Playing *game*" in green, Online in the accent, Away or Busy in amber, and offline friends greyed with "Last online 3 days ago" when Steam says. In-game friends come first, then online, busy, away and offline, by name.

- **Read-only.** No row takes focus, so A never acts on a friend. Up and Down scroll the list a row at a time; the wheel and touchpad pan it like any other list (`ScrollCamera`, with the edge shadows).
- **Where it comes from.** `SteamWebApiClient::friends` asks `ISteamUser/GetFriendList` (relationship `friend`), then `GetPlayerSummaries` for up to 100 friends a request, with the key from Settings → Third-Party → Steam Account. Avatars (Steam's 64 px) download four at a time from Steam's avatar hosts only, with no key, and are kept by URL for the session.
- **When.** Fetched when the page opens if the list is more than a minute old, and again when the Steam account changes. Nothing is stored on disk.
- **When there's no list,** the panel says why in a few words instead: "Connect Steam in Settings to see your friends", "Your Steam friends list is private" (Steam answers 401), "Can't reach Steam right now" (no earlier list), or "No friends yet". While the first list loads, the panel is empty. A private or unreachable list is never shown as zero friends; an unreachable refresh keeps the last list.

## The avatar

Without a picture, the avatar shows the first letter of the name (Horizon's "H" without one). Its look, sources, storage and animation are described in [HOME_UI.md](HOME_UI.md) ("Profile avatar") and ADR 0176.

`profile.json` (config) keeps `name` beside the picture `source`. Files written before names existed, or while a Steam picture could be square, still read.
