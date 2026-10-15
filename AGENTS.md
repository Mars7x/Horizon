# Horizon — agent instructions

Read [docs/INDEX.md](docs/INDEX.md), [project state](docs/PROJECT_STATE.md) and
[architecture](docs/ARCHITECTURE.md) before editing. Inspect the checked-out source,
branch, working tree and relevant call sites/tests; manifests and source define
current behavior. [History](docs/history/INDEX.md) explains old decisions and is
not onboarding, a roadmap or authorization to revive a feature.

## Product boundaries

- Horizon is a Rust/Slint desktop Flatpak moving toward embedded emulator cores.
  GBA has a reviewed folder importer; no core, BIOS manager or hosting architecture
  is implemented.
  Do not choose core/process APIs, resume compositor/session work or install host
  packages without explicit direction.
- Preserve Home's identity, carousel, focus geometry, selected scaling, motion
  and marquee. Utilities remain **Album, Activity, Achievements, Settings**.
  Library is full-shell; Profile opens from Home's avatar. Routes/history/focus
  belong in Rust, never Slint. Follow [navigation](docs/NAVIGATION.md).
- Steam is profile pictures and read-only friends only through the shared account
  boundary. Keep private/unavailable distinct from empty. SteamGridDB is independent.
  No Steam screenshot/achievement/local-installation access, Shop, external launcher
  or host command execution. RetroAchievements is planned, not implemented.
- Preserve SDL/controller, keyboard/mouse, normal Wayland/fullscreen, scaling,
  ultrawide and lifecycle handling. All motion respects Reduced Motion. New full
  pages use [shared UI standards](docs/UI_STANDARDS.md), not custom frame geometry.
- Source lifetime and observed sessions are different measurements; never sum them.
  Retain historical IDs, sessions/checkpoints, user settings and artwork. Unavailable
  data is unknown, not zero. See [database](docs/DATABASE.md).

## Engineering rules

- Domain stays independent of UI/SQL/platform types. SQL stays in persistence;
  paths are chosen only by data_paths. Network/filesystem/media work stays off the
  UI thread and out of Slint. Extend shared boundaries rather than duplicate them.
- Home, Library and Activity share append-only catalog indices. Index only slots
  obtained from iteration/position/change lists. Focus frames follow animated
  covers; read-only history never takes row focus.
- Deployed migrations are immutable. Add a numbered migration; do not edit old
  SQL or bypass its checksum guard. Partial/unavailable scans cannot prune data.
- No recoverable production unwrap/expect, hidden failures, fabricated statistics,
  unnecessary dependencies or broader sandbox access. Never expose secrets or
  secret-bearing URLs in UI state, logs, docs, screenshots or commits. File
  permissions are not encryption. Account switching must reject stale responses.
- Preserve authored SVGs, attribution, SPDX/license texts and third-party notices.
  **README.md stays blank.** Do not delete user data or local build output as an
  incidental cleanup step. Follow [storage](docs/STORAGE.md) and [licensing](docs/LICENSING.md).

## Change and verification workflow

1. Make a scoped, reviewable change. Keep current feature guides accurate rather
   than append historical phase notes. Use a new ADR for meaningful policy changes;
   archive retired material under docs/history. Follow [maintenance](docs/MAINTENANCE.md).
2. Run `bash scripts/check-repository.sh` (Python 3.11+ and Bash). Run the full
   `bash scripts/check.sh` when Rust/native dependencies are available: formatting,
   Clippy with warnings denied, all-target/all-feature tests and repository checks.
3. Relevant UI/packaging work also needs an actual Flatpak build and runtime review.
   Verify supported architectures individually. Installing an SDK is not compilation;
   do not infer legacy Phase 10.4.1.1 verification without the user's successful
   build report. State exactly what passed, failed or could not run.
4. Preserve existing work; no destructive Git reset/clean/revert/history rewrite.
   Do not commit or push without explicit approval for those actions.
5. Deliver only changed/new files, with exact deletions separately and exact
   git add/git rm/git commit commands. Exclude .git, build output, credentials and
   unrelated files. Full-repository ZIP only when explicitly requested.
