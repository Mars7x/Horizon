# Repository maintenance

Keep one current guide per concern. Update its existing contract when behavior
changes; do not append phase logs, dated status summaries or abandoned plans.
[The index](INDEX.md) lists every active guide. New behavior should arrive with
its source change, relevant tests and documentation update in the same review.

## Documentation lifecycle

- Current guides describe only implemented behavior and explicit limitations.
- Accepted durable decisions belong in numbered [ADRs](adr/), using the next
  unused identifier across current and historical records.
- On retirement, update current entry points and move the old decision/supporting
  notes into [history](history/INDEX.md). Mark reversals and retain the evidence.
- History is reference material, not onboarding or pending work. New work does
  not grow the archived phase guides. Use Git for routine edit history.
- Future directions remain clearly unimplemented. Do not document an API/core
  choice as settled before its decision and implementation exist.
- Keep README blank. Preserve license texts, attribution, user data, migrations
  and authored assets; cleanup is not permission to rewrite these.

## Automated guardrails

[The repository checker](../scripts/check-repository.py) validates indexed current
guides and local Markdown links, rejects phase headings in active guides, checks
Rust/runtime/schema documentation against manifests/source, verifies lockfile
references and fixed migration checksums, audits source module declarations,
checks retired provider dependencies/API paths and the explicit sandbox policy.
[Its regression tests](../scripts/test-repository-checks.py) exercise failures,
not just the repository's passing state. History's old code references are not
current-path promises and are excluded from current link validation.

[scripts/repository-contracts.json](../scripts/repository-contracts.json) records
protected migrations and reviewed sandbox permissions. Append new migrations;
never change an existing checksum to disguise a schema rewrite. A permission or
retired-integration policy change needs explicit user direction, an ADR and an
updated contract, not a workaround to silence the checker.

The offline gate runs in [CI](../.github/workflows/repository.yml) and before
Rust checks locally. Maintain its coverage as new boundaries appear. Structural
checks cannot prove semantic documentation accuracy, runtime safety or successful
compilation; source review and actual build/UI testing remain required.

## Dependency and generated-file hygiene

Remove a direct dependency only after checking all source/build consumers.
Keep the lockfile synchronized without unrelated upgrades. Source-generation
helpers use the existing lockfile and must not silently resolve new versions.
Builds, caches, credentials and generated artifacts do not belong in patch
packages. Ignored local build output is not source and is not automatically
deleted by repository cleanup. Commit and push only with explicit approval.
