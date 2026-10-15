# ADR 0178: Steam friends on the Profile page; no Friends utility

**Status:** Implemented. The build, the gate and the new tests pass (friend list and summary parsing, presence and its wording, sorting, the five-utility top row with the avatar left of Album). The page was rendered offscreen with sample friends, light and dark. Not yet tried against a real Steam account in the running app.

## Context

The Friends utility was a placeholder page ("Friends and social activity will appear here."), kept until Steam friends were asked for (AGENTS.md, PROJECT_STATE). The user asked to remove the Friends button and put friends in the Profile page instead, and chose a real Steam friends list over another placeholder.

## Decision

- **Five utilities.** `UtilityPage::Friends`, `TopUtility::Friends` and AppRouteView `friends` are gone; the row is Album, Activity, Achievements, Settings, Shop (`utility-row-width` five cells). The avatar now sits left of Album, and Up from a game left from the avatar returns to Album. The orange Friends artwork stays, as the Profile page's Friends heading.
- **The Profile page** keeps its profile as a card on the left (avatar 168 px and the name) and lists Steam friends on the right.
- **One focus, one dialog.** The user didn't want the picture's source line or the Change picture / Edit name buttons on the page itself. The card's avatar is now the page's only focus (Home's ring and lift); A, or a click on the avatar or name, opens **Edit profile**: Name (opening the name editor over it), Choose a picture, Use Steam picture, Remove chosen picture, and Done, beside a live preview. This replaces ADR 0177's two buttons and separate picture dialog. Hints are Back / Edit profile on the page.
- **Read-only list.** Rows are the shared `Surface`, not focusable: avatar with a presence dot, name, and "Playing *game*", Online, Away, Busy or "Last online …". Up/Down scroll it by rows (Rust-owned `profile-friends-row`, clamped with the page's `friends-visible-rows`); the wheel and touchpad use `ScrollCamera` as every list does. In-game first, then online, busy, away, offline; by name within each.
- **Data.** `services::friends` parses and sorts; `SteamWebApiClient::friends` asks GetFriendList, then GetPlayerSummaries 100 at a time; `friend_avatar` downloads Steam's 64 px avatars from the avatar hosts only, without the key. `presentation::friends::FriendsController` runs both on workers (avatars four at a time, kept by URL for the session), fetches when Profile opens if the list is over a minute old and when the Steam account changes, and ignores an answer from before an account change. Nothing is written to disk.
- **Unknown is not zero.** A 401 from GetFriendList is a private friend list and says so; a failed fetch with no earlier list says Steam couldn't be reached; a failed refresh keeps the last list. Only a real empty list reads "No friends on Steam yet".

## Consequences

- Presence is as fresh as the last fetch (at most a minute old on opening the page); it does not update while the page stays open.
- A large friend list mounts every row; rows outside the view are hidden, not unmounted. Steam's 2,000-friend cap keeps this bounded.
- Errors and logs never carry the request URL (`without_url()`), which holds the key.

## Follow-up: no wrap in the top row

The utility row no longer wraps: a fresh Right on Shop used to jump to Album. Now the avatar is the row's left end and Shop its right end, held or fresh (`ShellFocus::move_utility` clamps; its `repeated` argument is gone). The Home game row's fresh-press wrap is unchanged.
