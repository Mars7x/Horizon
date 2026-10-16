# ADR 0205 — Human-readable application storage

Status: Accepted following the user's approval of purpose-first storage,
game-named save files, future state-slot layout and no migration work.
[ADR 0206](0206-save-states-and-resume.md) implements the reserved state storage
as complete per-slot bundles.

Keep Horizon's existing private XDG data/config/cache roots. Group durable data
under library, saves, album and profile. Configuration separates appearance,
controller, shared playback, per-core preferences and integrations. Cache keeps
rebuildable media only. All location selection remains in data_paths.

Saves and Album use the same system/readable-game hierarchy. Versioned game.json
holds full source identity and the stable safe filename stem. A short identity
suffix distinguishes ROMs; prefix collisions extend it. Resolve existing folders
by full identity, never title or prefix alone. Preserve allocated names when a
display title changes; reject duplicate/corrupt identity documents. Regular saves
use the game name, with one previous valid save in backups. Writes remain atomic
and off the UI thread. Album references come from game metadata rather than
machine-oriented directory names. Do not follow symlinks outside owned storage.

Reserve states/<core>/<game name>.slot-NN.{state,json,png} without creating empty
state folders or implementing serialization/loading. State metadata must later
identify ROM/core/version/format for compatibility checks. Battery saves remain
independent of the core. No other core or hosting API is introduced.

Shared freeze policy is now playback.json; mGBA values use cores/mgba.json.
This supersedes ADR 0204's single-file persistence choice, preserving its shared
policy and UI behavior. A settings action changes one atomic file; no partial
cross-file transaction is allowed.

No legacy layout detection, copying, deletion or migration is performed. Retired
files stay untouched, while the relocated database/preferences start independently.
Appearance, controller and cache paths already fit the layout and remain in use.
This exception was explicitly requested by the user; it does not permit pruning
historic data or modifying deployed SQL migrations. Retain schema version 12.

See [Storage](../STORAGE.md) for the exact current layout.
