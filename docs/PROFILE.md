# Profile and Steam friends

[The Profile page](../ui/pages/profile.slint) opens from Home's avatar and owns a
local name/picture plus a read-only Steam friends panel. It is a full-shell route;
Back/Forward use normal history. A new visit closes editors and resets the
friends viewport.

[First-run setup](ONBOARDING.md) creates a local name and placeholder picture
before entering Home. Placeholder shapes are temporary text stand-ins, not final
avatar artwork.

## Local profile and pictures

The avatar is the page's one focus. A/click opens Edit profile; Up/Down scrolls
friends. The editor offers Name, Choose a picture, Use Steam picture and Done.
A chosen image comes through the FileChooser portal and is copied/normalized
into `data/profile/pictures/` as lossless JXL; animations are decoded, cropped/fitted
and re-encoded as animated JXL with their frame timing and transparency. Original
GIF/APNG source files are imported, not saved as profile originals. Native JXL
animations are accepted too. Images are bounded to 600 frames and 128 MiB of
prepared pixels; invalid/oversized imports report an error. Reduced Motion and
background freezing keep pictures still. Retained PNG/extensionless profile copies
remain readable until explicitly removed.
Local profile preferences live in `data/profile/profile.json`. Removal deletes
Horizon's copy after confirmation, not the user's original file.

The name is whitespace-normalized, at most 32 characters, and empty means no
name. Editing retains physical-keyboard shortcuts and the [on-screen keyboard](INPUT.md).
Initials (H without a name) are the fallback when no usable picture exists.

Steam picture uses the configured Steam account, with equipped animated avatars
when available. Cached copies permit offline display; unavailable refreshes
must not replace them with fabricated content. Account changes reject stale
workers/results. Steam credentials stay in [Settings](SETTINGS.md); interactive
login is not implemented. See [profile services](../src/services/profile.rs),
[profile presentation](../src/presentation/profile.rs) and
[the shared Steam boundary](../src/services/steam_account.rs).

## Friends

[Friends services](../src/services/friends.rs) normalize the shared Steam API
response; [presentation](../src/presentation/friends.rs) manages background fetch,
sorting and bounded avatar downloads. Rows show genuine name/avatar/status and
last-online data where available. Playing friends precede online/busy/away/offline
entries. The list is read-only and unfocusable: no messages, invitations or
friend-management actions.

Lists fetch on entry when stale and on account changes, rather than blocking
navigation. Private, unreachable, unconfigured and genuinely empty lists remain
distinct. A private list is never “no friends”; failed refresh may keep a prior
usable list. Friends data stays session-local; avatar downloads use the existing
HTTPS host allowlist without credential-bearing image URLs.

Steam integration is limited to these profile pictures and friends. SteamGridDB
is a separate [artwork service](ARTWORK.md), not a Steam login or game source.
