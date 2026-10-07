# ADR 0001: Rust + Slint

Status: Accepted

## Decision

Use Rust for application logic and Slint for the UI.

## Consequences

- The application avoids a C++/Qt bridge layer.
- Slint files remain presentation-only.
- Rust owns state, behavior, persistence, importing, launching, activity, and platform integration.
