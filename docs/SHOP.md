# Shop

The Shop is Flathub with a controller, and where Flatpak apps are managed
(ADR 0181): browse and search Flathub, then install, update and uninstall
apps. Horizon does this through the host's own `flatpak`.

## What it shows

- **Header values:** *Apps* (how many the listing has) and *Category*
  (LB/RB, or a click): Games, All apps, Flathub's main categories (Audio &
  Video, Development, Education, Graphics, Internet, Office, Science,
  System, Utilities), then **Installed**. During a search it reads *Search*.
  Every visit starts on Games; LB from Games reaches Installed.
- **Search field** above the grid: Up from the first row, or a click. A
  opens the on-screen keyboard (controller, touch) or types (keyboard). The
  results follow the typing after a short pause; Done, Enter, B or Down end
  it and focus the first result. X (or the field's ×) clears it.
- **Grid** of app cards: icon, name, developer (with Flathub's verified
  tick), summary, and a badge: *Installed*, *Update* (accent), or the job on
  it (*Installing*, *Updating*, *Uninstalling*, *Waiting*) with a progress
  bar. Most installed first; Installed lists updates first, then by name.
  More apps load as the selection nears the end.
- **An app's page** (A): icon, name, developer, summary, its buttons, the
  description (Up/Down scroll it) and the screenshots (LB/RB, or a click on
  either half). Left/Right move between the buttons. B goes back to the
  grid where it was.

| App is | Buttons (A on the focused one) |
| --- | --- |
| Not installed | Install |
| Installed | Update (only when one is waiting), Uninstall |
| Being worked on | none: the job and its progress instead |

Uninstall asks first ("Uninstall *name*?", *Its saved data stays*: data in
`~/.var/app` is kept). Horizon never offers to uninstall itself. A job that
fails says so under the buttons ("Couldn't install", "Couldn't update",
"Couldn't uninstall").

Only apps built for this machine's architecture are listed. A listing
Flathub couldn't send says "Can't reach Flathub right now"; a search with no
results says "Nothing found".

## Installing and managing

`src/platform/flatpak_host.rs` is Horizon's one host-execution path. In the
sandbox it runs `flatpak-spawn --host flatpak …`
(`--talk-name=org.freedesktop.Flatpak`); outside it, `flatpak`. It only
runs these, each with an app ID checked by `valid_app_id` (reverse DNS,
plain characters, starting with a letter, so never an option) and passed as
one argument, never through a shell:

| What | Command |
| --- | --- |
| Installed apps | `flatpak list --app --columns=application,name,installation,origin` |
| Updates waiting | `flatpak remote-ls --updates --app --columns=application` |
| Install | `flatpak install --system --noninteractive -y flathub <id>` when the system has Flathub (as GNOME Software does); if that's refused or there's none, `flatpak remote-add --user --if-not-exists flathub …` then `flatpak install --user …` |
| Update | `flatpak update --<installation> --noninteractive -y <id>` |
| Uninstall | `flatpak uninstall --<installation> --noninteractive -y <id>` |

Jobs run one at a time; others wait in a queue (*Waiting*). Progress is the
last percentage flatpak prints, or a sliding bar while it prints none.
After each job the installed list and updates are read again. What's
installed is read on every visit.

Without host Flatpak access (a development run without the permission),
Install falls back to opening the software store on the app
(`appstream://<id>` through OpenURI), or Flathub's page.

## Data

`services/flathub.rs` talks to Flathub's public API
(`https://flathub.org/api/v2`): `POST /search` for every listing (an empty
query with a category filter browses; the architecture filter always
applies) and `GET /appstream/<app-id>` for an app's page and for the
Installed list's summaries. Icons and screenshots download only from
`dl.flathub.org` over HTTPS, at most 8 MB each, decoded off the UI thread
and kept in memory. Each Flathub category's first pages stay in memory for
30 minutes. Nothing is written to disk.

## Code

- `src/services/flathub.rs`: API client, parsing, app ID check, links.
- `src/platform/flatpak_host.rs`: the host's Flatpak.
- `src/presentation/shop.rs`: `ShopController`: listing, paging, search,
  the app page and its buttons, the job queue.
- `ui/pages/shop.slint`, `ui/models/shop.slint`.
- On-screen keyboard target 2 is the search field (`TextTarget::ShopSearch`).

## Known limits

- Apps installed here don't appear in the Library; Steam and Heroic are
  the only game sources.
- A system install on a machine whose polkit asks for a password shows the
  desktop's password prompt; under gamescope there may be none, so it falls
  back to installing for the user.
- Untested on a handheld and under gamescope.
