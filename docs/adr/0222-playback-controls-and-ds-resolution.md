# ADR 0222 — Playback controls and DS resolution

Status: Accepted. The user requested a 1×/2× toggle, paired A/B and X/Y swapping,
and DS resolution through 8×. They chose to preserve higher-resolution 3D when
using filters rather than force native rendering.

The quick menu uses the shared Fast-forward (2×) toggle. Left/Right turns it
off/on; Select and pointer activation toggle it. Held fast-forward requests the
same 2× pace and restores the selected pace on release. Playback starts at 1×.
There are no 3×/4× playback presets or persisted speed settings to migrate.
This supersedes the speed choices in ADRs 0208 and 0220.

Swap Select and Back now exchanges both face-button pairs after device-family
normalization: A/B and X/Y, including mapped gameplay actions. Mapping rows show
and edit effective assignments. Swap Hints controls whether both pairs follow
the preference visually. Keyboard controls and existing saved mappings remain
intact. This is a relative swap, so already-Nintendo controller layouts invert
in the opposite direction when enabled.

DS internal resolution offers Native through 8× (1536p per screen), using the
existing OpenGL worker boundary and frame limits. Native retains software
rendering. LCD applies its native subpixel/grid pattern over the upscaled
pixels without reducing 3D detail or allocating a threefold intermediate.
Smooth pixel art remains a native-resolution filter; above Native it uses
Smooth and Settings explains the effective behavior. None and Smooth preserve
the higher-resolution image. This supersedes ADR 0218's sixfold limit and
blanket Smooth substitution for upscaled LCD.

Large-and-small screenshots expand only native frames threefold for whole
pixels. Upscaled screenshots retain their source resolution and average the
small screen down, bounding allocation at high internal resolutions. State and
rewind previews remain native-sized; stored user data and core APIs are unchanged.

Pure tests cover the two speeds, resolution bounds, paired swaps, LCD pattern,
color balance, per-screen separation and screenshot dimensions through 8×.
Native builds and GPU/controller runtime verification must be reported separately;
passing pixel tests does not establish performance or driver support at 8×.
