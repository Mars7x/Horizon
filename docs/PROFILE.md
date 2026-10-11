# Profile — current state

The profile is the user's name and picture on this device (ADRs 0176, 0177). It is not an account, and nothing in it leaves the device.

## Opening it

- **Pointer:** click the avatar at the top left of Home.
- **Controller or keyboard:** the avatar is part of the top row, to the left of the first utility. Left from Friends reaches it, held or not, and the row ends there. Right goes back to Friends, Down goes to the games, and A opens the Profile page.
  - On the avatar it gets the utilities' single-stroke focus ring and their 2 px lift, and no utility shows focus.
  - Up from a game it was left from returns to Friends.
- The **Profile page** (`AppRoute::Profile`) is a full-shell page with history: Back returns where you were, and Forward reopens it. Every visit starts on "Change picture" with nothing open.

## The page

The page is the profile itself, not a list of options (`pages/profile.slint`). Centred under the shared header: the avatar large (184 px), the name beneath it ("No name yet" without one), a line saying where the picture comes from ("Picture from this device", "Steam profile picture", "No picture yet"), and two buttons, **Change picture** and **Edit name**. A line under them reports results ("Name saved."). Clicking the avatar is Change picture; clicking the name is Edit name.

Controller: Left and Right move between the two buttons (a fresh press at an end wraps, a held one stops), and A opens the one selected.

### Change picture

A dialog with a live preview of the avatar on the left and the picture's options on the right, in Settings' row style (`components/settings-row.slint`), then **Done**:

| Option | Does |
| --- | --- |
| Choose a picture | The FileChooser portal. Any PNG, JPEG, GIF or JPEG XL becomes Horizon's square copy, and an animated one plays. |
| Use Steam picture | A switch, usable once the Steam Account is set up in Settings → Third-Party. It uses the Steam avatar, animated if it is. |
| Remove chosen picture | Deletes Horizon's copy of the chosen picture. Usable while one exists. |

The preview changes as soon as an option does, and results ("Picture updated.", "Couldn't use that image.") show beside Done. Up and Down move through the options that can be used now, then Done; an option that stops being usable (a picture just removed) hands the selection on. A does the option; B or Done closes the dialog, back on Change picture. Hints are Close/Select.

### Edit name

A dialog with the text field, Cancel and Save. The field takes focus with the name selected. Enter or Save keeps it; Escape, B or Cancel closes it. Names are tidied (single spaces, at most 32 characters); empty means no name. Down from the field goes to Save, Left and Right move between Cancel and Save, and Up returns to the field. Hints are Cancel/Select.

## The avatar

Without a picture, the avatar shows the first letter of the name (Horizon's "H" without one). Its look, sources, storage and animation are described in [HOME_UI.md](HOME_UI.md) ("Profile avatar") and ADR 0176.

`profile.json` (config) keeps `name` beside the picture `source`. Files written before names existed, or while a Steam picture could be square, still read.
