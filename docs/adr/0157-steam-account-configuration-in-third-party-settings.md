# ADR 0157 — Steam account configuration belongs in Third-Party settings

Status: Accepted for Phase 10.4.1 (pending build verification)

## Decision

Horizon stores a single opt-in SteamID64 / user-supplied Steam Web API key pair
behind an account-independent Rust service, separate from UI presentation.
Configuration lives under **Settings → Third-Party → Steam Account** and **not**
inside Achievements. The account page reuses the Settings key editor, including
controller input and Wayland clipboard behavior, to avoid a second input stack.
The saved key is never published to Slint. SteamID64-only entry is transient;
a complete credential pair is saved atomically in private local XDG config.

Changes to saved credentials invalidate old achievement data and detach pending
worker responses. Achievements and Activity share the connection state; future
Steam Friends may share the same account service, but its UI is not implemented.
The API key authenticates read-only Steam Web API requests, not the Steam client,
website, or an OAuth login. No new Flatpak permission, source change, or migration.

## Deliberate limitations

There is no key encryption or OS keyring in this phase. Steam's own Web API key
issuance and SteamID64 discovery remain external to Horizon. A malformed
existing account file currently returns a descriptive startup configuration
error instead of silently discarding credentials. Network authentication is
not verified at save time; Steam privacy and API failures remain unavailable
states, not zero achievement counts.
